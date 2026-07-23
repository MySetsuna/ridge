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

/// 挂起项：裁决信号 + 供远端只读列表的**脱敏**元数据。
/// P2 阶段 1（iteration 8）：`list_pending` 投影仅暴露此处字段——
/// **绝不存/不投影 `action` 命令全文**（可含密钥；全文只随桌面事件走本机）。
struct PendingEntry {
    tx: oneshot::Sender<HitlResolution>,
    initiator: String,
    reason: String,
    created_at_ms: u64,
    /// P2 阶段 2：一次性裁决票据（随挂起项生成，uuid v4 不可猜；仅经 E2EE 信道
    /// 随 `list_pending` 投影下发给已授权 controller，恒时比对 + 取出即毁）。
    nonce: String,
    /// M1 切片二：decisions 落盘的归属工作区（None = 测试/无归属，不落盘）。
    wid: Option<uuid::Uuid>,
}

/// M1 切片二 —— 裁决审计条目（**绝不含命令全文**；落 workspace-memory `decisions` 节）。
fn record_decision(entry: &PendingEntry, source: &str, verdict: &str, outcome: &str) {
    let Some(wid) = entry.wid else { return };
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    super::memory::append_decision_global(
        wid,
        serde_json::json!({
            "ts": ts,
            "source": source,
            "initiator": entry.initiator,
            "verdict": verdict,
            "riskLevel": "Dangerous",
            "reasonSummary": entry.reason,
            "outcome": outcome,
        }),
    );
}

static PENDING: LazyLock<Mutex<HashMap<String, PendingEntry>>> =
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
    wid: uuid::Uuid,
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
    insert_pending(id.clone(), Some(wid), initiator, &assessment.reason, tx);

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
                if let Some(entry) = g.remove(&id) {
                    record_decision(&entry, "timeout", "reject", "fail-closed");
                }
            }
            HitlResolution::Reject
        }
    }
}

fn insert_pending(
    id: String,
    wid: Option<uuid::Uuid>,
    initiator: &str,
    reason: &str,
    tx: oneshot::Sender<HitlResolution>,
) {
    let created_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut g) = PENDING.lock() {
        g.insert(
            id,
            PendingEntry {
                tx,
                initiator: initiator.to_string(),
                reason: reason.to_string(),
                created_at_ms,
                nonce: uuid::Uuid::new_v4().simple().to_string(),
                wid,
            },
        );
    }
}

