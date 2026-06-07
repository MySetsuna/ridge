//! `ridge-core` error type and its boundary mappings.
//!
//! `CoreError` is the single error currency inside `ridge-core`. It is
//! deliberately **independent of Tauri's serialization** (§5.1 "错误映射边界"):
//!
//!   - the desktop thin wrapper maps it to the legacy `Result<T, String>` shape
//!     that `#[tauri::command]` already serializes (so desktop behaviour is
//!     byte-for-byte unchanged), via [`CoreError::to_command_string`];
//!   - the transport layer (S2's RPC client speaks JSON-RPC 2.0) maps it to a
//!     JSON-RPC `error` object `{ code, message, data }` via
//!     [`CoreError::to_json_rpc`].
//!
//! ## JSON-RPC error-code convention (handed to S2)
//!
//! JSON-RPC 2.0 reserves `-32768..=-32000`. We reuse the two standard codes
//! that map cleanly onto dispatch-boundary failures and allocate ridge-core's
//! own application range **above** the reserved block (positive codes) so there
//! is no collision with the spec's reserved space:
//!
//! | `CoreError` variant      | JSON-RPC `code` | meaning                                   |
//! |--------------------------|-----------------|-------------------------------------------|
//! | `MethodNotFound`         | `-32601`        | spec: method not in dispatch table        |
//! | `InvalidArgs`            | `-32602`        | spec: invalid params (bad/missing arg)    |
//! | `CapabilityDenied`       | `1001`          | host capability set forbids this method   |
//! | `ReadOnly`               | `1002`          | session is read-only, mutating call refused |
//! | `PathTraversal`          | `1003`          | path-bearing arg contained `..`           |
//! | `HostUnavailable`        | `1004`          | host state/handle not ready               |
//! | `Io`                     | `1005`          | filesystem / IO failure                   |
//! | `OutsideSandbox`         | `1006`          | path-bearing arg resolved outside allowed roots |
//! | `Internal`               | `1000`          | uncategorised command failure             |
//!
//! `code` is stable across hosts (desktop and ridge-cli emit the same codes for
//! the same failure), which is exactly the anti-drift property S7's parity
//! suite will assert.

use serde_json::json;
use uuid::Uuid;

/// JSON-RPC application error code for an uncategorised internal failure.
pub const CODE_INTERNAL: i64 = 1000;
/// JSON-RPC application error code: capability set forbids this method (D8).
pub const CODE_CAPABILITY_DENIED: i64 = 1001;
/// JSON-RPC application error code: read-only session refused a mutating call.
pub const CODE_READ_ONLY: i64 = 1002;
/// JSON-RPC application error code: path traversal (`..`) rejected.
pub const CODE_PATH_TRAVERSAL: i64 = 1003;
/// JSON-RPC application error code: host state / handle not ready.
pub const CODE_HOST_UNAVAILABLE: i64 = 1004;
/// JSON-RPC application error code: filesystem / IO failure.
pub const CODE_IO: i64 = 1005;
/// JSON-RPC application error code: path-bearing arg resolved outside the
/// host-granted workspace roots (fs sandbox / root-scoping, D8 / §5.6, R10).
pub const CODE_OUTSIDE_SANDBOX: i64 = 1006;
/// JSON-RPC application error code: referenced pane id not found in the
/// workspace graph (D11 Wave A; mirrors the desktop `AppError::PaneNotFound`).
pub const CODE_PANE_NOT_FOUND: i64 = 1007;
/// JSON-RPC spec code: method not found.
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC spec code: invalid params.
pub const CODE_INVALID_PARAMS: i64 = -32602;

/// The single error currency inside `ridge-core`.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// Method name was not in the dispatch table.
    #[error("command not available: {0}")]
    MethodNotFound(String),

    /// A required argument was missing or failed to deserialize.
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),

    /// The active capability set (D8) does not grant this method.
    #[error("command not available remotely: {0}")]
    CapabilityDenied(String),

    /// The session is read-only and the method mutates state.
    #[error("remote filesystem is read-only")]
    ReadOnly,

    /// A path-bearing argument contained a `..` traversal segment.
    #[error("path traversal rejected")]
    PathTraversal,

    /// A path-bearing argument resolved to a location outside the workspace
    /// roots the host granted (fs sandbox / root-scoping, D8 / §5.6, R10).
    #[error("path outside permitted workspace roots")]
    OutsideSandbox,

    /// A referenced pane id does not exist in the workspace graph (D11 Wave A).
    /// The message is **byte-identical** to the desktop `AppError::PaneNotFound`
    /// because the frontend string-matches `"Pane not found"` to suppress a
    /// benign activate-pane race (`RidgePane.svelte`, `ptyBridge.ts`).
    #[error("Pane not found: {0}")]
    PaneNotFound(Uuid),

    /// The host could not provide the state/handle the command needs.
    #[error("host unavailable: {0}")]
    HostUnavailable(String),

    /// Filesystem / IO failure.
    #[error("io error: {0}")]
    Io(String),

    /// Any other command-level failure (carries the handler's message).
    #[error("{0}")]
    Internal(String),
}

