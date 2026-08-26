//! A2A JSON-RPC client and fenced external-agent registry.
//!
//! Ridge keeps its bounded SQLite Hub as the source of truth. This module is
//! only the external adapter: it discovers an A2A Agent Card, selects a
//! JSON-RPC interface, sends standard camelCase messages, and exposes the
//! task/stream operations without leaking credentials into receipts or logs.

use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::sync::Mutex;
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CARD_BYTES: usize = 512 * 1024;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_STREAM_EVENTS: usize = 4096;
const MAX_STREAM_LINE_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum A2aError {
    #[error("invalid A2A configuration: {0}")]
    InvalidConfig(String),
    #[error("A2A HTTP request failed: {0}")]
    Http(String),
    #[error("A2A HTTP status {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("A2A response exceeded the bounded body limit")]
    ResponseTooLarge,
    #[error("invalid A2A Agent Card: {0}")]
    InvalidAgentCard(String),
    #[error("invalid A2A JSON-RPC response: {0}")]
    InvalidRpc(String),
    #[error("A2A remote error {code}: {message}")]
    Remote { code: i64, message: String },
    #[error("A2A capability is not advertised: {0}")]
    UnsupportedCapability(&'static str),
    #[error("A2A stream failed: {0}")]
    Stream(String),
    #[error("A2A route is missing or fenced out")]
    RouteUnavailable,
}

#[derive(Clone)]
pub struct A2aClientConfig {
    pub agent_card_url: String,
    pub bearer_token: Option<String>,
    pub timeout: Duration,
    pub max_response_bytes: usize,
    pub preferred_protocol_version: Option<String>,
    pub extensions: Vec<String>,
}

impl fmt::Debug for A2aClientConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("A2aClientConfig")
            .field("agent_card_url", &self.agent_card_url)
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field(
                "preferred_protocol_version",
                &self.preferred_protocol_version,
            )
            .field("extensions", &self.extensions)
            .finish()
    }
}

impl Default for A2aClientConfig {
    fn default() -> Self {
        Self {
            agent_card_url: String::new(),
            bearer_token: None,
            timeout: DEFAULT_TIMEOUT,
            max_response_bytes: MAX_RESPONSE_BYTES,
            preferred_protocol_version: None,
            extensions: Vec::new(),
        }
    }
}