/// P2 —— 待裁决动作的**脱敏**只读快照（远端可见面）。
/// 仅 `{id, initiator, level, reason, createdAt, resolutionNonce}`；`level` 恒为
/// `Dangerous`（非 L2 动作根本不挂起）。按 createdAt 升序，顺序稳定。
pub fn list_pending() -> Vec<serde_json::Value> {
    let mut items: Vec<(u64, serde_json::Value)> = match PENDING.lock() {
        Ok(g) => g
            .iter()
            .map(|(id, e)| {
                (
                    e.created_at_ms,
                    serde_json::json!({
                        "id": id,
                        "initiator": e.initiator,
                        "level": "Dangerous",
                        "reason": e.reason,
                        "createdAt": e.created_at_ms,
                        "resolutionNonce": e.nonce,
                    }),
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    items.sort_by_key(|(t, _)| *t);
    items.into_iter().map(|(_, v)| v).collect()
}

/// 恒时比较（防计时侧信道摸 nonce；长度不同立即 false——长度非秘密）。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// P2 阶段 2 —— 远端裁决结局（可判定字符串，随 RPC 回包）。
pub const OUTCOME_CONSUMED: &str = "consumed";
pub const OUTCOME_ALREADY_RESOLVED: &str = "already-resolved";
pub const OUTCOME_NONCE_MISMATCH: &str = "nonce-mismatch";
pub const OUTCOME_BAD_VERDICT: &str = "bad-verdict";

/// P2 阶段 2 —— 远端裁决：nonce 恒时比对，一致才取出（同锁内查+取 = 单次消费原子）。
/// verdict 仅 `approve`/`reject`——**modify 永不开放**（远程任意命令执行面）。
/// 超时/已裁决后到达 → `already-resolved` 无副作用；错 nonce → 条目存活、拒并告警。
pub fn resolve_remote(id: &str, nonce: &str, verdict: &str) -> &'static str {
    let res = match verdict {
        "approve" => HitlResolution::Approve,
        "reject" => HitlResolution::Reject,
        _ => return OUTCOME_BAD_VERDICT,
    };
    let Ok(mut g) = PENDING.lock() else { return OUTCOME_ALREADY_RESOLVED };
    let Some(entry) = g.get(id) else { return OUTCOME_ALREADY_RESOLVED };
    if !constant_time_eq(&entry.nonce, nonce) {
        tracing::warn!(target: "ridge::hitl", %id, "远端裁决 nonce 不匹配（拒）");
        // M1 切片二：败者尝试亦入审计（条目存活，仅记录）。
        record_decision(entry, "remote", verdict, OUTCOME_NONCE_MISMATCH);
        return OUTCOME_NONCE_MISMATCH;
    }
    let entry = g.remove(id).expect("checked above under same lock");
    drop(g);
    record_decision(&entry, "remote", verdict, OUTCOME_CONSUMED);
    let _ = entry.tx.send(res);
    OUTCOME_CONSUMED
}

/// 人类裁决回传：按 id 取出挂起项并发回结果。返回是否命中一个挂起项。
pub fn resolve(id: &str, verdict: &str, replacement: Option<String>) -> bool {
    let entry = PENDING.lock().ok().and_then(|mut g| g.remove(id));
    match entry {
        Some(entry) => {
            let res = match verdict {
                "approve" => HitlResolution::Approve,
                "modify" => HitlResolution::Modify(replacement.unwrap_or_default()),
                _ => HitlResolution::Reject,
            };
            // M1 切片二：桌面裁决入审计（modify 只记动词，不记替换文本）。
            record_decision(&entry, "desktop", verdict, "consumed");
            let _ = entry.tx.send(res);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P2 脱敏门禁：`list_pending` 投影绝不含 `action` 命令全文，字段恰为
    /// {id, initiator, level, reason, createdAt, resolutionNonce}；resolve 后即出列。
    /// （PENDING 为进程全局，测试用独有 id 前缀过滤，避免并行串扰。）
    #[test]
    fn list_pending_projection_is_sanitized_and_cleared_on_resolve() {
        let id = "hitl_test_sanitized_0".to_string();
        let (tx, mut rx) = oneshot::channel();
        insert_pending(id.clone(), None, "claude-a", "递归删除目录", tx);

        let mine: Vec<_> = list_pending()
            .into_iter()
            .filter(|v| v["id"] == id.as_str())
            .collect();
        assert_eq!(mine.len(), 1);
        let item = mine[0].as_object().expect("pending item object");
        for key in item.keys() {
            assert!(
                ["id", "initiator", "level", "reason", "createdAt", "resolutionNonce"]
                    .contains(&key.as_str()),
                "unexpected pending field `{key}`"
            );
        }
        assert!(item.get("action").is_none(), "projection must never carry action");
        assert_eq!(item["level"], "Dangerous");
        assert_eq!(item["initiator"], "claude-a");
        assert!(item["resolutionNonce"].as_str().is_some_and(|n| n.len() >= 32));

        assert!(resolve(&id, "reject", None));
        assert!(matches!(rx.try_recv(), Ok(HitlResolution::Reject)));
        assert!(list_pending().iter().all(|v| v["id"] != id.as_str()));
    }

    /// P2 阶段 2 裁决通道门禁：错 nonce 拒且条目存活；对 nonce 恰消费一次、
    /// 二次 already-resolved；modify/未知 verdict 拒（远端 modify 永不开放）。
    #[test]
    fn resolve_remote_single_consume_nonce_and_verdict_gates() {
        let id = "hitl_test_remote_0".to_string();
        let (tx, mut rx) = oneshot::channel();
        insert_pending(id.clone(), None, "claude-b", "强制推送", tx);
        let nonce = list_pending()
            .into_iter()
            .find(|v| v["id"] == id.as_str())
            .and_then(|v| v["resolutionNonce"].as_str().map(String::from))
            .expect("nonce in projection");

        assert_eq!(resolve_remote(&id, &nonce, "modify"), OUTCOME_BAD_VERDICT);
        assert_eq!(resolve_remote(&id, "wrong-nonce", "approve"), OUTCOME_NONCE_MISMATCH);
        assert!(
            list_pending().iter().any(|v| v["id"] == id.as_str()),
            "错 nonce 后条目必须存活"
        );

        assert_eq!(resolve_remote(&id, &nonce, "approve"), OUTCOME_CONSUMED);
        assert!(matches!(rx.try_recv(), Ok(HitlResolution::Approve)));
        assert_eq!(resolve_remote(&id, &nonce, "approve"), OUTCOME_ALREADY_RESOLVED);
        assert!(list_pending().iter().all(|v| v["id"] != id.as_str()));
    }

    /// M1 切片二：远端消费 → decision 落 workspace-memory `decisions` 节，
    /// 含归因 initiator、来源与结局，**绝不含命令全文**。
    #[test]
    fn remote_consume_records_sanitized_decision() {
        let dir = std::env::temp_dir().join(format!("ridge-hitl-decisions-{}", uuid::Uuid::new_v4()));
        super::super::memory::init_dir(dir);
        let dir = super::super::memory::dir().expect("dir injected").to_path_buf();
        let wid = uuid::Uuid::new_v4();

        let id = "hitl_test_decision_0".to_string();
        let (tx, _rx) = oneshot::channel();
        insert_pending(id.clone(), Some(wid), "claude-c", "递归删除目录", tx);
        let nonce = list_pending()
            .into_iter()
            .find(|v| v["id"] == id.as_str())
            .and_then(|v| v["resolutionNonce"].as_str().map(String::from))
            .unwrap();
        assert_eq!(resolve_remote(&id, &nonce, "reject"), OUTCOME_CONSUMED);

        let doc = super::super::memory::read(&dir, wid).expect("decision doc");
        let list = doc["decisions"].as_array().expect("decisions array");
        let d = list.last().unwrap().as_object().unwrap();
        assert_eq!(d["source"], "remote");
        assert_eq!(d["verdict"], "reject");
        assert_eq!(d["outcome"], OUTCOME_CONSUMED);
        assert_eq!(d["initiator"], "claude-c");
        assert!(d.get("action").is_none() && d.get("command").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
