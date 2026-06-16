//! ICE servers 拉取（契约 §5.2）。
//!
//! 客户端**必须**调用 `GET /api/v1/ice-servers`(Bearer) 取 iceServers，不要硬编码。
//! v1 返回仅含公共 STUN；后续可加 TURN 而不改客户端。拉取失败时回退到契约写明
//! 的公共 STUN，保证可用性。

use anyhow::Result;
use serde::Deserialize;

use crate::config;
use crate::rtc::FALLBACK_STUN;

#[derive(Debug, Clone)]
pub struct IceServerConfig {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct IceServerEntry {
    #[serde(default)]
    urls: UrlsField,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    credential: Option<String>,
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

/// 取 iceServers 列表（含 STUN/TURN 地址和时效凭证）。
/// 失败 / 为空时回退到公共 STUN（无凭证）。
pub async fn fetch_ice_servers(client: &reqwest::Client, device_token: &str) -> Vec<IceServerConfig> {
    match try_fetch(client, device_token).await {
        Ok(servers) if !servers.is_empty() => servers,
        Ok(_) => {
            tracing::warn!(target: "ridge_cli::ice", "ice-servers empty; using fallback STUN");
            vec![IceServerConfig {
                urls: vec![FALLBACK_STUN.to_string()],
                username: None,
                credential: None,
            }]
        }
        Err(e) => {
            tracing::warn!(target: "ridge_cli::ice", error = %e, "ice-servers fetch failed; using fallback STUN");
            vec![IceServerConfig {
                urls: vec![FALLBACK_STUN.to_string()],
                username: None,
                credential: None,
            }]
        }
    }
}

async fn try_fetch(client: &reqwest::Client, device_token: &str) -> Result<Vec<IceServerConfig>> {
    let url = format!("{}/ice-servers", config::api_base());
    let body = client
        .get(url)
        .bearer_auth(device_token)
        .send()
        .await?
        .text()
        .await?;
    let data: IceServersData = crate::envelope::parse_envelope(&body)?;
    let mut servers = Vec::new();
    for entry in data.ice_servers {
        let urls = match entry.urls {
            UrlsField::One(u) => vec![u],
            UrlsField::Many(many) => many,
        };
        if urls.is_empty() {
            continue;
        }
        servers.push(IceServerConfig {
            urls,
            username: entry.username,
            credential: entry.credential,
        });
    }
    Ok(servers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_and_array_urls() {
        let body = r#"{"ok":true,"data":{"iceServers":[
            {"urls":"stun:a:1"},
            {"urls":["stun:b:2","turn:c:3"],"username":"1700000000","credential":"abcd1234"}
        ]}}"#;
        let data: IceServersData = crate::envelope::parse_envelope(body).unwrap();
        assert_eq!(data.ice_servers.len(), 2);

        let stun = &data.ice_servers[0];
        assert!(matches!(stun.urls, UrlsField::One(_)));
        assert!(stun.username.is_none());
        assert!(stun.credential.is_none());

        let turn = &data.ice_servers[1];
        if let UrlsField::Many(ref urls) = turn.urls {
            assert_eq!(urls[0], "stun:b:2");
            assert_eq!(urls[1], "turn:c:3");
        } else {
            panic!("expected array urls");
        }
        assert_eq!(turn.username.as_deref(), Some("1700000000"));
        assert_eq!(turn.credential.as_deref(), Some("abcd1234"));

        // verify full conversion to IceServerConfig
        let mut servers = Vec::new();
        for entry in data.ice_servers {
            let urls = match entry.urls {
                UrlsField::One(u) => vec![u],
                UrlsField::Many(many) => many,
            };
            servers.push(IceServerConfig {
                urls,
                username: entry.username,
                credential: entry.credential,
            });
        }
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].urls, vec!["stun:a:1"]);
        assert_eq!(servers[0].username, None);
        assert_eq!(servers[1].urls, vec!["stun:b:2", "turn:c:3"]);
        assert_eq!(servers[1].username.as_deref(), Some("1700000000"));
        assert_eq!(servers[1].credential.as_deref(), Some("abcd1234"));
    }
}
