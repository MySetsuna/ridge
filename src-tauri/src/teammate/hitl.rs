//! Domain D2 —— 人类中间审批网关后端 (Human-in-the-Loop).
//!
//! 进程级挂起注册表：当一个高危 (L2 / `RiskLevel::Dangerous`) 动作经过网关时，
//! 后端为它建一个 `oneshot`，emit `teammate://hitl-approval-required` 给前端，
//! 然后**挂起调用方**直到人类裁决（`resolve_hitl_request` 命令回信号）或超时。
//!
//! **默认关闭**（`ENABLED=false`）：[`request_approval`] 在关闭时**立即放行**，
//! 保持现有 send-keys 行为零变化——前端 `HitlApprovalModal` 挂载并真机 e2e 通过后，
//! 再经 `set_hitl_enabled(true)` 开启。注册表是进程全局 (单进程)，无需改 `AppState`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use tauri::Emitter;
use tokio::sync::oneshot;

/// 人类对一个挂起动作的裁决。
pub enum HitlResolution {
    /// 批准：原指令继续执行。
    Approve,
    /// 拒绝：向 agent 返回授权策略阻断错误。
    Reject,
    /// 修改并执行：用新指令替换原指令。
    Modify(String),
}

/// 待裁决动作的事件名（与前端 `HitlApprovalModal` 监听一致）。
pub const HITL_EVENT: &str = "teammate://hitl-approval-required";

/// 人类未裁决时的挂起上限——超时后 fail-closed 视为拒绝（绝不静默放行高危）。
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

static PENDING: LazyLock<Mutex<HashMap<String, oneshot::Sender<HitlResolution>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static ENABLED: AtomicBool = AtomicBool::new(false);
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 网关是否开启（默认关）。
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 开/关网关。开启后高危动作才会被挂起审批。
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// 请求对某动作的人类授权。
///
/// - 网关关闭，或风险经 [`ridge_core::classify_shell_command`] 判为非 L2 → **立即 Approve**。
/// - 否则 emit 待审批事件并挂起，直到 `resolve` 回信号或超时（超时 → Reject）。
pub async fn request_approval(
    handle: &tauri::AppHandle,
    initiator: &str,
    action: &str,
) -> HitlResolution {
    if !is_enabled() {
        return HitlResolution::Approve;
    }
    let assessment = ridge_core::classify_shell_command(action);
    if assessment.level != ridge_core::RiskLevel::Dangerous {
        return HitlResolution::Approve;
    }

    let id = format!("hitl_{}", COUNTER.fetch_add(1, Ordering::Relaxed));
    let (tx, rx) = oneshot::channel();
    if let Ok(mut g) = PENDING.lock() {
        g.insert(id.clone(), tx);
    }

    let _ = handle.emit(
        HITL_EVENT,
        serde_json::json!({
            "id": id,
            "initiator": initiator,
            "action": action,
            "level": "Dangerous",
            "reason": assessment.reason,
        }),
    );

    match tokio::time::timeout(APPROVAL_TIMEOUT, rx).await {
        Ok(Ok(res)) => res,
        // 超时 / 发送端被丢弃（modal 未挂载等）→ fail-closed 拒绝。
        _ => {
            if let Ok(mut g) = PENDING.lock() {
                g.remove(&id);
            }
            HitlResolution::Reject
        }
    }
}

/// 人类裁决回传：按 id 取出挂起项并发回结果。返回是否命中一个挂起项。
pub fn resolve(id: &str, verdict: &str, replacement: Option<String>) -> bool {
    let tx = PENDING.lock().ok().and_then(|mut g| g.remove(id));
    match tx {
        Some(tx) => {
            let res = match verdict {
                "approve" => HitlResolution::Approve,
                "modify" => HitlResolution::Modify(replacement.unwrap_or_default()),
                _ => HitlResolution::Reject,
            };
            let _ = tx.send(res);
            true
        }
        None => false,
    }
}
