//! Transitional registrations forward Cloud controls; no socket or peer lives here.
use serde_json::{json,Value};
#[tauri::command]
pub fn cloud_host_online(device_token:String,device_name:String,username:String,base_domain:String,plaintext:Option<bool>)->Result<(),String> {
    super::runtime::request("cloud_host_online",json!({"deviceToken":device_token,"deviceName":device_name,"username":username,"baseDomain":base_domain,"plaintext":plaintext.unwrap_or(false)}),true,super::runtime::window_connection("cloud-host"))
}
#[tauri::command]
pub fn cloud_host_offline()->Result<(),String> {super::runtime::request("cloud_host_offline",json!({}),true,super::runtime::window_connection("cloud-host"))}
#[tauri::command]
pub fn cloud_host_kick(cid:String)->Result<(),String> {super::runtime::request("cloud_host_kick",json!({"cid":cid}),true,super::runtime::window_connection("cloud-host"))}
#[tauri::command]
pub fn cloud_host_blacklist(cid:String)->Result<(),String> {super::runtime::request("cloud_host_blacklist",json!({"cid":cid}),true,super::runtime::window_connection("cloud-host"))}
#[tauri::command]
pub fn get_cloud_host_state()->Result<Value,String> {
    let Some(endpoint)=ridge_kernel::client::running_endpoint() else {return Ok(json!({"state":"offline","error":"","sessions":[]}));};
    let reply=ridge_kernel::client::request_json(&endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"get_cloud_host_state","args":{},"native":true,"roots":[],"connectionId":super::runtime::window_connection("cloud-host")})))?;
    if reply["ok"]!=true{return Err("Cloud state query failed".into());}Ok(reply["value"].clone())
}
