fn emit_terminal(
    app: &AppHandle,
    event_name: &str,
    envelope: &ridge_term::terminal_protocol::TerminalFrame,
) -> bool {
    let Ok(frame) = ridge_term::terminal_protocol::encode_frame(envelope) else {
        return false;
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(frame);
    app.emit(event_name, serde_json::json!({ "b64": b64 }))
        .is_ok()
}

