//! Protocol admission guards (AC4-C10).
//!
//! Pure checks for method naming / role surface before host dispatch.
//! Complements capability allowlists — does not replace REMOTE_ALLOWLIST.

/// Reject empty / whitespace method names.
pub fn is_valid_method_name(method: &str) -> bool {
    let m = method.trim();
    if m.is_empty() || m.len() > 128 {
        return false;
    }
    m.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/' || c == '$' || c == '-')
}

/// Methods that mutate host state and require elevated desktop trust.
pub fn is_desktop_privileged(method: &str) -> bool {
    matches!(
        method,
        "connect_host"
            | "disconnect_host"
            | "forget_host"
            | "attach_host_session"
            | "detach_host_session"
            | "pump_host_output"
            | "step_host_reconnect"
            | "cancel_host_reconnect"
            | "bind_mock_outbound_and_list"
            | "get_outbound_stats"
    )
}

/// Remote controllers must not invoke desktop-privileged methods.
pub fn remote_may_invoke(method: &str, is_remote_controller: bool) -> bool {
    if !is_valid_method_name(method) {
        return false;
    }
    if is_remote_controller && is_desktop_privileged(method) {
        return false;
    }
    true
}

/// Normalize legacy aliases to canonical method names.
pub fn canonicalize_method(method: &str) -> &str {
    match method.trim() {
        "write_pty" => "write_to_pty",
        "search" => "text_search",
        m => m,
    }
}

/// Admit for remote path: canonicalize then remote_may_invoke(true).
pub fn admit_remote_method(method: &str) -> Result<String, String> {
    let c = canonicalize_method(method).to_string();
    if !remote_may_invoke(&c, true) {
        return Err(format!("remote denied: {c}"));
    }
    Ok(c)
}

/// Admit for desktop path (still rejects invalid names).
pub fn admit_desktop_method(method: &str) -> Result<String, String> {
    let c = canonicalize_method(method).to_string();
    if !is_valid_method_name(&c) {
        return Err(format!("invalid method: {c}"));
    }
    Ok(c)
}

/// Batch admit: first failure short-circuits.
pub fn admit_remote_batch(methods: &[&str]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(methods.len());
    for m in methods {
        out.push(admit_remote_method(m)?);
    }
    Ok(out)
}

/// Method surface category for metrics / deny logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodCategory {
    DesktopHost,
    Teammate,
    Workspace,
    Terminal,
    Other,
}

pub fn method_category(method: &str) -> MethodCategory {
    let c = canonicalize_method(method);
    if is_desktop_privileged(c)
        || c.starts_with("host_")
        || c.contains("_host_")
        || c.ends_with("_host")
    {
        return MethodCategory::DesktopHost;
    }
    if c.contains("hitl") || c.contains("teammate") || c.contains("orchestration") {
        return MethodCategory::Teammate;
    }
    if c.contains("workspace") {
        return MethodCategory::Workspace;
    }
    if c.contains("pty") || c.contains("terminal") || c.contains("write_to") {
        return MethodCategory::Terminal;
    }
    MethodCategory::Other
}

/// Full desktop-privileged catalog (keep in sync with hosts desktop_surface).
pub fn desktop_privileged_catalog() -> &'static [&'static str] {
    &[
        "connect_host",
        "disconnect_host",
        "forget_host",
        "attach_host_session",
        "detach_host_session",
        "pump_host_output",
        "step_host_reconnect",
        "cancel_host_reconnect",
        "bind_mock_outbound_and_list",
        "get_outbound_stats",
    ]
}

/// Ensure every catalog entry is denied for remote.
pub fn assert_catalog_remote_denied() -> Result<(), String> {
    for m in desktop_privileged_catalog() {
        if remote_may_invoke(m, true) {
            return Err(format!("catalog leak: {m}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_name_validation() {
        assert!(is_valid_method_name("write_to_pty"));
        assert!(is_valid_method_name("$/hello"));
        assert!(!is_valid_method_name(""));
        assert!(!is_valid_method_name("bad name"));
        assert!(!is_valid_method_name(&"x".repeat(200)));
    }

    #[test]
    fn remote_blocked_from_desktop_host() {
        assert!(!remote_may_invoke("connect_host", true));
        assert!(remote_may_invoke("connect_host", false));
        assert!(remote_may_invoke("list_hitl_pending", true));
        assert!(!remote_may_invoke("", true));
        assert!(admit_remote_method("list_hitl_pending").is_ok());
        assert!(admit_remote_method("connect_host").is_err());
        assert_eq!(admit_remote_method("write_pty").unwrap(), "write_to_pty");
    }

    #[test]
    fn canonicalize_aliases() {
        assert_eq!(canonicalize_method("write_pty"), "write_to_pty");
        assert_eq!(canonicalize_method("search"), "text_search");
        assert_eq!(canonicalize_method("get_scm_status"), "get_scm_status");
    }

    #[test]
    fn all_desktop_privileged_are_remote_denied() {
        for m in [
            "connect_host",
            "attach_host_session",
            "pump_host_output",
            "step_host_reconnect",
            "get_outbound_stats",
        ] {
            assert!(is_desktop_privileged(m), "{m}");
            assert!(!remote_may_invoke(m, true), "{m}");
        }
    }

    #[test]
    fn catalog_and_batch_and_category() {
        assert_catalog_remote_denied().unwrap();
        let batch = admit_remote_batch(&["list_hitl_pending", "write_pty"]).unwrap();
        assert_eq!(batch[1], "write_to_pty");
        assert!(admit_remote_batch(&["connect_host"]).is_err());
        assert_eq!(method_category("connect_host"), MethodCategory::DesktopHost);
        assert_eq!(method_category("list_hitl_pending"), MethodCategory::Teammate);
        assert_eq!(method_category("write_to_pty"), MethodCategory::Terminal);
        assert!(admit_desktop_method("connect_host").is_ok());
        assert!(admit_desktop_method("").is_err());
        assert_eq!(desktop_privileged_catalog().len(), 10);
    }
}
