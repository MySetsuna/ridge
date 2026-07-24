//! Capability matrix consistency helpers (AC4-C9).
//!
//! Machine-readable checks that the remote allowlist and teammate method
//! surface stay aligned — used by tests and can be called from scripts.

/// Methods that must appear in REMOTE_ALLOWLIST for teammate remote console.
pub const TEAMMATE_REMOTE_REQUIRED: &[&str] = &[
    "get_teammate_topology",
    "list_hitl_pending",
    "list_hitl_audit_remote",
    "resolve_hitl_remote",
    "get_orchestration_health",
];

/// Desktop-only host methods that must **never** appear in REMOTE_ALLOWLIST.
pub const DESKTOP_HOST_FORBIDDEN_REMOTE: &[&str] = &[
    "connect_host",
    "attach_host_session",
    "detach_host_session",
    "pump_host_output",
    "step_host_reconnect",
    "cancel_host_reconnect",
    "get_outbound_stats",
    "bind_mock_outbound_and_list",
];

pub fn missing_required(allowlist: &[&str], required: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|m| !allowlist.iter().any(|a| a == *m))
        .map(|s| (*s).to_string())
        .collect()
}

pub fn forbidden_present(allowlist: &[&str], forbidden: &[&str]) -> Vec<String> {
    forbidden
        .iter()
        .filter(|m| allowlist.iter().any(|a| a == *m))
        .map(|s| (*s).to_string())
        .collect()
}

pub fn validate_teammate_and_hosts_boundary(allowlist: &[&str]) -> Result<(), String> {
    let miss = missing_required(allowlist, TEAMMATE_REMOTE_REQUIRED);
    if !miss.is_empty() {
        return Err(format!("REMOTE_ALLOWLIST missing teammate methods: {miss:?}"));
    }
    let bad = forbidden_present(allowlist, DESKTOP_HOST_FORBIDDEN_REMOTE);
    if !bad.is_empty() {
        return Err(format!("REMOTE_ALLOWLIST leaks desktop host methods: {bad:?}"));
    }
    Ok(())
}

/// Parse controller-minimum methods for a capability from matrix JSON text.
pub fn teammate_methods_from_matrix_json(matrix_json: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(matrix_json).map_err(|e| format!("matrix json: {e}"))?;
    let methods = v
        .pointer("/capabilities/teammate/methods")
        .and_then(|m| m.as_array())
        .ok_or_else(|| "missing capabilities.teammate.methods".to_string())?;
    Ok(methods
        .iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect())
}

/// Ensure matrix teammate methods include every TEAMMATE_REMOTE_REQUIRED entry.
pub fn validate_matrix_teammate_methods(matrix_json: &str) -> Result<(), String> {
    let methods = teammate_methods_from_matrix_json(matrix_json)?;
    let refs: Vec<&str> = methods.iter().map(|s| s.as_str()).collect();
    let miss = missing_required(&refs, TEAMMATE_REMOTE_REQUIRED);
    if !miss.is_empty() {
        return Err(format!("matrix teammate methods missing: {miss:?}"));
    }
    Ok(())
}

/// Hosts capability methods that must remain desktop-only (never in matrix remote).
pub const HOSTS_DESKTOP_ONLY_HINTS: &[&str] = &[
    "connect_host",
    "attach_host_session",
    "detach_host_session",
    "pump_host_output",
];

/// Full dual check: allowlist boundary + matrix teammate methods.
pub fn validate_full_surface(allowlist: &[&str], matrix_json: &str) -> Result<(), String> {
    validate_teammate_and_hosts_boundary(allowlist)?;
    validate_matrix_teammate_methods(matrix_json)?;
    // Matrix must not list desktop host methods under teammate
    let methods = teammate_methods_from_matrix_json(matrix_json)?;
    let refs: Vec<&str> = methods.iter().map(|s| s.as_str()).collect();
    let bad = forbidden_present(&refs, HOSTS_DESKTOP_ONLY_HINTS);
    if !bad.is_empty() {
        return Err(format!("matrix teammate lists desktop host methods: {bad:?}"));
    }
    Ok(())
}

