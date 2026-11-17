//! OS evidence for exact process instances and local protocol socket ownership.
//! No executable-name/brand inference; no process launch or termination.
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io, net::SocketAddrV4};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInstance {
    pub pid: u32,
    pub start_instance: String,
}

pub fn instance(pid: u32) -> io::Result<ProcessInstance> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "process PID must be nonzero",
        ));
    }
    Ok(ProcessInstance {
        pid,
        start_instance: platform::start(pid)?,
    })
}
pub fn validate(expected: &ProcessInstance) -> io::Result<()> {
    if instance(expected.pid)? != *expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process instance changed",
        ));
    }
    Ok(())
}
pub fn descendant(root: &ProcessInstance, target: &ProcessInstance) -> io::Result<bool> {
    validate(root)?;
    validate(target)?;
    let mut current = target.pid;
    let mut visited = HashSet::new();
    while visited.insert(current) && visited.len() <= 256 {
        if current == root.pid {
            validate(root)?;
            validate(target)?;
            return Ok(true);
        }
        let Ok(Some(parent)) = platform::parent(current) else {
            return Ok(false);
        };
        // Parent creation must precede child creation; a recycled PPID is not ancestry.
        let (Ok(parent_start), Ok(child_start)) =
            (platform::start(parent), platform::start(current))
        else {
            return Ok(false);
        };
        if parent_start > child_start {
            return Ok(false);
        }
        current = parent;
    }
    Ok(false)
}
pub fn owns_connection(
    owner: &ProcessInstance,
    server: SocketAddrV4,
    client: SocketAddrV4,
) -> io::Result<()> {
    if !server.ip().is_loopback() || !client.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ACP OS-attested connection must be loopback",
        ));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
    loop {
        validate(owner)?;
        match platform::owns(owner.pid, server, client) {
            Ok(true) => return validate(owner),
            Ok(false) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "protocol socket belongs to a different process",
                ))
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                // Observe the same socket again. No handshake or connection retry.
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Discover the unique OS owner of this connected socket, then attest its
/// process instance on the same four-tuple. Never infer ownership from LISTEN.
pub fn connection_owner(server: SocketAddrV4, client: SocketAddrV4) -> io::Result<ProcessInstance> {
    if !server.ip().is_loopback() || !client.ip().is_loopback() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "protocol peer must be loopback"));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
    loop {
        match platform::owner(server, client) {
            Ok(pid) => {
                let peer = instance(pid)?;
                owns_connection(&peer, server, client)?;
                return Ok(peer);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock
                && std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(2)),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetProcessTimes(
            handle: *mut c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn WaitForSingleObject(handle: *mut c_void, timeout: u32) -> u32;
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(
            table: *mut c_void,
            size: *mut u32,
            ordered: i32,
            family: u32,
            class: u32,
            reserved: u32,
        ) -> u32;
    }
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub fn start(pid: u32) -> io::Result<String> {
        let handle = Handle(unsafe { OpenProcess(0x00101000, 0, pid) });
        if handle.0.is_null() {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(87) {
                io::Error::new(io::ErrorKind::NotFound, "process no longer exists")
            } else {
                error
            });
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let wait = unsafe { WaitForSingleObject(handle.0, 0) };
        if wait != 258 {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("process {pid} is not running (wait status {wait})"),
            ));
        }
        Ok(format!(
            "win:{:016x}",
            ((created.high as u64) << 32) | created.low as u64
        ))
    }
    pub fn parent(pid: u32) -> io::Result<Option<u32>> {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(
            pid,
        )]));
        let process = system
            .process(sysinfo::Pid::from_u32(pid))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "process not found"))?;
        Ok(process.parent().map(|pid| pid.as_u32()))
    }
    pub fn owns(pid: u32, server: SocketAddrV4, client: SocketAddrV4) -> io::Result<bool> {
        Ok(owner(server, client)? == pid)
    }
    pub fn owner(server: SocketAddrV4, client: SocketAddrV4) -> io::Result<u32> {
        let mut size = 0;
        let first = unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, 2, 5, 0) };
        if first != 0 && first != 122 {
            return Err(io::Error::from_raw_os_error(first as i32));
        }
        for _ in 0..4 {
            if size > 64 * 1024 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "TCP owner table exceeds budget",
                ));
            }
            let mut table = vec![0u32; (size as usize + 3) / 4];
            let result =
                unsafe { GetExtendedTcpTable(table.as_mut_ptr().cast(), &mut size, 0, 2, 5, 0) };
            if result == 122 {
                continue;
            }
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result as i32));
            }
            return connection_pid(&table, server, client);
        }
        Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "TCP owner table changed repeatedly",
        ))
    }
    #[cfg(test)]
    fn connection_owner(
        table: &[u32],
        pid: u32,
        server: SocketAddrV4,
        client: SocketAddrV4,
    ) -> io::Result<bool> {
        let actual = connection_pid(table, server, client)?;
        if actual != pid {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "protocol socket belongs to a different process"));
        }
        Ok(true)
    }
    fn connection_pid(table: &[u32], server: SocketAddrV4, client: SocketAddrV4) -> io::Result<u32> {
        let count = table.first().copied().unwrap_or(0) as usize;
        if count
            .checked_mul(6)
            .and_then(|n| n.checked_add(1))
            .is_none_or(|n| n > table.len())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid TCP owner table",
            ));
        }
        let matching = table[1..1 + count * 6]
            .chunks_exact(6)
            .filter(|row| {
                row[1] == u32::from_ne_bytes(server.ip().octets())
                    && u16::from_be(row[2] as u16) == server.port()
                    && row[3] == u32::from_ne_bytes(client.ip().octets())
                    && u16::from_be(row[4] as u16) == client.port()
            })
            .collect::<Vec<_>>();
        let owners = matching
            .iter()
            // Live Windows connections can report incorrect states, including
            // 0 and LISTEN with a nonzero peer port. Identity comes from the
            // exact four-tuple and unique PID. The caller keeps the connected
            // stream and must still complete the protocol handshake.
            .map(|row| row[5])
            .collect::<HashSet<_>>();
        if owners.is_empty() || owners == HashSet::from([0]) {
            let endpoint_rows=table[1..1+count*6].chunks_exact(6)
                .filter(|row|row[1]==u32::from_ne_bytes(server.ip().octets()) && u16::from_be(row[2] as u16)==server.port())
                .take(8).map(|row|(row[0],u16::from_be(row[4] as u16),row[5])).collect::<Vec<_>>();
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!("protocol socket ownership is not yet observable (endpoint {server}, client {client}, endpoint rows [state, peer port, PID] {endpoint_rows:?})"),
            ));
        }
        if owners.len() != 1 || owners.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "protocol socket ownership is ambiguous",
            ));
        }
        Ok(*owners.iter().next().expect("one nonzero socket owner"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn pending_publication_never_grants_or_masks_a_foreign_owner() {
            let server = "127.0.0.1:1001".parse::<SocketAddrV4>().unwrap();
            let client = "127.0.0.1:1002".parse::<SocketAddrV4>().unwrap();
            let row = |state, owner| {
                vec![
                    state,
                    u32::from_ne_bytes(server.ip().octets()),
                    u32::from(server.port().to_be()),
                    u32::from_ne_bytes(client.ip().octets()),
                    u32::from(client.port().to_be()),
                    owner,
                ]
            };
            let table = |rows: Vec<Vec<u32>>| {
                let mut table = vec![rows.len() as u32];
                table.extend(rows.into_iter().flatten());
                table
            };
            for state in [0,2,4,5,6,12] {
                assert!(connection_owner(&table(vec![row(state, 42)]), 42, server, client).unwrap());
            }
            for rows in [vec![], vec![row(5, 0)], vec![row(0, 0)]] {
                assert_eq!(
                    connection_owner(&table(rows), 42, server, client)
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::WouldBlock
                );
            }
            let mut listener=row(2,42);
            listener[3]=0;listener[4]=0;
            let mut other_peer=row(5,42);
            other_peer[4]=0;
            for unrelated in [listener,other_peer] {
                assert_eq!(connection_owner(&table(vec![unrelated]),42,server,client).unwrap_err().kind(),io::ErrorKind::WouldBlock);
            }
            for rows in [
                vec![row(5, 43)],
                vec![row(0, 43)],
                vec![row(2, 43)],
                vec![row(4, 43)],
                vec![row(6, 43)],
                vec![row(5, 42), row(5, 0)],
                vec![row(5, 42), row(5, 43)],
                vec![row(0, 42), row(0, 0)],
                vec![row(0, 42), row(0, 43)],
            ] {
                assert_eq!(
                    connection_owner(&table(rows), 42, server, client)
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::PermissionDenied
                );
            }
            assert_eq!(
                connection_owner(&[1], 42, server, client)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    fn stat(pid: u32) -> io::Result<Vec<String>> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let end = stat
            .rfind(')')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat"))?;
        let fields: Vec<_> = stat[end + 1..]
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if fields.len() < 20 || fields[0] == "Z" {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "process unavailable",
            ));
        }
        Ok(fields)
    }
    pub fn start(pid: u32) -> io::Result<String> {
        let fields = stat(pid)?;
        let ticks = fields[19].parse::<u64>().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid process start ticks")
        })?;
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        Ok(format!("linux:{}:{ticks:016x}", boot.trim()))
    }
    pub fn parent(pid: u32) -> io::Result<Option<u32>> {
        let pid = stat(pid)?[1]
            .parse::<u32>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid parent PID"))?;
        Ok((pid != 0).then_some(pid))
    }
    fn address(value: &str) -> Option<SocketAddrV4> {
        let (ip, port) = value.split_once(':')?;
        Some(SocketAddrV4::new(
            std::net::Ipv4Addr::from(u32::from_str_radix(ip, 16).ok()?.to_ne_bytes()),
            u16::from_str_radix(port, 16).ok()?,
        ))
    }
    fn socket_inodes(server: SocketAddrV4, client: SocketAddrV4) -> io::Result<HashSet<String>> {
        let table = std::fs::read_to_string("/proc/net/tcp")?;
        socket_inodes_from_table(&table, server, client)
    }
    fn socket_inodes_from_table(table: &str, server: SocketAddrV4, client: SocketAddrV4) -> io::Result<HashSet<String>> {
        let mut matching = table
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.len() > 3
                    && fields[3] == "01"
                    && address(fields[1]) == Some(server)
                    && address(fields[2]) == Some(client)
                {
                    Some(fields)
                } else {
                    None
                }
            });
        let pending = || io::Error::new(io::ErrorKind::WouldBlock, "TCP socket ownership evidence is not yet visible");
        let row = matching.next().ok_or_else(pending)?;
        if matching.next().is_some() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "TCP socket evidence is ambiguous"));
        }
        let inode = row.get(9).and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid TCP socket inode"))?;
        // Linux publishes an established backlog entry with inode 0 before
        // accept installs a file descriptor. Observe this same socket again;
        // an absent proof must never grant ownership or imply another owner.
        if inode == 0 { return Err(pending()); }
        Ok(HashSet::from([format!("socket:[{inode}]")]))
    }
    fn owns_inodes(pid: u32, inodes: &HashSet<String>) -> io::Result<bool> {
        for fd in std::fs::read_dir(format!("/proc/{pid}/fd"))? {
            if let Ok(target) = std::fs::read_link(fd?.path()) {
                if inodes.contains(target.to_string_lossy().as_ref()) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    pub fn owns(pid: u32, server: SocketAddrV4, client: SocketAddrV4) -> io::Result<bool> {
        owns_inodes(pid, &socket_inodes(server, client)?)
    }
    pub fn owner(server: SocketAddrV4, client: SocketAddrV4) -> io::Result<u32> {
        let inodes = socket_inodes(server, client)?;
        let mut owners = HashSet::new();
        for entry in std::fs::read_dir("/proc")? {
            let entry = entry?;
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()) else {continue;};
            if owns_inodes(pid, &inodes).unwrap_or(false) {owners.insert(pid);}
            if owners.len() > 1 {break;}
        }
        if owners.len() != 1 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "protocol socket owner is unavailable or ambiguous"));
        }
        Ok(*owners.iter().next().expect("unique socket owner"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn inode_evidence_requires_one_visible_exact_connection() {
            let server = "127.0.0.1:4660".parse().unwrap();
            let client = "127.0.0.1:22136".parse().unwrap();
            let row = |inode| format!("0: 0100007F:1234 0100007F:5678 01 0 0 0 0 0 {inode}");
            let table = |rows: Vec<String>| format!("header\n{}", rows.join("\n"));
            for rows in [vec![], vec![row(0)]] {
                assert_eq!(socket_inodes_from_table(&table(rows), server, client).unwrap_err().kind(), io::ErrorKind::WouldBlock);
            }
            assert_eq!(socket_inodes_from_table(&table(vec![row(42)]), server, client).unwrap(), HashSet::from(["socket:[42]".into()]));
            for rows in [vec![row(0), row(42)], vec![row(42), row(43)], vec![row(42), row(42)]] {
                assert_eq!(socket_inodes_from_table(&table(rows), server, client).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            }
        }
    }
}
#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "OS process/socket attestation unavailable on this platform",
        )
    }
    pub fn start(_: u32) -> io::Result<String> {
        Err(unsupported())
    }
    pub fn parent(_: u32) -> io::Result<Option<u32>> {
        Err(unsupported())
    }
    pub fn owns(_: u32, _: SocketAddrV4, _: SocketAddrV4) -> io::Result<bool> {
        Err(unsupported())
    }
    pub fn owner(_: SocketAddrV4, _: SocketAddrV4) -> io::Result<u32> { Err(unsupported()) }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn process_instance_and_socket_owner_are_os_facts() {
        let current = instance(std::process::id()).unwrap();
        assert_eq!(instance(current.pid).unwrap(), current);
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--exact",
            "process_identity::tests::process_identity_child_fixture",
            "--ignored",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = Child(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let child_instance = instance(child.0.id()).unwrap_or_else(|error| {
            panic!(
                "child identity failed: {error}; child status: {:?}",
                child.0.try_wait()
            )
        });
        assert!(descendant(&current, &child_instance).unwrap());
        assert!(!descendant(&child_instance, &current).unwrap());
        let mut changed = child_instance.clone();
        changed.start_instance.push('x');
        assert!(validate(&changed).is_err());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let std::net::SocketAddr::V4(local) = server.local_addr().unwrap() else {
            unreachable!()
        };
        let std::net::SocketAddr::V4(remote) = socket.local_addr().unwrap() else {
            unreachable!()
        };
        owns_connection(&current, local, remote).unwrap();
        assert_eq!(connection_owner(local, remote).unwrap(), current);
        assert!(owns_connection(&child_instance, local, remote).is_err());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(validate(&child_instance).is_err());
    }

    #[test]
    fn connection_attestation_waits_for_publication_without_reconnecting() {
        let current = instance(std::process::id()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let std::net::SocketAddr::V4(server) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        for _ in 0..32 {
            let socket = std::net::TcpStream::connect(server).unwrap();
            let std::net::SocketAddr::V4(client) = socket.local_addr().unwrap() else {
                unreachable!()
            };
            // Windows can already attest backlog ownership. Linux needs the
            // accepted FD; neither platform may open a replacement connection.
            #[cfg(windows)]
            owns_connection(&current, server, client).unwrap();
            std::thread::scope(|scope| {
                let accepted = scope.spawn(|| {
                    #[cfg(target_os = "linux")]
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    listener.accept().unwrap().0
                });
                owns_connection(&current, server, client).unwrap();
                let accepted = accepted.join().unwrap();
                assert_eq!(accepted.peer_addr().unwrap(), std::net::SocketAddr::V4(client));
            });
            drop(socket);
        }
    }

    #[cfg(windows)]
    #[test]
    fn unobservable_connection_times_out_without_granting_ownership() {
        let current = instance(std::process::id()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let std::net::SocketAddr::V4(server) = listener.local_addr().unwrap() else {
            unreachable!()
        };
        let absent_client = "127.0.0.1:0".parse().unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            owns_connection(&current, server, absent_client)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let mut stale = current;
        stale.start_instance.push('x');
        assert!(owns_connection(&stale, server, absent_client).is_err());
    }

    #[test]
    #[ignore = "owned child-process fixture"]
    fn process_identity_child_fixture() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
