    let rows = sessions
        .into_iter()
        .map(|session| HostSessionMeta {
            attached: !state
                .hosts
                .panes_for_remote(&host_id, &session.id)
                .is_empty(),
            id: session.id,
            title: session.title,
        })
        .collect();
    state.hosts.upsert_kernel_authoritative(HostRecord {
        id: host_id,
        kind,
        label,
        addr: "frontend-remote-link".into(),
        status: HostStatus::Connected,
        detail: "RemoteLink topology".into(),
        sessions: rows,
    })
}