/// Extract all method strings under any capability for leak scans.
pub fn all_methods_from_matrix_json(matrix_json: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(matrix_json).map_err(|e| format!("matrix json: {e}"))?;
    let caps = v
        .pointer("/capabilities")
        .and_then(|c| c.as_object())
        .ok_or_else(|| "missing capabilities".to_string())?;
    let mut out = Vec::new();
    for (_k, cap) in caps {
        if let Some(arr) = cap.get("methods").and_then(|m| m.as_array()) {
            for x in arr {
                if let Some(s) = x.as_str() {
                    out.push(s.to_string());
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::REMOTE_ALLOWLIST;

    #[test]
    fn shipped_allowlist_passes_boundary() {
        validate_teammate_and_hosts_boundary(REMOTE_ALLOWLIST).expect("boundary");
    }

    #[test]
    fn detects_missing_and_forbidden() {
        let thin = &["get_teammate_topology"];
        assert!(!missing_required(thin, TEAMMATE_REMOTE_REQUIRED).is_empty());
        let leak = &["get_teammate_topology", "connect_host"];
        assert!(!forbidden_present(leak, DESKTOP_HOST_FORBIDDEN_REMOTE).is_empty());
    }

    #[test]
    fn matrix_json_teammate_methods_include_required() {
        let json = r#"{
          "capabilities": {
            "teammate": {
              "methods": [
                "get_teammate_topology",
                "list_hitl_pending",
                "list_hitl_audit_remote",
                "resolve_hitl_remote",
                "get_orchestration_health"
              ]
            }
          }
        }"#;
        validate_matrix_teammate_methods(json).expect("matrix ok");
        let methods = teammate_methods_from_matrix_json(json).unwrap();
        assert!(methods.contains(&"list_hitl_audit_remote".to_string()));
    }

    #[test]
    fn matrix_json_rejects_missing_audit() {
        let json = r#"{
          "capabilities": {
            "teammate": {
              "methods": ["get_teammate_topology", "list_hitl_pending"]
            }
          }
        }"#;
        assert!(validate_matrix_teammate_methods(json).is_err());
    }

    #[test]
    fn full_surface_and_all_methods() {
        let json = r#"{
          "capabilities": {
            "teammate": {
              "methods": [
                "get_teammate_topology",
                "list_hitl_pending",
                "list_hitl_audit_remote",
                "resolve_hitl_remote",
                "get_orchestration_health"
              ]
            },
            "terminal": { "methods": ["write_to_pty"] }
          }
        }"#;
        validate_full_surface(REMOTE_ALLOWLIST, json).expect("full");
        let all = all_methods_from_matrix_json(json).unwrap();
        assert!(all.contains(&"write_to_pty".to_string()));
        assert!(all.contains(&"list_hitl_audit_remote".to_string()));
        let leak_json = r#"{
          "capabilities": {
            "teammate": {
              "methods": [
                "get_teammate_topology",
                "list_hitl_pending",
                "list_hitl_audit_remote",
                "resolve_hitl_remote",
                "get_orchestration_health",
                "connect_host"
              ]
            }
          }
        }"#;
        assert!(validate_full_surface(REMOTE_ALLOWLIST, leak_json).is_err());
    }

    /// Product path: load shipped docs/capability-matrix.json when present.
    #[test]
    fn shipped_matrix_file_passes_when_available() {
        let candidates = [
            "docs/capability-matrix.json",
            "../docs/capability-matrix.json",
            "../../docs/capability-matrix.json",
        ];
        let mut loaded: Option<String> = None;
        for p in candidates {
            if let Ok(s) = std::fs::read_to_string(p) {
                loaded = Some(s);
                break;
            }
        }
        let Some(json) = loaded else {
            // CI/workspace layout may not resolve relative path from test cwd —
            // synthetic full_surface already covers logic.
            return;
        };
        validate_matrix_teammate_methods(&json).expect("shipped matrix teammate methods");
        // Desktop host methods must not appear under teammate methods
        let methods = teammate_methods_from_matrix_json(&json).unwrap();
        for f in HOSTS_DESKTOP_ONLY_HINTS {
            assert!(
                !methods.iter().any(|m| m == f),
                "shipped matrix teammate must not list {f}"
            );
        }
    }
}