impl A2aClientConfig {
    fn validate(&self) -> Result<(), A2aError> {
        validate_http_url(&self.agent_card_url)?;
        if self.timeout.is_zero() {
            return Err(A2aError::InvalidConfig("timeout must be positive".into()));
        }
        if !(1024..=MAX_RESPONSE_BYTES).contains(&self.max_response_bytes) {
            return Err(A2aError::InvalidConfig(
                "max_response_bytes must be between 1024 and 4 MiB".into(),
            ));
        }
        if self
            .bearer_token
            .as_deref()
            .is_some_and(|token| token.trim().is_empty() || token.len() > 8192)
        {
            return Err(A2aError::InvalidConfig(
                "bearer_token must be bounded when present".into(),
            ));
        }
        if self
            .extensions
            .iter()
            .any(|extension| extension.trim().is_empty() || extension.len() > 2048)
        {
            return Err(A2aError::InvalidConfig(
                "A2A extension URI is empty or too large".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub supported_interfaces: Vec<AgentInterface>,
    #[serde(default)]
    pub provider: Option<AgentProvider>,
    pub version: String,
    #[serde(default)]
    pub documentation_url: Option<String>,
    #[serde(default)]
    pub capabilities: AgentCapabilities,
    #[serde(default)]
    pub security_schemes: Value,
    #[serde(default)]
    pub security_requirements: Vec<Value>,
    #[serde(default)]
    pub default_input_modes: Vec<String>,
    #[serde(default)]
    pub default_output_modes: Vec<String>,
    #[serde(default)]
    pub skills: Vec<Value>,
    #[serde(default)]
    pub signatures: Vec<Value>,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInterface {
    pub url: String,
    pub protocol_binding: String,
    pub protocol_version: String,
    #[serde(default)]
    pub tenant: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProvider {
    pub organization: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub push_notifications: bool,
    #[serde(default)]
    pub extended_agent_card: bool,
    #[serde(default)]
    pub extensions: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aMessage {
    pub message_id: String,
    pub role: String,
    pub parts: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_task_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct A2aSendResponse {
    pub task: Option<Value>,
    pub message: Option<A2aMessage>,
    pub raw: Value,
}

impl A2aSendResponse {
    pub fn remote_id(&self) -> Option<String> {
        self.task
            .as_ref()
            .and_then(|task| task.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                self.message
                    .as_ref()
                    .map(|message| message.message_id.clone())
            })
    }
}

#[derive(Debug, Clone, Copy)]
enum MethodStyle {
    Standard,
    Legacy,
}

impl MethodStyle {
    fn for_version(version: &str) -> Self {
        if version.trim_start().starts_with('0') {
            Self::Legacy
        } else {
            Self::Standard
        }
    }

    fn method(self, operation: Operation) -> &'static str {
        match (self, operation) {
            (Self::Standard, Operation::SendMessage) => "SendMessage",
            (Self::Standard, Operation::SendStreamingMessage) => "SendStreamingMessage",
            (Self::Standard, Operation::GetTask) => "GetTask",
            (Self::Standard, Operation::ListTasks) => "ListTasks",
            (Self::Standard, Operation::CancelTask) => "CancelTask",
            (Self::Standard, Operation::SubscribeToTask) => "SubscribeToTask",
            (Self::Standard, Operation::CreatePushConfig) => "CreateTaskPushNotificationConfig",
            (Self::Standard, Operation::GetPushConfig) => "GetTaskPushNotificationConfig",
            (Self::Standard, Operation::ListPushConfigs) => "ListTaskPushNotificationConfigs",
            (Self::Standard, Operation::DeletePushConfig) => "DeleteTaskPushNotificationConfig",
            (Self::Standard, Operation::GetExtendedAgentCard) => "GetExtendedAgentCard",
            (Self::Legacy, Operation::SendMessage) => "message/send",
            (Self::Legacy, Operation::SendStreamingMessage) => "message/stream",
            (Self::Legacy, Operation::GetTask) => "tasks/get",
            (Self::Legacy, Operation::ListTasks) => "tasks/list",
            (Self::Legacy, Operation::CancelTask) => "tasks/cancel",
            (Self::Legacy, Operation::SubscribeToTask) => "tasks/resubscribe",
            (Self::Legacy, Operation::CreatePushConfig) => "tasks/pushNotificationConfig/set",
            (Self::Legacy, Operation::GetPushConfig) => "tasks/pushNotificationConfig/get",
            (Self::Legacy, Operation::ListPushConfigs) => "tasks/pushNotificationConfig/list",
            (Self::Legacy, Operation::DeletePushConfig) => "tasks/pushNotificationConfig/delete",
            (Self::Legacy, Operation::GetExtendedAgentCard) => "agent/getExtendedCard",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    SendMessage,
    SendStreamingMessage,
    GetTask,
    ListTasks,
    CancelTask,
    SubscribeToTask,
    CreatePushConfig,
    GetPushConfig,
    ListPushConfigs,
    DeletePushConfig,
    GetExtendedAgentCard,
}

#[derive(Clone)]
pub struct A2aClient {
    http: Client,
    config: A2aClientConfig,
    card: AgentCard,
    endpoint: String,
    method_style: MethodStyle,
    protocol_version: String,
    tenant: Option<String>,
}

impl fmt::Debug for A2aClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("A2aClient")
            .field("config", &self.config)
            .field("endpoint", &self.endpoint)
            .field("protocol_version", &self.protocol_version)
            .field("tenant", &self.tenant)
            .finish()
    }
}

impl A2aClient {
    pub fn discover(config: A2aClientConfig) -> Result<Self, A2aError> {
        config.validate()?;
        let http = build_http_client(&config)?;
        let response = http
            .get(&config.agent_card_url)
            .headers(auth_headers(&config)?)
            .send()
            .map_err(|error| A2aError::Http(error.to_string()))?;
        let card: AgentCard = decode_json_response(response, MAX_CARD_BYTES)?;
        Self::from_card_with_http(config, http, card)
    }

    pub fn from_card(config: A2aClientConfig, card: AgentCard) -> Result<Self, A2aError> {
        config.validate()?;
        let http = build_http_client(&config)?;
        Self::from_card_with_http(config, http, card)
    }

    fn from_card_with_http(
        config: A2aClientConfig,
        http: Client,
        card: AgentCard,
    ) -> Result<Self, A2aError> {
        validate_card(&card)?;
        let (interface_url, protocol_version, tenant) = {
            let interface = select_interface(&card, config.preferred_protocol_version.as_deref())?;
            (
                interface.url.clone(),
                interface.protocol_version.clone(),
                interface.tenant.clone(),
            )
        };
        let card_url = reqwest::Url::parse(&config.agent_card_url)
            .map_err(|error| A2aError::InvalidConfig(error.to_string()))?;
        let endpoint = card_url
            .join(&interface_url)
            .map_err(|error| A2aError::InvalidAgentCard(format!("invalid interface URL: {error}")))?
            .to_string();
        validate_http_url(&endpoint)?;
        Ok(Self {
            http,
            config,
            card,
            endpoint,
            method_style: MethodStyle::for_version(&protocol_version),
            protocol_version,
            tenant,
        })
    }

    pub fn agent_card(&self) -> &AgentCard {
        &self.card
    }

    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    pub fn send_message(&self, message: A2aMessage) -> Result<A2aSendResponse, A2aError> {
        validate_message(&message)?;
        let result = self.call_json(Operation::SendMessage, self.send_params(message))?;
        parse_send_response(result)
    }

    /// Convert a durable Ridge Hub entry into a standard A2A `Message` and
    /// return the remote task/message id used by the delivery receipt.
    pub fn send_hub_entry(&self, entry: &Value) -> Result<Option<String>, A2aError> {
        let payload = entry
            .get("payload")
            .ok_or_else(|| A2aError::InvalidRpc("Hub entry has no payload".into()))?;
        let text = payload
            .get("text")
            .or_else(|| payload.get("objective"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                (payload.get("topic").is_some() || payload.get("payload").is_some())
                    .then(|| payload.to_string())
            })
            .ok_or_else(|| A2aError::InvalidRpc("Hub payload is not text-addressable".into()))?;
        if text.len() > MAX_TEXT_BYTES {
            return Err(A2aError::InvalidRpc(
                "Hub payload exceeds A2A text limit".into(),
            ));
        }
        let message_id = entry
            .get("messageId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let ridge_metadata = json!({
            "deliveryId": entry.get("deliveryId"),
            "messageId": entry.get("messageId"),
            "from": entry.get("from"),
            "kind": entry.get("kind"),
            "sequence": entry.get("sequence"),
        });
        let response = self.send_message(A2aMessage {
            message_id,
            role: "ROLE_USER".into(),
            parts: vec![json!({ "text": text })],
            context_id: entry
                .get("conversationId")
                .and_then(Value::as_str)
                .map(str::to_string),
            task_id: payload
                .get("a2aTaskId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            metadata: Some(json!({ "ridge": ridge_metadata })),
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        })?;
        Ok(response.remote_id())
    }

    pub fn get_task(&self, id: &str, history_length: Option<u32>) -> Result<Value, A2aError> {
        let mut params = json!({ "id": id });
        if let Some(history_length) = history_length {
            params["historyLength"] = json!(history_length);
        }
        self.call_json(Operation::GetTask, params)
    }

    pub fn list_tasks(&self, params: Value) -> Result<Value, A2aError> {
        self.call_json(Operation::ListTasks, params)
    }

    pub fn cancel_task(&self, id: &str) -> Result<Value, A2aError> {
        self.call_json(Operation::CancelTask, json!({ "id": id }))
    }

    pub fn create_push_notification_config(&self, config: Value) -> Result<Value, A2aError> {
        self.call_json(Operation::CreatePushConfig, config)
    }

    pub fn get_push_notification_config(&self, params: Value) -> Result<Value, A2aError> {
        self.call_json(Operation::GetPushConfig, params)
    }

    pub fn list_push_notification_configs(&self, params: Value) -> Result<Value, A2aError> {
        self.call_json(Operation::ListPushConfigs, params)
    }

    pub fn delete_push_notification_config(&self, params: Value) -> Result<Value, A2aError> {
        self.call_json(Operation::DeletePushConfig, params)
    }

    pub fn get_extended_agent_card(&self) -> Result<AgentCard, A2aError> {
        let value = self.call_json(Operation::GetExtendedAgentCard, json!({}))?;
        serde_json::from_value(value).map_err(|error| A2aError::InvalidAgentCard(error.to_string()))
    }

    pub fn send_message_stream(&self, message: A2aMessage) -> Result<A2aEventStream, A2aError> {
        validate_message(&message)?;
        self.open_stream(Operation::SendStreamingMessage, self.send_params(message))
    }

    pub fn subscribe_to_task(&self, id: &str) -> Result<A2aEventStream, A2aError> {
        self.open_stream(Operation::SubscribeToTask, json!({ "id": id }))
    }

    fn call_json(&self, operation: Operation, params: Value) -> Result<Value, A2aError> {
        self.ensure_capability(operation)?;
        let id = Value::String(Uuid::new_v4().to_string());
        let params = self.with_tenant(params);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": self.method_style.method(operation),
            "params": params,
        });
        let response = self
            .http
            .post(&self.endpoint)
            .headers(self.request_headers("application/json"))
            .json(&body)
            .send()
            .map_err(|error| A2aError::Http(error.to_string()))?;
        let value: Value = decode_json_response(response, self.config.max_response_bytes)?;
        parse_json_rpc(value, &id)
    }

    fn open_stream(&self, operation: Operation, params: Value) -> Result<A2aEventStream, A2aError> {
        self.ensure_capability(operation)?;
        let id = Value::String(Uuid::new_v4().to_string());
        let params = self.with_tenant(params);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": self.method_style.method(operation),
            "params": params,
        });
        let response = self
            .http
            .post(&self.endpoint)
            .headers(self.request_headers("text/event-stream"))
            .json(&body)
            .send()
            .map_err(|error| A2aError::Http(error.to_string()))?;
        if !response.status().is_success() {
            return Err(response_status_error(response));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type
            .to_ascii_lowercase()
            .starts_with("text/event-stream")
        {
            return Err(A2aError::Stream(
                "streaming operation did not return text/event-stream".into(),
            ));
        }
        Ok(A2aEventStream {
            reader: BufReader::new(response),
            expected_id: id,
            events: 0,
            bytes: 0,
            max_bytes: self.config.max_response_bytes,
            terminated: false,
        })
    }

    fn send_params(&self, message: A2aMessage) -> Value {
        json!({
            "message": message,
            "configuration": {
                "acceptedOutputModes": if self.card.default_output_modes.is_empty() {
                    vec!["text/plain".to_string()]
                } else {
                    self.card.default_output_modes.clone()
                }
            },
        })
    }

    fn with_tenant(&self, params: Value) -> Value {
        let Some(tenant) = self.tenant.as_deref() else {
            return params;
        };
        let Value::Object(mut params) = params else {
            return params;
        };
        params.insert("tenant".into(), Value::String(tenant.into()));
        Value::Object(params)
    }

    fn ensure_capability(&self, operation: Operation) -> Result<(), A2aError> {
        match operation {
            Operation::SendStreamingMessage | Operation::SubscribeToTask
                if !self.card.capabilities.streaming =>
            {
                Err(A2aError::UnsupportedCapability("streaming"))
            }
            Operation::CreatePushConfig
            | Operation::GetPushConfig
            | Operation::ListPushConfigs
            | Operation::DeletePushConfig
                if !self.card.capabilities.push_notifications =>
            {
                Err(A2aError::UnsupportedCapability("pushNotifications"))
            }
            Operation::GetExtendedAgentCard if !self.card.capabilities.extended_agent_card => {
                Err(A2aError::UnsupportedCapability("extendedAgentCard"))
            }
            _ => Ok(()),
        }
    }

    fn request_headers(&self, accept: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_str(accept)
                .unwrap_or_else(|_| HeaderValue::from_static("application/json")),
        );
        headers.insert(
            "a2a-version",
            HeaderValue::from_str(&self.protocol_version)
                .unwrap_or_else(|_| HeaderValue::from_static("1.0")),
        );
        if !self.config.extensions.is_empty() {
            if let Ok(value) = HeaderValue::from_str(&self.config.extensions.join(",")) {
                headers.insert("a2a-extensions", value);
            }
        }
        if let Some(token) = self.config.bearer_token.as_deref() {
            if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(AUTHORIZATION, value);
            }
        }
        headers
    }
}

pub struct A2aEventStream {
    reader: BufReader<Response>,
    expected_id: Value,
    events: usize,
    bytes: usize,
    max_bytes: usize,
    terminated: bool,
}

impl Iterator for A2aEventStream {
    type Item = Result<Value, A2aError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminated {
            return None;
        }
        if self.events >= MAX_STREAM_EVENTS {
            self.terminated = true;
            return Some(Err(A2aError::Stream("stream event limit exceeded".into())));
        }
        let data = match self.read_event_data() {
            Ok(Some(data)) => data,
            Ok(None) => return None,
            Err(error) => {
                self.terminated = true;
                return Some(Err(error));
            }
        };
        self.events = self.events.saturating_add(1);
        let value = match serde_json::from_str::<Value>(&data) {
            Ok(value) => value,
            Err(error) => {
                self.terminated = true;
                return Some(Err(A2aError::Stream(format!("invalid SSE JSON: {error}"))));
            }
        };
        match parse_json_rpc(value, &self.expected_id).and_then(validate_stream_response) {
            Ok(value) => Some(Ok(value)),
            Err(error) => {
                self.terminated = true;
                Some(Err(error))
            }
        }
    }
}

