//! Wind teammate HTTP 客户端与 `list-panes?json=1` 布局。

use serde::Deserialize;

pub(crate) fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("client")
}

pub(crate) fn auth_headers(token: &str) -> reqwest::header::HeaderMap {
    let mut m = reqwest::header::HeaderMap::new();
    m.insert(
        "X-Wind-Token",
        reqwest::header::HeaderValue::from_str(token).expect("token header"),
    );
    m
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListPanesJsonBody {
    pub(crate) active_index: usize,
    pub(crate) pane_count: usize,
    pub(crate) panes: Vec<PaneRowJson>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct PaneRowJson {
    pub(crate) index: usize,
    #[serde(default)]
    pub(crate) pane_id: String,
    #[serde(default)]
    pub(crate) uuid: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
}

pub(crate) fn fetch_list_windows_plain(url: &str, token: &str) -> Result<String, ()> {
    let u = format!("{}/api/v1/list-windows", url.trim_end_matches('/'));
    let res = client()
        .get(&u)
        .headers(auth_headers(token))
        .send()
        .map_err(|_| ())?;
    if !res.status().is_success() {
        return Err(());
    }
    res.text()
        .map_err(|_| ())
        .map(|t| t.trim().to_string())
}

pub(crate) fn fetch_pane_layout(url: &str, token: &str) -> Result<ListPanesJsonBody, ()> {
    let u = format!("{}/api/v1/list-panes?json=1", url.trim_end_matches('/'));
    let res = client()
        .get(&u)
        .headers(auth_headers(token))
        .send()
        .map_err(|_| ())?;
    if !res.status().is_success() {
        return Err(());
    }
    let text = res.text().map_err(|_| ())?;
    serde_json::from_str(&text).map_err(|_| ())
}