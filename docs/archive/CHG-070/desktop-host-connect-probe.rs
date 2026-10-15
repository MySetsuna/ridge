    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return Err("地址不能为空".to_string());
    }
    let _ = token;
    let kind = match kind.as_str() {
        "rdg" => HostKind::Rdg,
        _ => HostKind::Remote,
    };
    let id = format!(
        "{}:{}",
        if kind == HostKind::Rdg { "rdg" } else { "lan" },
        addr
    );
    let label = label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.clone());

    let (host, port) = parse_host_port(&addr, kind)?;
    state.hosts.upsert_kernel_authoritative(HostRecord {
        id: id.clone(),
        kind,
        label: label.clone(),
        addr: addr.clone(),
        status: HostStatus::Connecting,
        detail: format!("探测 {host}:{port} …"),
        sessions: Vec::new(),
    })?;

    match probe_tcp(&host, port, 1500) {
        Ok(()) => {
            state.hosts.upsert_kernel_authoritative(HostRecord {
                id: id.clone(),
                kind,
                label,
                addr,
                status: HostStatus::Connected,
                detail: format!("TCP {host}:{port} 可达（live PTY 会话列表待协商）"),
                sessions: vec![HostSessionMeta {
                    id: "probe".into(),
                    title: "reachability-ok".into(),
                    attached: false,
                }],
            })?;
        }
        Err(e) => {
            state.hosts.upsert_kernel_authoritative(HostRecord {
                id: id.clone(),
                kind,
                label,
                addr,
                status: HostStatus::Error,
                detail: e,
                sessions: Vec::new(),
            })?;
        }
    }
    Ok(id)
}