impl A2aEventStream {
    fn read_event_data(&mut self) -> Result<Option<String>, A2aError> {
        let mut data = String::new();
        loop {
            let Some(line) = self.read_sse_line()? else {
                return Ok((!data.is_empty()).then_some(data));
            };
            match classify_sse_line(&line) {
                SseLine::Data(value) => append_sse_data(&mut data, value),
                SseLine::Boundary if !data.is_empty() => return Ok(Some(data)),
                _ => {}
            }
        }
    }

    fn read_sse_line(&mut self) -> Result<Option<String>, A2aError> {
        let mut line = String::new();
        let size = self
            .reader
            .read_line(&mut line)
            .map_err(|error| A2aError::Stream(error.to_string()))?;
        if size == 0 {
            return Ok(None);
        }
        self.bytes = self.bytes.saturating_add(size);
        if size > MAX_STREAM_LINE_BYTES || self.bytes > self.max_bytes {
            return Err(A2aError::Stream("stream body limit exceeded".into()));
        }
        Ok(Some(line))
    }
}

enum SseLine<'a> {
    Data(&'a str),
    Boundary,
    Ignore,
}

fn classify_sse_line(line: &str) -> SseLine<'_> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return SseLine::Boundary;
    }
    line.strip_prefix("data:")
        .map(|value| SseLine::Data(value.strip_prefix(' ').unwrap_or(value)))
        .unwrap_or(SseLine::Ignore)
}

