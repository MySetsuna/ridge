//! ACP framing layer — stdio JSON-RPC 2.0 + Content-Length headers.
//!
//! This is the boundary contract between `ridge-acp` and any external
//! ACP-compatible agent. It is intentionally minimal: only the methods and
//! notifications that M1 actually exercises, plus serde tagging so Mobile /
//! Desktop consumers can decode the stream without parsing shell ANSI.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// ACP protocol version this adapter speaks. ACP is evolving; we pin M1 to
/// v1. Canonical ACP v1 uses a numeric version; we emit the integer
/// `1` (matching the `agent-client-protocol` schema) and accept both
/// `1` and `"v1"` for backwards compatibility with the live_acp_agent
/// MOCK_FIXTURE.
pub const ACP_PROTOCOL_VERSION: u32 = 1;

// JSON-RPC 2.0 method names.
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_SESSION_NEW: &str = "session/new";
pub const METHOD_SESSION_PROMPT: &str = "session/prompt";
pub const METHOD_SESSION_CANCEL: &str = "session/cancel";
/// `session/approve` is an M1 extension — ACP spec v1 has no approval method.
/// Labeled explicitly so consumers know it is Ridge-side, not upstream.
pub const METHOD_SESSION_APPROVE: &str = "session/approve";

// Agent -> host notifications (server-pushed).
pub const NOTIFY_SESSION_UPDATE: &str = "session/update";
pub const NOTIFY_AGENT_THINKING_CHUNK: &str = "agent/thinking_chunk";
pub const NOTIFY_AGENT_MESSAGE_CHUNK: &str = "agent/message_chunk";
pub const NOTIFY_AGENT_TOOL_CALL: &str = "agent/tool_call";
pub const NOTIFY_AGENT_TOOL_RESULT: &str = "agent/tool_result";
pub const NOTIFY_AGENT_REQUEST_APPROVAL: &str = "agent/request_approval";

// ---------- JSON-RPC 2.0 envelopes ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRequest {
    pub jsonrpc: String, // always "2.0"
    pub id: i64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpResponse {
    pub jsonrpc: String,
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpErrorObj>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcpErrorObj {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ---------- Initialize ----------
//
// Canonical ACP v1 wire form:
//   {
//     "jsonrpc": "2.0",
//     "id": <int>,
//     "method": "initialize",
//     "params": {
//       "protocolVersion": <int>,            // 1
//       "clientInfo": { "name": ..., "version": ... },
//       "capabilities": { ... }                // structured object
//     }
//   }
//
//   {
//     "jsonrpc": "2.0",
//     "id": <int>,
//     "result": {
//       "protocolVersion": <int>,
//       "agentInfo": { "name": ..., "version": ... },
//       "agentCapabilities": { ... },
//       "authMethods": [ ... ]
//     }
//   }
//
// We accept the legacy MOCK_FIXTURE shape too (`protocolVersion: "v1"`,
// `client: {...}`, `capabilities: Vec<String>`) so `live_acp_agent`
// continues to work as the deterministic protocol fixture.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    #[serde(
        default,
        rename = "protocolVersion",
        alias = "protocolVersion",
        alias = "protocol_version"
    )]
    pub protocol_version: ProtocolVersionField,
    #[serde(
        default,
        rename = "clientInfo",
        alias = "clientInfo",
        alias = "client"
    )]
    pub client_info: ClientInfoField,
    #[serde(default)]
    pub capabilities: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(
        default,
        rename = "protocolVersion",
        alias = "protocolVersion",
        alias = "protocol_version"
    )]
    pub protocol_version: ProtocolVersionField,
    #[serde(
        default,
        rename = "agentInfo",
        alias = "agentInfo",
        alias = "agent"
    )]
    pub agent_info: ClientInfoField,
    #[serde(
        default,
        rename = "agentCapabilities",
        alias = "agentCapabilities",
        alias = "capabilities"
    )]
    pub agent_capabilities: Map<String, Value>,
    #[serde(default, alias = "authMethods", alias = "auth_methods")]
    /// Canonical ACP returns objects (e.g. `{name, description, id}`);
    /// `live_acp_agent` returns string labels. Accept either via
    /// `serde_json::Value` so we do not reject legitimate producers
    /// that pick the more descriptive shape. Mobile UI never inspects
    /// this field — it is recorded as host-side audit only.
    pub auth_methods: Vec<Value>,
}

/// `protocolVersion` is `u32` in canonical ACP. We keep a permissive
/// enum so legacy `"v1"` (string) from `live_acp_agent` continues to
/// deserialize — we do not fabricate a value when missing.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProtocolVersionField(pub u32);

