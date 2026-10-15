/// Parse `host:port` (or bare host → default 443 for https-style, 7522 for rdg heuristic).
pub fn parse_host_port(addr: &str, kind: HostKind) -> Result<(String, u16), String> {
    let addr = addr.trim();
    if addr.is_empty() {
        return Err("地址不能为空".into());
    }
    // strip scheme if present
    let stripped = addr
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let (host, port) = if let Some((h, p)) = stripped.rsplit_once(':') {
        let port: u16 = p
            .split('/')
            .next()
            .unwrap_or(p)
            .parse()
            .map_err(|_| format!("无效端口: {p}"))?;
        (h.to_string(), port)
    } else {
        let host = stripped.split('/').next().unwrap_or(stripped).to_string();
        let port = match kind {
            HostKind::Rdg => 7522,
            HostKind::Remote => 443,
        };
        (host, port)
    };
    if host.is_empty() {
        return Err("主机名为空".into());
    }
    Ok((host, port))
}

/// TCP reachability probe (V-H1 minimal live path). `timeout_ms` caps wait.
pub fn probe_tcp(host: &str, port: u16, timeout_ms: u64) -> Result<(), String> {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let addr = format!("{host}:{port}");
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|e| format!("DNS/解析失败: {e}"))?;
    let sock: SocketAddr = addrs
        .next()
        .ok_or_else(|| format!("无法解析地址: {addr}"))?;
    TcpStream::connect_timeout(&sock, Duration::from_millis(timeout_ms))
        .map_err(|e| format!("不可达 {addr}: {e}"))?;
    Ok(())
}