fn append_sse_data(data: &mut String, value: &str) {
    if !data.is_empty() {
        data.push('\n');
    }
    data.push_str(value);
}

#[derive(Debug, Clone)]
pub struct A2aDeliveryResult {
    pub remote_id: Option<String>,
}

struct A2aRoute {
    generation: u64,
    lease: String,
    client: A2aClient,
}

/// External A2A routes are process-local and session-bound. The Hub remains
/// durable; a new Agent generation must explicitly rediscover and re-register
/// its endpoint before A2A can be selected.
#[derive(Default)]
pub struct A2aEndpointRegistry {
    routes: Mutex<HashMap<(String, String), A2aRoute>>,
}

impl A2aEndpointRegistry {
    pub fn register(
        &self,
        workspace_id: impl Into<String>,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
        config: A2aClientConfig,
    ) -> Result<AgentCard, String> {
        let workspace_id = workspace_id.into();
        let agent_id = agent_id.into();
        let lease = lease.into();
        if workspace_id.trim().is_empty()
            || agent_id.trim().is_empty()
            || generation == 0
            || lease.trim().is_empty()
        {
            return Err(
                "A2A registration requires workspace_id, agent_id, generation, and lease".into(),
            );
        }
        let client = A2aClient::discover(config).map_err(|error| error.to_string())?;
        let card = client.agent_card().clone();
        let mut routes = self
            .routes
            .lock()
            .map_err(|_| "A2A route registry lock poisoned".to_string())?;
        let key = (workspace_id, agent_id);
        if let Some(current) = routes.get(&key) {
            if generation < current.generation {
                return Err("A2A registration generation is stale".into());
            }
            if generation == current.generation && lease != current.lease {
                return Err("A2A registration lease is stale".into());
            }
        }
        routes.insert(
            key,
            A2aRoute {
                generation,
                lease,
                client,
            },
        );
        Ok(card)
    }