impl CoreError {
    /// Construct an internal error from any displayable value.
    pub fn internal(msg: impl std::fmt::Display) -> Self {
        CoreError::Internal(msg.to_string())
    }

    /// Construct an IO error from any displayable value.
    pub fn io(msg: impl std::fmt::Display) -> Self {
        CoreError::Io(msg.to_string())
    }

    /// The stable JSON-RPC error `code` for this variant (see module table).
    pub fn json_rpc_code(&self) -> i64 {
        match self {
            CoreError::MethodNotFound(_) => CODE_METHOD_NOT_FOUND,
            CoreError::InvalidArgs(_) => CODE_INVALID_PARAMS,
            CoreError::CapabilityDenied(_) => CODE_CAPABILITY_DENIED,
            CoreError::ReadOnly => CODE_READ_ONLY,
            CoreError::PathTraversal => CODE_PATH_TRAVERSAL,
            CoreError::OutsideSandbox => CODE_OUTSIDE_SANDBOX,
            CoreError::PaneNotFound(_) => CODE_PANE_NOT_FOUND,
            CoreError::HostUnavailable(_) => CODE_HOST_UNAVAILABLE,
            CoreError::Io(_) => CODE_IO,
            CoreError::Internal(_) => CODE_INTERNAL,
        }
    }

    /// Map to a JSON-RPC 2.0 `error` object `{ code, message, data }`.
    ///
    /// `data` carries the machine-readable variant tag so a client can branch
    /// without string-matching `message`. S2's RPC layer embeds this object
    /// under the response `"error"` key.
    pub fn to_json_rpc(&self) -> serde_json::Value {
        json!({
            "code": self.json_rpc_code(),
            "message": self.to_string(),
            "data": { "kind": self.kind_tag() },
        })
    }

    /// Map to the legacy desktop command error string. The historical
    /// `#[tauri::command]` handlers returned `Result<T, String>`, so the thin
    /// wrapper preserves exactly that shape — desktop behaviour is unchanged.
    pub fn to_command_string(&self) -> String {
        self.to_string()
    }

    /// Stable machine-readable tag (mirrors the variant name in `snake_case`).
    pub fn kind_tag(&self) -> &'static str {
        match self {
            CoreError::MethodNotFound(_) => "method_not_found",
            CoreError::InvalidArgs(_) => "invalid_args",
            CoreError::CapabilityDenied(_) => "capability_denied",
            CoreError::ReadOnly => "read_only",
            CoreError::PathTraversal => "path_traversal",
            CoreError::OutsideSandbox => "outside_sandbox",
            CoreError::PaneNotFound(_) => "pane_not_found",
            CoreError::HostUnavailable(_) => "host_unavailable",
            CoreError::Io(_) => "io",
            CoreError::Internal(_) => "internal",
        }
    }
}

/// A migrated handler returns this. `T` is whatever serializes onto the wire.
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_codes_are_stable_per_variant() {
        assert_eq!(CoreError::ReadOnly.json_rpc_code(), CODE_READ_ONLY);
        assert_eq!(
            CoreError::PathTraversal.json_rpc_code(),
            CODE_PATH_TRAVERSAL
        );
        assert_eq!(
            CoreError::MethodNotFound("x".into()).json_rpc_code(),
            CODE_METHOD_NOT_FOUND
        );
        assert_eq!(
            CoreError::CapabilityDenied("x".into()).json_rpc_code(),
            CODE_CAPABILITY_DENIED
        );
        assert_eq!(
            CoreError::OutsideSandbox.json_rpc_code(),
            CODE_OUTSIDE_SANDBOX
        );
        assert_eq!(CoreError::OutsideSandbox.kind_tag(), "outside_sandbox");
    }

    #[test]
    fn json_rpc_object_carries_code_message_and_kind() {
        let obj = CoreError::CapabilityDenied("set_remote_enabled".into()).to_json_rpc();
        assert_eq!(obj["code"], json!(CODE_CAPABILITY_DENIED));
        assert_eq!(obj["data"]["kind"], json!("capability_denied"));
        assert!(obj["message"]
            .as_str()
            .unwrap()
            .contains("set_remote_enabled"));
    }

    #[test]
    fn pane_not_found_message_is_frontend_compatible_and_code_stable() {
        // The frontend suppresses a benign activate-pane race by matching
        // `.includes("Pane not found")` — the message MUST contain that substring.
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let e = CoreError::PaneNotFound(id);
        assert!(e.to_command_string().contains("Pane not found"));
        assert_eq!(
            e.to_command_string(),
            "Pane not found: 00000000-0000-0000-0000-000000000001"
        );
        assert_eq!(e.json_rpc_code(), CODE_PANE_NOT_FOUND);
        assert_eq!(e.kind_tag(), "pane_not_found");
    }

    #[test]
    fn command_string_preserves_legacy_message() {
        // The desktop read-only gate historically returned exactly this string.
        assert_eq!(
            CoreError::ReadOnly.to_command_string(),
            "remote filesystem is read-only"
        );
        assert_eq!(
            CoreError::PathTraversal.to_command_string(),
            "path traversal rejected"
        );
    }
}
