//! 统一响应信封（契约 §2）。
//!
//! 成功：`{ "ok": true, "data": <T> }`
//! 失败：`{ "ok": false, "error": { "code": "<CODE>", "message": "<人类可读>" } }`

use anyhow::{bail, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;

/// §2 错误体。
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

/// §2 信封。`ok=true` 带 `data`，`ok=false` 带 `error`。
///
/// 显式 `bound` 覆盖 serde derive 默认推导的 `T: Default`（`#[serde(default)]`
/// 在泛型字段上会引入该约束，但 `Option::<T>::default()` 实际不需要它）。
#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct Envelope<T> {
    pub ok: bool,
    #[serde(default = "default_none")]
    pub data: Option<T>,
    #[serde(default)]
    pub error: Option<ApiError>,
}

/// `Option::<T>::default()` 不需要 `T: Default`，避免 serde 推导出多余约束。
fn default_none<T>() -> Option<T> {
    None
}

impl<T> Envelope<T> {
    /// 把信封折叠成 `Result<T>`。失败时把 `code: message` 作为 anyhow 错误。
    pub fn into_result(self) -> Result<T> {
        if self.ok {
            match self.data {
                Some(d) => Ok(d),
                None => bail!("API ok=true but data missing"),
            }
        } else {
            match self.error {
                Some(e) => bail!("API error [{}]: {}", e.code, e.message),
                None => bail!("API ok=false but error missing"),
            }
        }
    }
}

/// 把响应体字符串按信封解析并折叠为 `Result<T>`。
pub fn parse_envelope<T: DeserializeOwned>(body: &str) -> Result<T> {
    let env: Envelope<T> = serde_json::from_str(body)?;
    env.into_result()
}
