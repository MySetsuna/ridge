//! 领域只读出口（REQ-RIDGE-KERNEL-DOMAIN-01 首切片）。
//! FS 列表经 ridge-core；Agent 配置表为内置 profiles（与桌面 catalog 对齐的最小集）。

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .or_else(|| headers.get("x-ridge-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
}

/// 内置 agent 表（与 `agent_catalog::builtin_profiles` 同构最小集）。
pub fn builtin_agent_profiles() -> Value {
    json!([
        {"id":"claude","processNames":["claude","claude-code"],"executable":"claude"},
        {"id":"codex","processNames":["codex"],"executable":"codex"},
        {"id":"grok","processNames":["grok"],"executable":"grok"},
        {"id":"gemini","processNames":["gemini"],"executable":"gemini"},
        {"id":"cursor-agent","processNames":["cursor-agent"],"executable":"cursor-agent"}
    ])
}

#[derive(Serialize)]
pub struct DomainMeta {
    pub ok: bool,
    pub capabilities: &'static [&'static str],
}

pub async fn domain_meta(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DomainMeta>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(DomainMeta {
        ok: true,
        capabilities: &["fs.list", "agents.profiles", "git.status", "mcp"],
    }))
}

pub async fn domain_agents(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "profiles": builtin_agent_profiles(),
    })))
}

#[derive(Deserialize)]
pub struct FsListQuery {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn domain_fs_list(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FsListQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match ridge_core::fs::commands::get_directory_children(&q.path, q.offset, q.limit) {
        Ok(page) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "path": q.path,
            "page": page,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "error": e.to_string(),
        }))),
    }
}

#[derive(Deserialize)]
pub struct GitQuery {
    pub path: String,
}

pub async fn domain_git_status(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let is_repo = ridge_core::commands::git::is_git_repo(q.path.clone());
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": q.path,
        })));
    }
    match ridge_core::commands::git::get_scm_status_sync(q.path.clone()) {
        Ok(status) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": q.path,
            "status": status,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "error": e,
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_nonempty() {
        let v = builtin_agent_profiles();
        assert!(v.as_array().unwrap().len() >= 3);
    }
}