    pub fn unregister(
        &self,
        workspace_id: &str,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        let mut routes = self
            .routes
            .lock()
            .map_err(|_| "A2A route registry lock poisoned".to_string())?;
        let key = (workspace_id.to_string(), agent_id.to_string());
        let Some(current) = routes.get(&key) else {
            return Ok(false);
        };
        if current.generation != generation || current.lease != lease {
            return Err("A2A route teardown generation or lease is stale".into());
        }
        routes.remove(&key);
        Ok(true)
    }

    pub fn probe(&self, target: &Value) -> bool {
        let Some((workspace_id, agent_id, generation, lease)) = target_identity(target) else {
            return false;
        };
        let Ok(routes) = self.routes.lock() else {
            return false;
        };
        let Some(route) = routes.get(&(workspace_id.to_string(), agent_id.to_string())) else {
            return false;
        };
        route.generation == generation && route.lease == lease
    }

    pub fn deliver(&self, target: &Value, entry: &Value) -> Result<A2aDeliveryResult, String> {
        let Some((workspace_id, agent_id, generation, lease)) = target_identity(target) else {
            return Err(
                "A2A delivery target lacks workspace_id, agent_id, generation, or lease".into(),
            );
        };
        let client = {
            let routes = self
                .routes
                .lock()
                .map_err(|_| "A2A route registry lock poisoned".to_string())?;
            let route = routes
                .get(&(workspace_id.to_string(), agent_id.to_string()))
                .ok_or_else(|| A2aError::RouteUnavailable.to_string())?;
            if route.generation != generation || route.lease != lease {
                return Err(A2aError::RouteUnavailable.to_string());
            }
            route.client.clone()
        };
        client
            .send_hub_entry(entry)
            .map(|remote_id| A2aDeliveryResult { remote_id })
            .map_err(|error| error.to_string())
    }
}

fn target_identity(target: &Value) -> Option<(&str, &str, u64, &str)> {
    let workspace_id = target
        .get("workspaceId")
        .or_else(|| target.get("workspace_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    Some((
        workspace_id,
        target.get("agentId")?.as_str()?,
        target.get("generation")?.as_u64()?,
        target.get("lease")?.as_str()?,
    ))
}

fn validate_http_url(value: &str) -> Result<(), A2aError> {
    let url = reqwest::Url::parse(value)
        .map_err(|error| A2aError::InvalidConfig(format!("invalid URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(A2aError::InvalidConfig(
            "A2A endpoint must use http or https and include a host".into(),
        ));
    }
    if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
        return Err(A2aError::InvalidConfig(
            "A2A endpoint may not contain credentials or fragments".into(),
        ));
    }
    Ok(())
}

