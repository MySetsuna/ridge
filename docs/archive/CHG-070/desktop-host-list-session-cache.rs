pub fn list_host_sessions(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<Vec<HostSessionMeta>, String> {
    state
        .hosts
        .list_sessions(&host_id)
        .ok_or_else(|| format!("未知主机: {host_id}"))
}