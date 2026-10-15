//! Authentication proxy without a seed, identity cache or persistent grant writer.
use crate::{client,registry::KernelEndpoint};
use ridge_remote::auth::RemoteAuth;
use serde_json::json;
use uuid::Uuid;

pub fn remote_auth(endpoint:KernelEndpoint)->RemoteAuth {
    let connection=Uuid::new_v4();
    RemoteAuth::delegated(move |method,args| {
        let reply=client::request_json(&endpoint,"POST","/v1/domain/commands",Some(&json!({
            "method":method,"args":args,"native":true,"roots":[],"connectionId":connection
        })))?;
        if reply["ok"]!=true {return Err(reply["error"]["message"].as_str().unwrap_or("Runtime authentication request failed").into());}
        Ok(reply["value"].clone())
    })
}