impl From<u32> for ProtocolVersionField {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl Serialize for ProtocolVersionField {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersionField {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = serde_json::Value::deserialize(de)?;
        match v {
            serde_json::Value::Number(n) => n
                .as_u64()
                .map(|n| ProtocolVersionField(n as u32))
                .ok_or_else(|| D::Error::custom("protocolVersion not u32")),
            serde_json::Value::String(s) if s == "v1" => Ok(ProtocolVersionField(1)),
            serde_json::Value::String(s) => Err(D::Error::custom(format!(
                "protocolVersion string '{s}' not supported"
            ))),
            serde_json::Value::Null => Ok(ProtocolVersionField::default()),
            other => Err(D::Error::custom(format!(
                "protocolVersion must be number or 'v1', got {other:?}"
            ))),
        }
    }
}

/// `clientInfo` / `agentInfo` are structured objects in canonical ACP.
/// We accept a free-form JSON map (the canonical schema has named
/// fields `name` + `version`) but default to `{}` when missing.
#[derive(Debug, Clone, Default)]
pub struct ClientInfoField(pub Map<String, Value>);

impl From<Map<String, Value>> for ClientInfoField {
    fn from(m: Map<String, Value>) -> Self {
        Self(m)
    }
}

impl Serialize for ClientInfoField {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for ClientInfoField {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(de)?;
        match v {
            serde_json::Value::Object(m) => Ok(ClientInfoField(m)),
            serde_json::Value::Null => Ok(ClientInfoField::default()),
            other => Err(serde::de::Error::custom(format!(
                "client/agent info must be object, got {other:?}"
            ))),
        }
    }
}

// ---------- session/new ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewParams {
    /// Canonical ACP allows `cwd` to be absent or a string. Some
    /// producers (opencode) require a concrete string — `null`,
    /// `undefined`, and absent all fail. We default to empty string
    /// when missing; producers that want an actual cwd can set it
    /// before serializing.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Canonical ACP uses `mcpServers`. Wind's M1 fixture accepted
    /// `mcp_servers`; canonical producers (opencode) require camelCase.
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<Value>,
}

impl Default for SessionNewParams {
    fn default() -> Self {
        Self {
            cwd: Some(String::new()),
            mcp_servers: Vec::new(),
        }
    }
}

