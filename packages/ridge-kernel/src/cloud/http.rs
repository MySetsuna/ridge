//! Native Cloud API proxy through the shared Runtime; never a remote SSRF surface.
use ridge_core::{CoreError,CoreResult};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::HashMap,time::Duration};

const MAX_BODY_BYTES:usize=1024*1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method:String,
    url:String,
    headers:HashMap<String,String>,
    body:Option<String>,
}

fn allowed_url(url:&reqwest::Url)->bool {
    if !matches!(url.scheme(),"http"|"https")||!url.username().is_empty()||url.password().is_some(){return false;}
    let Some(host)=url.host_str() else{return false;};
    host=="9527127.xyz"||host.ends_with(".9527127.xyz")||host=="localhost"||host.ends_with(".localhost")||matches!(host,"127.0.0.1"|"::1"|"[::1]")
}

pub(crate) async fn execute(args:&Value,native:bool)->CoreResult<Value>{
    if !native{return Err(CoreError::CapabilityDenied("cloud_http".into()));}
    let invalid=|message:&str|CoreError::InvalidArgs(message.into());
    let request:Request=serde_json::from_value(args.clone()).map_err(|_|invalid("invalid Cloud HTTP request"))?;
    if request.url.len()>8192||request.body.as_ref().is_some_and(|body|body.len()>MAX_BODY_BYTES){return Err(invalid("Cloud HTTP request exceeds bound"));}
    let url=reqwest::Url::parse(&request.url).map_err(|_|invalid("invalid Cloud HTTP URL"))?;
    if !allowed_url(&url){return Err(invalid("Cloud HTTP target not allowed"));}
    let method=reqwest::Method::from_bytes(request.method.to_ascii_uppercase().as_bytes()).map_err(|_|invalid("invalid Cloud HTTP method"))?;
    let policy=reqwest::redirect::Policy::custom(|attempt|{
        if !allowed_url(attempt.url()){attempt.error("Cloud HTTP redirect target not allowed")}
        else if attempt.previous().len()>=10{attempt.error("Cloud HTTP redirect limit")}
        else{attempt.follow()}
    });
    let builder=reqwest::Client::builder().timeout(Duration::from_secs(20)).redirect(policy);
    let builder=if matches!(url.host_str(),Some("127.0.0.1"|"::1"|"[::1]"))||url.host_str().is_some_and(|host|host=="localhost"||host.ends_with(".localhost")){builder.no_proxy()}else{builder};
    let client=builder.build().map_err(|_|CoreError::internal("Cloud HTTP client unavailable"))?;
    let mut call=client.request(method,url);
    for (name,value) in request.headers {
        if name.eq_ignore_ascii_case("host"){return Err(invalid("Cloud HTTP Host header is not allowed"));}
        call=call.header(name,value);
    }
    if let Some(body)=request.body{call=call.body(body);}
    let mut response=call.send().await.map_err(|_|CoreError::internal("Cloud HTTP transport failed"))?;
    let status=response.status().as_u16();
    if response.content_length().is_some_and(|length|length>MAX_BODY_BYTES as u64){return Err(invalid("Cloud HTTP response exceeds bound"));}
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|CoreError::internal("Cloud HTTP response failed"))?{
        if chunk.len()>MAX_BODY_BYTES.saturating_sub(bytes.len()){return Err(invalid("Cloud HTTP response exceeds bound"));}
        bytes.extend_from_slice(&chunk);
    }
    Ok(json!({"status":status,"body":String::from_utf8_lossy(&bytes)}))
}