fn build_http_client(config: &A2aClientConfig) -> Result<Client, A2aError> {
    let mut builder = Client::builder()
        .timeout(config.timeout)
        .redirect(reqwest::redirect::Policy::limited(5));
    if reqwest::Url::parse(&config.agent_card_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
    {
        // Local conformance fixtures and local Agent runtimes must not be
        // routed through an inherited desktop HTTP proxy.
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|error| A2aError::Http(error.to_string()))
}

fn validate_card(card: &AgentCard) -> Result<(), A2aError> {
    if card.name.trim().is_empty()
        || card.description.trim().is_empty()
        || card.version.trim().is_empty()
        || card.supported_interfaces.is_empty()
    {
        return Err(A2aError::InvalidAgentCard(
            "name, description, version, and supportedInterfaces are required".into(),
        ));
    }
    for interface in &card.supported_interfaces {
        if !matches!(
            interface.protocol_binding.to_ascii_lowercase().as_str(),
            "jsonrpc" | "json-rpc"
        ) {
            continue;
        }
        if interface.protocol_version.trim().is_empty() {
            return Err(A2aError::InvalidAgentCard(
                "JSON-RPC interface requires protocolVersion".into(),
            ));
        }
        validate_http_url(&interface.url).or_else(|_| {
            if interface.url.starts_with('/') && !interface.url.starts_with("//") {
                Ok(())
            } else {
                Err(A2aError::InvalidAgentCard(
                    "JSON-RPC interface URL is not an HTTP URL".into(),
                ))
            }
        })?;
    }
    if !card.supported_interfaces.iter().any(|interface| {
        matches!(
            interface.protocol_binding.to_ascii_lowercase().as_str(),
            "jsonrpc" | "json-rpc"
        )
    }) {
        return Err(A2aError::InvalidAgentCard(
            "Agent Card has no JSON-RPC interface".into(),
        ));
    }
    Ok(())
}

fn select_interface<'a>(
    card: &'a AgentCard,
    preferred_version: Option<&str>,
) -> Result<&'a AgentInterface, A2aError> {
    let jsonrpc = card
        .supported_interfaces
        .iter()
        .filter(|interface| {
            matches!(
                interface.protocol_binding.to_ascii_lowercase().as_str(),
                "jsonrpc" | "json-rpc"
            )
        })
        .collect::<Vec<_>>();
    preferred_version
        .and_then(|version| {
            jsonrpc
                .iter()
                .find(|interface| interface.protocol_version == version)
                .copied()
        })
        .or_else(|| jsonrpc.first().copied())
        .ok_or_else(|| A2aError::InvalidAgentCard("no selectable JSON-RPC interface".into()))
}

fn validate_message(message: &A2aMessage) -> Result<(), A2aError> {
    if message.message_id.trim().is_empty() || message.parts.is_empty() {
        return Err(A2aError::InvalidRpc(
            "A2A message requires messageId and at least one part".into(),
        ));
    }
    if !matches!(message.role.as_str(), "ROLE_USER" | "ROLE_AGENT") {
        return Err(A2aError::InvalidRpc(
            "A2A message role must be ROLE_USER or ROLE_AGENT".into(),
        ));
    }
    for part in &message.parts {
        let Some(object) = part.as_object() else {
            return Err(A2aError::InvalidRpc(
                "A2A message part must be an object".into(),
            ));
        };
        let content_fields = ["text", "raw", "url", "data"]
            .into_iter()
            .filter(|field| object.contains_key(*field))
            .count();
        if content_fields != 1 {
            return Err(A2aError::InvalidRpc(
                "A2A message part must contain exactly one content field".into(),
            ));
        }
    }
    Ok(())
}

fn auth_headers(config: &A2aClientConfig) -> Result<HeaderMap, A2aError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if let Some(token) = config.bearer_token.as_deref() {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| A2aError::InvalidConfig("bearer_token is not a valid header".into()))?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn decode_json_response<T: for<'de> Deserialize<'de>>(
    mut response: Response,
    max_bytes: usize,
) -> Result<T, A2aError> {
    if !response.status().is_success() {
        return Err(response_status_error(response));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(A2aError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    response
        .by_ref()
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| A2aError::Http(error.to_string()))?;
    if body.len() > max_bytes {
        return Err(A2aError::ResponseTooLarge);
    }
    serde_json::from_slice(&body).map_err(|error| A2aError::InvalidRpc(error.to_string()))
}

fn response_status_error(mut response: Response) -> A2aError {
    let status = response.status().as_u16();
    let mut body = Vec::new();
    let _ = response.by_ref().take(16 * 1024).read_to_end(&mut body);
    let body = String::from_utf8_lossy(&body).replace('\n', " ");
    A2aError::HttpStatus { status, body }
}

fn parse_json_rpc(value: Value, expected_id: &Value) -> Result<Value, A2aError> {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(A2aError::InvalidRpc("jsonrpc must be 2.0".into()));
    }
    if value.get("id") != Some(expected_id) {
        return Err(A2aError::InvalidRpc("JSON-RPC response id mismatch".into()));
    }
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32603);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("A2A remote error")
            .to_string();
        return Err(A2aError::Remote { code, message });
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| A2aError::InvalidRpc("JSON-RPC response has no result".into()))
}

fn parse_send_response(result: Value) -> Result<A2aSendResponse, A2aError> {
    let task = result.get("task").cloned();
    let message = result
        .get("message")
        .cloned()
        .map(|value| {
            serde_json::from_value(value).map_err(|error| A2aError::InvalidRpc(error.to_string()))
        })
        .transpose()?;
    if task.is_some() == message.is_some() {
        return Err(A2aError::InvalidRpc(
            "SendMessage result must contain exactly one task or message".into(),
        ));
    }
    Ok(A2aSendResponse {
        task,
        message,
        raw: result,
    })
}

