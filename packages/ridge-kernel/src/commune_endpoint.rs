//! A local protocol connection is authenticated by its OS peer before any wire data.
use ridge_core::process_identity::{self, ProcessInstance};
use serde::Deserialize;
use std::{
    fmt,
    net::{SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::http::uri::PathAndQuery;

#[derive(Deserialize)]
#[serde(try_from = "String")]
pub(crate) enum LocalEndpoint {
    Tcp(SocketAddrV4),
    WebSocket(SocketAddrV4, PathAndQuery),
    Unix(PathBuf),
}

impl TryFrom<String> for LocalEndpoint {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if let Ok(address) = value.parse::<SocketAddrV4>() {
            if address.ip().is_loopback() && address.port() != 0 {
                return Ok(Self::Tcp(address));
            }
        }
        if let Some(endpoint) = value.strip_prefix("ws://") {
            let (host, path) = endpoint
                .find('/')
                .map_or((endpoint, "/"), |index| endpoint.split_at(index));
            if let Ok(address) = host.parse::<SocketAddrV4>() {
                if address.ip().is_loopback()
                    && address.port() != 0
                    && path.len() <= 4096
                    && !path.starts_with("//")
                    && !path.contains(['?', '#', '\\'])
                {
                    if path == "/" {
                        return Ok(Self::Tcp(address));
                    }
                    if let Ok(path) = path.parse() {
                        return Ok(Self::WebSocket(address, path));
                    }
                }
            }
        }
        if let Some(path) = value.strip_prefix("unix://") {
            let local = Path::new(path);
            if path.len() <= 4096 && !path.contains('\0') && local_path(local) {
                return Ok(Self::Unix(local.to_path_buf()));
            }
        }
        Err("endpoint must be IPv4 loopback, a local WebSocket URL without a query or fragment, or an absolute local Unix socket".into())
    }
}

pub(crate) fn local_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
    {
        return false;
    }
    true
}

impl fmt::Display for LocalEndpoint {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(address) => address.fmt(output),
            Self::WebSocket(address, path) => write!(output, "ws://{address}{path}"),
            Self::Unix(path) => write!(output, "unix://{}", path.display()),
        }
    }
}

pub(crate) trait LocalIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> LocalIo for T {}
pub(crate) type LocalStream = Box<dyn LocalIo>;