impl SessionNewParams {
    /// Build with a host-known working directory. Mobile side calls
    /// this with `None` to mean "no cwd" — we serialize `""` rather
    /// than `null` so canonical producers that reject nulls (opencode)
    /// accept the call.
    pub fn with_cwd(cwd: Option<String>) -> Self {
        Self {
            cwd: cwd.or_else(|| Some(String::new())),
            mcp_servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNewResult {
    /// Canonical ACP uses `sessionId`. Wind's M1 fixture accepted
    /// `session_id`; canonical producers (opencode) require camelCase.
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

// ---------- session/prompt ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptParams {
    /// Canonical ACP uses `sessionId`. Wind's M1 fixture accepted
    /// `session_id`; canonical producers (opencode) require camelCase.
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub prompt: Vec<PromptPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptPart {
    Text { text: String },
    ResourceLink {
        #[serde(default, rename = "uri")]
        uri: String,
        #[serde(default, rename = "mimeType")]
        mime_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPromptResult {
    /// Canonical ACP uses `turnId`.
    #[serde(rename = "turnId")]
    pub turn_id: String,
    /// `complete | cancelled | failed` — surfaced to host as terminal outcome.
    pub status: String,
}

// ---------- session/cancel ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCancelParams {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(default, rename = "turnId")]
    pub turn_id: Option<String>,
}

// ---------- session/update (agent -> host) ----------
//
// Wire-side this is a JSON-RPC notification whose `params` carries a
// `SessionUpdate` enum. The enum's `kind` tag is what consumers dispatch on,
// so Mobile / Desktop never need to peek at JSON-RPC internals.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionUpdate {
    Plan { plan: PlanPayload },
    ThinkingChunk { text: String, turn_id: String },
    MessageChunk { text: String, turn_id: String },
    ToolCallStarted { tool_call: ToolCallPayload, turn_id: String },
    ToolCallFinished {
        tool_call_id: String,
        turn_id: String,
        status: String, // `succeeded | failed | cancelled`
    },
    ApprovalRequested {
        tool_call_id: String,
        turn_id: String,
        reason: String,
    },
    TurnCompleted { turn_id: String, status: String },
    TurnFailed { turn_id: String, error: AcpErrorObj },
    SessionClosed { session_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanPayload {
    pub entries: Vec<PlanEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntry {
    pub id: String,
    pub title: String,
    pub status: String, // `pending | in_progress | done | failed`
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallPayload {
    pub id: String,
    pub name: String,
    pub arguments: Map<String, Value>,
    /// `read | write | shell | net | other` — coarse kind for the host UI.
    pub kind: String,
}

// ---------- Framing helpers (Content-Length over stdio) ----------

/// Build a Content-Length framed JSON-RPC byte vector.
pub fn encode_frame(envelope: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    let body = serde_json::to_vec(envelope)?;
    let mut out = Vec::with_capacity(body.len() + 32);
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Read one Content-Length framed message from a buffered stream. Returns
/// `Ok(None)` on EOF without a complete frame.
pub fn decode_frame(header_end: usize, buf: &[u8]) -> Option<FrameSplit<'_>> {
    let header = std::str::from_utf8(&buf[..header_end]).ok()?;
    let len_line = header
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))?;
    let n: usize = len_line.trim_start_matches("Content-Length:").trim().parse().ok()?;
    let body_start = header_end + 4; // "\r\n\r\n"
    if buf.len() < body_start + n {
        return None;
    }
    Some(FrameSplit {
        body: &buf[body_start..body_start + n],
    })
}

/// Locate the `\r\n\r\n` header terminator in `buf`. Returns the index of the
/// first `\r` of the terminator, or `None` if not yet present.
pub fn locate_header_end(buf: &[u8]) -> Option<usize> {
    find_subsequence(buf, b"\r\n\r\n")
}

fn find_subsequence(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

pub struct FrameSplit<'a> {
    pub body: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request_frame() {
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id: 1,
            method: METHOD_SESSION_PROMPT.into(),
            params: Some(serde_json::json!({"session_id": "abc"})),
        };
        let framed = encode_frame(&req).unwrap();
        let header_end = locate_header_end(&framed).unwrap();
        let split = decode_frame(header_end, &framed).unwrap();
        let parsed: AcpRequest = serde_json::from_slice(split.body).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.method, METHOD_SESSION_PROMPT);
    }

    #[test]
    fn session_update_tag_roundtrip() {
        let u = SessionUpdate::MessageChunk {
            text: "hi".into(),
            turn_id: "t1".into(),
        };
        let v = serde_json::to_value(&u).unwrap();
        assert_eq!(v["kind"], "message_chunk");
        let back: SessionUpdate = serde_json::from_value(v).unwrap();
        matches!(back, SessionUpdate::MessageChunk { .. });
    }

    #[test]
    fn canonical_initialize_emits_numeric_protocol_version() {
        // Iteration 3.4 §②: the canonical ACP wire shape uses a numeric
        // `protocolVersion` and structured `clientInfo` + `capabilities`.
        // Mobile must never see raw vendor types; the host emits canonical.
        let params = InitializeParams {
            protocol_version: ProtocolVersionField::from(ACP_PROTOCOL_VERSION),
            client_info: ClientInfoField(serde_json::Map::from_iter([
                ("name".into(), serde_json::json!("ridge-mobile")),
                ("version".into(), serde_json::json!("0.1.0")),
            ])),
            capabilities: serde_json::Map::from_iter([
                ("fs".into(), serde_json::json!({"read": true, "write": true})),
                ("terminal".into(), serde_json::json!(true)),
            ]),
        };
        let v = serde_json::to_value(&params).unwrap();
        assert_eq!(v["protocolVersion"], serde_json::json!(1));
        assert_eq!(v["clientInfo"]["name"], "ridge-mobile");
        assert_eq!(v["capabilities"]["fs"]["read"], true);
        // Legacy keys must NOT be present.
        assert!(v.get("protocol_version").is_none(), "snake-case leak");
        assert!(v.get("client").is_none(), "legacy client key leak");
    }

    #[test]
    fn legacy_initialize_deserializes_for_back_compat() {
        // live_acp_agent MOCK_FIXTURE legacy shape: `protocol_version:
        // "v1"` + `client: {...}` + `capabilities: Vec<String>`. The canonical
        // adapter must continue to ingest that shape and remap.
        let v = serde_json::json!({
            "protocol_version": "v1",
            "client": {
                "name": "live_acp_agent",
                "version": "0.1",
            },
            "capabilities": {
                "tools": true,
                "streaming": true,
                "approvals": true,
            },
        });
        let params: InitializeParams = serde_json::from_value(v).unwrap();
        assert_eq!(params.protocol_version.0, 1);
        assert_eq!(params.client_info.0.get("name").unwrap(), "live_acp_agent");
        assert_eq!(params.capabilities.get("streaming"), Some(&serde_json::json!(true)));
    }

    #[test]
    fn canonical_initialize_result_parses_opencode_shape() {
        // OpenCode 2.x emits numeric protocolVersion + structured
        // agentInfo + agentCapabilities + authMethods. The canonical
        // adapter must ingest this without losing information.
        let v = serde_json::json!({
            "protocolVersion": 1,
            "agentInfo": {
                "name": "opencode",
                "version": "2.0.6",
            },
            "agentCapabilities": {
                "loadSession": true,
                "promptMedia": false,
            },
            "authMethods": ["token", "oauth"],
        });
        let res: InitializeResult = serde_json::from_value(v).unwrap();
        assert_eq!(res.protocol_version.0, 1);
        assert_eq!(
            res.agent_info.0.get("name").unwrap().as_str().unwrap(),
            "opencode"
        );
        assert!(res.agent_capabilities.get("loadSession").unwrap().as_bool().unwrap());
        assert_eq!(res.auth_methods, vec!["token", "oauth"]);
    }
}
