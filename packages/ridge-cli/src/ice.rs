//! ICE servers 拉取（契约 §5.2）。
//!
//! 客户端**必须**调用 `GET /api/v1/ice-servers`(Bearer) 取 iceServers，不要硬编码。
//! v1 返回仅含公共 STUN；后续可加 TURN 而不改客户端。拉取失败时回退到契约写明
//! 的公共 STUN，保证可用性。

use anyhow::Result;
use serde::Deserialize;

use crate::config;
use crate::rtc::FALLBACK_STUN;

#[derive(Debug, Clone, Deserialize)]
struct IceServerEntry {
    #[serde(default)]
    urls: UrlsField,
}

/// `urls` 既可能是单个字符串也可能是数组（W3C RTCIceServer 约定）。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum UrlsField {
    One(String),
    Many(Vec<String>),
}

impl Default for UrlsField {
    fn default() -> Self {
        UrlsField::Many(Vec::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct IceServersData {
    #[serde(rename = "iceServers", default)]
    ice_servers: Vec<IceServerEntry>,
}

/// 取 iceServers 的 url 列表。失败 / 为空时回退到公共 STUN。
pub async fn fetch_ice_urls(client: &reqwest::Client, device_token: &str) -> Vec<String> {
    match try_fetch(client, device_token).await {
        Ok(urls) if !urls.is_empty() => urls,
        Ok(_) => {
            tracing::warn!(target: "ridge_cli::ice", "ice-servers empty; using fallback STUN");
            vec![FALLBACK_STUN.to_string()]
        }
        Err(e) => {
            tracing::warn!(target: "ridge_cli::ice", error = %e, "ice-servers fetch failed; using fallback STUN");
            vec![FALLBACK_STUN.to_string()]
        }
    }
}

async fn try_fetch(client: &reqwest::Client, device_token: &str) -> Result<Vec<String>> {
    let url = format!("{}/ice-servers", config::api_base());
    let body = client
        .get(url)
        .bearer_auth(device_token)
        .send()
        .await?
        .text()
        .await?;
    let data: IceServersData = crate::envelope::parse_envelope(&body)?;
    let mut urls = Vec::new();
    for entry in data.ice_servers {
        match entry.urls {
            UrlsField::One(u) => urls.push(u),
            UrlsField::Many(many) => urls.extend(many),
        }
    }
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_array_urls() {
        let body = r#"{"ok":true,"data":{"iceServers":[
            {"urls":"stun:a:1"},
            {"urls":["stun:b:2","turn:c:3"]}
        ]}}"#;
        let data: IceServersData = crate::envelope::parse_envelope(body).unwrap();
        let mut urls = Vec::new();
        for e in data.ice_servers {
            match e.urls {
                UrlsField::One(u) => urls.push(u),
                UrlsField::Many(m) => urls.extend(m),
            }
        }
        assert_eq!(urls, vec!["stun:a:1", "stun:b:2", "turn:c:3"]);
    }
}