#[derive(Clone)]
pub(crate) struct PeerBinding {
    pub owner: ProcessInstance,
    tcp: Option<(SocketAddrV4, SocketAddrV4)>,
}
impl PeerBinding {
    pub fn verify(&self) -> Result<(), String> {
        process_identity::validate(&self.owner).map_err(|error| error.to_string())?;
        if let Some((server, client)) = self.tcp {
            process_identity::owns_connection(&self.owner, server, client).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

pub(crate) fn read_token(path: Option<&Path>) -> Result<Option<String>, String> {
    let Some(path) = path else {return Ok(None);};
    if !local_path(path) {return Err("protocol credential reference must be an absolute local path".into());}
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "cannot read protocol credential reference")?;
    let mut token = String::new();
    file.take(4097).read_to_string(&mut token).map_err(|_| "invalid protocol credential")?;
    let token = token.trim().to_owned();
    if token.is_empty() || token.len() > 4096 || token.contains(['\r', '\n']) {return Err("invalid protocol credential".into());}
    Ok(Some(token))
}

impl LocalEndpoint {
    pub(crate) fn namespace(&self, owner: &ProcessInstance) -> String {
        let transport = match self {
            Self::Tcp(_) | Self::WebSocket(..) => "os-tcp",
            Self::Unix(_) => "os-uds",
        };
        format!("{transport}:{}:{}:{self}", owner.pid, owner.start_instance)
    }

    pub(crate) fn websocket_url(&self) -> String {
        match self {
            Self::Tcp(address) => format!("ws://{address}/"),
            Self::WebSocket(..) => self.to_string(),
            Self::Unix(_) => "ws://localhost/".into(),
        }
    }

    pub(crate) async fn connect(&self, owner: &ProcessInstance) -> Result<LocalStream, String> {
        process_identity::validate(owner).map_err(|error| error.to_string())?;
        tokio::time::timeout(Duration::from_secs(5), async {
            match self {
                Self::Tcp(address) | Self::WebSocket(address, _) => {
                    let stream = tokio::net::TcpStream::connect(address)
                        .await
                        .map_err(|error| error.to_string())?;
                    let SocketAddr::V4(client) =
                        stream.local_addr().map_err(|error| error.to_string())?
                    else {
                        return Err("protocol connection address family mismatch".into());
                    };
                    process_identity::owns_connection(owner, *address, client)
                        .map_err(|error| error.to_string())?;
                    Ok(Box::new(stream) as LocalStream)
                }
                Self::Unix(path) => connect_unix(path, Some(owner)).await.map(|(stream, _)| stream),
            }
        })
        .await
        .map_err(|_| "protocol endpoint connection timed out")?
    }

    pub(crate) async fn connect_attested(&self) -> Result<(LocalStream, PeerBinding), String> {
        tokio::time::timeout(Duration::from_secs(5), async {
            match self {
                Self::Tcp(server) | Self::WebSocket(server, _) => {
                    let stream = tokio::net::TcpStream::connect(server).await.map_err(|error| error.to_string())?;
                    let SocketAddr::V4(client) = stream.local_addr().map_err(|error| error.to_string())? else {
                        return Err("protocol connection address family mismatch".into());
                    };
                    let owner = process_identity::connection_owner(*server, client).map_err(|error| error.to_string())?;
                    Ok((Box::new(stream) as LocalStream, PeerBinding {owner, tcp: Some((*server, client))}))
                }
                Self::Unix(path) => {
                    let (stream, owner) = connect_unix(path, None).await?;
                    Ok((stream, PeerBinding {owner, tcp: None}))
                }
            }
        }).await.map_err(|_| "protocol endpoint connection timed out")?
    }
}

#[cfg(test)]
mod tests {
    use super::LocalEndpoint;

    #[test]
    fn loopback_websocket_preserves_the_route_and_rejects_other_authorities() {
        let url = "ws://127.0.0.1:2419/ws";
        let endpoint = LocalEndpoint::try_from(url.to_owned()).unwrap();
        assert_eq!(endpoint.to_string(), url);
        assert_eq!(endpoint.websocket_url(), url);
        for legacy in ["127.0.0.1:2419", "ws://127.0.0.1:2419", "ws://127.0.0.1:2419/"] {
            let endpoint = LocalEndpoint::try_from(legacy.to_owned()).unwrap();
            assert_eq!(endpoint.to_string(), "127.0.0.1:2419");
            assert_eq!(endpoint.websocket_url(), "ws://127.0.0.1:2419/");
        }
        for invalid in [
            "ws://0.0.0.0:2419/ws",
            "ws://127.0.0.1:0/ws",
            "ws://localhost:2419/ws",
            "ws://user@127.0.0.1:2419/ws",
            "ws://127.0.0.1:2419//example.test/ws",
            "ws://127.0.0.1:2419/ws?secret=fixture",
            "ws://127.0.0.1:2419/ws#fragment",
        ] {
            assert!(LocalEndpoint::try_from(invalid.to_owned()).is_err(), "{invalid}");
        }
    }
}

fn verify_peer(pid: Option<u32>, expected: Option<&ProcessInstance>) -> Result<ProcessInstance, String> {
    let owner = process_identity::instance(pid.ok_or("Unix socket peer PID unavailable")?).map_err(|error| error.to_string())?;
    if expected.is_some_and(|expected| *expected != owner) {
        return Err("Unix socket peer does not match the declared process".into());
    }
    process_identity::validate(&owner).map_err(|error| error.to_string())?;
    Ok(owner)
}

#[cfg(unix)]
async fn connect_unix(path: &Path, owner: Option<&ProcessInstance>) -> Result<(LocalStream, ProcessInstance), String> {
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|error| error.to_string())?;
    let peer = stream.peer_cred().map_err(|error| error.to_string())?;
    // SAFETY: geteuid reads the current process identity without dereferencing pointers.
    if peer.uid() != unsafe { libc::geteuid() } {
        return Err("Unix socket peer is owned by a different user".into());
    }
    let owner = verify_peer(peer.pid().and_then(|pid| u32::try_from(pid).ok()), owner)?;
    Ok((Box::new(stream), owner))
}

#[cfg(windows)]
async fn connect_unix(path: &Path, owner: Option<&ProcessInstance>) -> Result<(LocalStream, ProcessInstance), String> {
    use std::{
        io::{Read, Write},
        os::windows::io::{AsRawSocket, AsSocket, BorrowedSocket},
        ptr,
    };
    use tokio_util::compat::FuturesAsyncReadCompatExt;
    use windows_sys::Win32::Networking::WinSock::{
        WSAGetLastError, WSAIoctl, SIO_AF_UNIX_GETPEERPID,
    };

    struct Socket(uds_windows::UnixStream);
    impl AsSocket for Socket {
        fn as_socket(&self) -> BorrowedSocket<'_> {
            self.0.as_socket()
        }
    }
    impl Read for Socket {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buffer)
        }
    }
    impl Write for Socket {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.write(buffer)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }
    // SAFETY: the I/O methods delegate to the same socket without replacing its handle.
    unsafe impl async_io::IoSafe for Socket {}

    let path = path.to_path_buf();
    let socket = tokio::task::spawn_blocking(move || uds_windows::UnixStream::connect(path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let mut pid = 0u32;
    let mut bytes = 0u32;
    // SAFETY: the socket remains owned, and the output buffer is one initialized DWORD.
    let result = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as _,
            SIO_AF_UNIX_GETPEERPID,
            ptr::null(),
            0,
            ptr::addr_of_mut!(pid).cast(),
            4,
            &mut bytes,
            ptr::null_mut(),
            None,
        )
    };
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() }).to_string());
    }
    // Some Windows builds write the PID while reporting zero bytes.
    let owner = verify_peer(Some(pid), owner)?;
    let stream = async_io::Async::new(Socket(socket)).map_err(|error| error.to_string())?;
    Ok((Box::new(stream.compat()), owner))
}
