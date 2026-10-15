pub fn attach_host_session(
    state: State<'_, AppState>,
    host_id: String,
    session_id: String,
    workspace_id: Option<String>,
) -> Result<String, String> {
    attach_host_session_inner(&state, host_id, session_id, workspace_id)
}
