//! Shell adapter retaining public identity and private transport authorization, never a device key.
use crate::{client::request_json, registry::KernelEndpoint};
use ridge_core::device_identity::DeviceBindingSigner;
use serde_json::{json, Value};
use uuid::Uuid;

pub struct DeviceBindingClient { endpoint: KernelEndpoint, connection: Uuid, public: [u8;32] }
impl DeviceBindingClient {
    pub fn connect(endpoint: KernelEndpoint) -> Result<Self, String> {
        let mut client = Self { endpoint, connection: Uuid::new_v4(), public: [0;32] };
        let value = client.request("get_device_identity_pub", json!({}))?;
        client.public = serde_json::from_value::<Vec<u8>>(value).map_err(|error|error.to_string())?.try_into().map_err(|_|"invalid device public key length")?;
        Ok(client)
    }
    fn request(&self, method: &str, args: Value) -> Result<Value, String> {
        let reply = request_json(&self.endpoint, "POST", "/v1/domain/commands", Some(&json!({
            "method":method,"args":args,"native":true,"roots":[],"connectionId":self.connection
        })))?;
        if reply["ok"] != true { return Err(reply["error"]["message"].as_str().unwrap_or("Runtime identity request failed").into()); }
        Ok(reply["value"].clone())
    }
}
impl DeviceBindingSigner for DeviceBindingClient {
    fn public_bytes(&self) -> [u8;32] { self.public }
    fn sign_binding(&self, context: &[u8]) -> Result<[u8;64], String> {
        serde_json::from_value::<Vec<u8>>(self.request("sign_device_identity", json!({"context":context}))?)
            .map_err(|error|error.to_string())?.try_into().map_err(|_|"invalid device signature length".into())
    }
}
