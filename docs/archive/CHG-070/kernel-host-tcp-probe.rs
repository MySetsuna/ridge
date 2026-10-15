fn connect(state: &AppState, args: &Value) -> CoreResult<Value> {
    let kind = args["kind"].as_str().ok_or_else(|| CoreError::InvalidArgs("missing host kind".into()))?;
    let addr = args["addr"].as_str().ok_or_else(|| CoreError::InvalidArgs("missing host address".into()))?;
    for field in ["label", "token"] {
        if args.get(field).is_some_and(|value| !value.is_null() && !value.is_string()) { return Err(CoreError::InvalidArgs(format!("invalid {field}"))); }
    }
    let label = args["label"].as_str().map(str::to_owned);
    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return Err(CoreError::InvalidArgs("empty host address".into()));
    }
    let kind = match kind {
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

    let (host, port) = ridge_core::remote::parse_host_port(&addr, kind).map_err(CoreError::InvalidArgs)?;
    upsert(state, HostRecord {
        id: id.clone(),
        kind,
        label: label.clone(),
        addr: addr.clone(),
        status: HostStatus::Connecting,
        detail: format!("探测 {host}:{port} …"),
        sessions: Vec::new(),
    })?;

    match ridge_core::remote::probe_tcp(&host, port, 1500) {
        Ok(()) => {
            upsert(state, HostRecord {
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
            upsert(state, HostRecord {
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
    Ok(json!(id))
}

fn upsert(state: &AppState, host: HostRecord) -> CoreResult<()> {
    let mut hosts = state.remote_hosts.lock().map_err(|_| CoreError::InvalidArgs("host topology unavailable".into()))?;
    let mut next = hosts.clone();
    next.upsert(host);
    let changed = next.records() != hosts.records();
    crate::registry::save_remote_hosts_at(&state.remote_hosts_path, next.records()).map_err(|error| CoreError::InvalidArgs(error.to_string()))?;
    *hosts = next;
    if changed { state.business_events.broadcast("host-list-changed", Value::Null); }
    Ok(())
}