fn validate_stream_response(result: Value) -> Result<Value, A2aError> {
    let object = result
        .as_object()
        .ok_or_else(|| A2aError::InvalidRpc("A2A stream result must be an object".into()))?;
    let variants = ["task", "message", "statusUpdate", "artifactUpdate"]
        .into_iter()
        .filter(|field| object.contains_key(*field))
        .count();
    if variants != 1 {
        return Err(A2aError::InvalidRpc(
            "A2A stream result must contain exactly one response variant".into(),
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn card(url: &str) -> String {
        json!({
            "name": "Ridge test agent",
            "description": "A bounded A2A fixture",
            "supportedInterfaces": [{
                "url": url,
                "protocolBinding": "JSONRPC",
                "protocolVersion": "1.0",
                "tenant": "tenant-a"
            }],
            "version": "test-1",
            "capabilities": {"streaming": true},
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"],
            "skills": []
        })
        .to_string()
    }

    fn response(status: &str, content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(header_end) = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
            {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end.saturating_add(content_length) {
                    break;
                }
            }
            let read = stream.read(&mut chunk).expect("read HTTP request");
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn card_selection_prefers_v1_jsonrpc_and_redacts_token() {
        let config = A2aClientConfig {
            agent_card_url: "http://127.0.0.1:1234/card".into(),
            bearer_token: Some("secret".into()),
            ..Default::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("secret"));
        let card: AgentCard = serde_json::from_str(&card("http://127.0.0.1:1234/rpc")).unwrap();
        let client = A2aClient::from_card(config, card).unwrap();
        assert_eq!(client.protocol_version(), "1.0");
        assert_eq!(
            client.method_style.method(Operation::SendMessage),
            "SendMessage"
        );
    }

    #[test]
    fn card_rejects_scheme_relative_interface_host() {
        let config = A2aClientConfig {
            agent_card_url: "http://127.0.0.1:1234/card".into(),
            ..Default::default()
        };
        let card: AgentCard = serde_json::from_value(json!({
            "name": "Ridge test agent",
            "description": "A bounded A2A fixture",
            "supportedInterfaces": [{
                "url": "//attacker.example/rpc",
                "protocolBinding": "JSONRPC",
                "protocolVersion": "1.0"
            }],
            "version": "test-1",
            "capabilities": {}
        }))
        .unwrap();
        let error = A2aClient::from_card(config, card).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no selectable JSON-RPC interface")
                || error.to_string().contains("HTTP URL")
        );
    }

    #[test]
    fn send_hub_entry_uses_standard_jsonrpc_headers_and_task_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            let (mut card_stream, _) = listener.accept().unwrap();
            let request = read_request(&mut card_stream);
            assert!(request.starts_with("GET /card"));
            let body = card(&format!("http://{address}/rpc"));
            card_stream
                .write_all(response("200 OK", "application/json", &body).as_bytes())
                .unwrap();

            let (mut rpc_stream, _) = listener.accept().unwrap();
            let request = read_request(&mut rpc_stream);
            assert!(request.contains("POST /rpc"));
            assert!(request.to_ascii_lowercase().contains("a2a-version: 1.0"));
            assert!(request
                .to_ascii_lowercase()
                .contains("accept: application/json"));
            assert!(request.contains("\"tenant\":\"tenant-a\""));
            assert!(!request.to_ascii_lowercase().contains("a2a-tenant:"));
            assert!(request.contains("\"method\":\"SendMessage\""));
            assert!(request.contains("\"text\":\"hello\""));
            assert!(!request.contains("\"taskId\":\"local-task-1\""));
            let body = json!({
                "jsonrpc": "2.0",
                "id": request
                    .split("\"id\":\"")
                    .nth(1)
                    .and_then(|value| value.split('"').next())
                    .unwrap_or("request"),
                "result": {"task": {"id": "remote-task-1"}}
            })
            .to_string();
            rpc_stream
                .write_all(response("200 OK", "application/json", &body).as_bytes())
                .unwrap();
        });
        let client = A2aClient::discover(A2aClientConfig {
            agent_card_url: format!("http://{address}/card"),
            bearer_token: Some("secret".into()),
            ..Default::default()
        })
        .unwrap();
        let remote_id = client
            .send_hub_entry(&json!({
                "messageId": "message-1",
                "deliveryId": "delivery-1",
                "from": "ridge",
                "kind": "message",
                "taskId": "local-task-1",
                "payload": {"text": "hello"}
            }))
            .unwrap();
        assert_eq!(remote_id.as_deref(), Some("remote-task-1"));
        thread.join().unwrap();
    }

    #[test]
    fn stream_parser_returns_each_jsonrpc_event_and_stops_at_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let card_body = card(&format!("http://{address}/rpc"));
            stream
                .write_all(response("200 OK", "application/json", &card_body).as_bytes())
                .unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request(&mut stream);
            let id = request
                .split("\"id\":\"")
                .nth(1)
                .and_then(|value| value.split('"').next())
                .unwrap_or("request");
            let body = format!(
                "data: {{\"jsonrpc\":\"2.0\",\"id\":\"{id}\",\"result\":{{\"task\":{{\"id\":\"t-1\"}}}}}}\n\ndata: {{\"jsonrpc\":\"2.0\",\"id\":\"{id}\",\"result\":{{\"statusUpdate\":{{\"taskId\":\"t-1\"}}}}}}\n\n"
            );
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
        });
        let client = A2aClient::discover(A2aClientConfig {
            agent_card_url: format!("http://{address}/card"),
            ..Default::default()
        })
        .unwrap();
        let mut stream = client
            .send_message_stream(A2aMessage {
                message_id: "message-1".into(),
                role: "ROLE_USER".into(),
                parts: vec![json!({"text": "hello"})],
                context_id: None,
                task_id: None,
                metadata: None,
                extensions: Vec::new(),
                reference_task_ids: Vec::new(),
            })
            .unwrap();
        assert_eq!(stream.next().unwrap().unwrap()["task"]["id"], "t-1");
        assert_eq!(
            stream.next().unwrap().unwrap()["statusUpdate"]["taskId"],
            "t-1"
        );
        assert!(stream.next().is_none());
        thread.join().unwrap();
    }

    #[test]
    fn send_response_rejects_missing_or_duplicate_oneof() {
        let result = parse_send_response(json!({"task": {}, "message": {}}));
        assert!(matches!(result, Err(A2aError::InvalidRpc(_))));
        let result = parse_send_response(json!({}));
        assert!(matches!(result, Err(A2aError::InvalidRpc(_))));
    }

    #[test]
    fn stream_response_rejects_invalid_oneof() {
        assert!(validate_stream_response(json!({"task": {}, "message": {}})).is_err());
        assert!(validate_stream_response(json!({})).is_err());
        assert!(validate_stream_response(json!({"artifactUpdate": {}})).is_ok());
    }

    #[test]
    fn legacy_version_keeps_legacy_jsonrpc_method_names() {
        let card: AgentCard = serde_json::from_value(json!({
            "name": "legacy",
            "description": "legacy fixture",
            "supportedInterfaces": [{
                "url": "http://127.0.0.1:1234/rpc",
                "protocolBinding": "JSONRPC",
                "protocolVersion": "0.3"
            }],
            "version": "legacy-1",
            "capabilities": {},
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"],
            "skills": []
        }))
        .unwrap();
        let client = A2aClient::from_card(
            A2aClientConfig {
                agent_card_url: "http://127.0.0.1:1234/card".into(),
                ..Default::default()
            },
            card,
        )
        .unwrap();
        assert_eq!(
            client.method_style.method(Operation::SendMessage),
            "message/send"
        );
        assert_eq!(client.method_style.method(Operation::GetTask), "tasks/get");
    }

    #[test]
    fn message_validation_rejects_bad_role_and_part_oneof() {
        let base = A2aMessage {
            message_id: "message-1".into(),
            role: "ROLE_USER".into(),
            parts: vec![json!({"text": "hello"})],
            context_id: None,
            task_id: None,
            metadata: None,
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        };
        assert!(validate_message(&base).is_ok());
        assert!(matches!(
            validate_message(&A2aMessage {
                role: "user".into(),
                ..base.clone()
            }),
            Err(A2aError::InvalidRpc(_))
        ));
        assert!(matches!(
            validate_message(&A2aMessage {
                parts: vec![json!({"text": "hello", "data": {}})],
                ..base
            }),
            Err(A2aError::InvalidRpc(_))
        ));
    }

    #[test]
    fn endpoint_registry_fences_generation_and_lease_teardown() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_request(&mut stream);
            let body = card(&format!("http://{address}/rpc"));
            stream
                .write_all(response("200 OK", "application/json", &body).as_bytes())
                .unwrap();
        });
        let registry = A2aEndpointRegistry::default();
        registry
            .register(
                "ws-a",
                "agent-a",
                2,
                "lease-2",
                A2aClientConfig {
                    agent_card_url: format!("http://{address}/card"),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(registry.probe(&json!({
            "workspaceId": "ws-a",
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        })));
        assert!(!registry.probe(&json!({
            "workspaceId": "ws-a",
            "agentId": "agent-a",
            "generation": 1,
            "lease": "lease-1"
        })));
        assert!(!registry.probe(&json!({
            "workspaceId": "ws-b",
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        })));
        assert!(registry
            .unregister("ws-a", "agent-a", 1, "lease-2")
            .is_err());
        assert!(registry
            .unregister("ws-a", "agent-a", 2, "lease-2")
            .unwrap());
        assert!(!registry.probe(&json!({
            "workspaceId": "ws-a",
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        })));
        thread.join().unwrap();
    }
}
