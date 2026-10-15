//! E2EE 公钥↔身份绑定的判定核心（D-GM-10 / B3，方案 X）。
//!
//! 与桌面 `src/lib/remote/cloud/keyBinding.ts` 的 `decideKeyBinding` / `constantTimeEqual`
//! **逐字段对齐**：host/controller 在 E2EE 握手收到对端临时 X25519 公钥后，与对端经
//! **已认证信令**旁路（A 层 `e2ee-pubkey`，**仅 eph_pub、不带 sig** —— 方案 X，relay 零
//! 密码学材料）转发回来的公钥做恒定时间比对。不一致 = relay 层主动 MITM（给两端各发攻击
//! 者自己的公钥）→ 拒绝会话。
//!
//! 启用门 = **信令公钥到达性**（而非 D9 `$/hello` 能力位）：`$/hello` 在 E2EE 握手完成后
//! 才发，太晚无法据它在握手时拒绝。改用更稳健的"信令公钥到达性"——DataChannel 网络中间人
//! 无法篡改另一条已认证 TLS 信令上转发的公钥，故"收到信令公钥"即强制比对；"宽限期内未收
//! 到"暂缓；"宽限期过仍未收到"判对端为不发该信令的旧端 → 回落 relay-trust（向后兼容，
//! 旧端不回归）。DataChannel MITM 无法借此回落逃逸——它无法阻止信令公钥经独立 TLS 到达。
//!
//! 纯模块（无 I/O、无网络），可独立单测；session.rs 据其判定结果决定 accept / 断开 / 等待。

use crate::e2ee::PUB_KEY_LEN;

/// 恒定时间比较两个字节序列是否相等（防计时侧信道）。长度不同立即返回 false
/// （长度本身非秘密）；等长时逐字节累积差异，不提前退出。
pub fn constant_time_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// 三态判定（与 keyBinding.ts `KeyBindingDecision` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBindingDecision {
    /// 校验通过（或宽限期回落 relay-trust）：可放行业务。
    Accept,
    /// 检测到 MITM：握手公钥 ≠ 信令旁路公钥 → 必须断开。
    Reject,
    /// 信令旁路公钥尚未到达且宽限期未过 → 暂缓决定（既不放行也不拒绝）。
    Wait,
}

/// 一个连接**已决**的 B3 绑定模式（诊断/日志可读）。对应桌面 `keyBinding.ts`
/// `KeyBindingMode` 的两个**终态**；其 `pending`（未决）态在 cli 侧由
/// `binding_decided: bool` 跟踪，无需独立枚举值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBindingMode {
    /// 收到信令旁路公钥并比对一致（严格绑定生效）。
    Enforced,
    /// 宽限期内未收到信令公钥 → 回落 v1（对端疑为旧端）。
    RelayTrust,
}

/// B3 在线判定（与 keyBinding.ts::decideKeyBinding 逐字段对齐）。
///
/// - `handshake_pub`：E2EE DataChannel 握手帧解出的对端公钥（`0x01||pub32`）。
/// - `signaling_pub`：对端经已认证信令转发回来的公钥；尚未到达为 `None`。
/// - `grace_expired`：宽限期是否已过（仍未收到 `signaling_pub` 时用于回落判定）。
pub fn decide_key_binding(
    handshake_pub: &[u8],
    signaling_pub: Option<&[u8]>,
    grace_expired: bool,
) -> KeyBindingDecision {
    if let Some(sig) = signaling_pub {
        if sig.len() != PUB_KEY_LEN || handshake_pub.len() != PUB_KEY_LEN {
            return KeyBindingDecision::Reject; // 防御性：任一公钥长度非法 → 拒绝
        }
        return if constant_time_equal(handshake_pub, sig) {
            KeyBindingDecision::Accept
        } else {
            KeyBindingDecision::Reject // 握手公钥 ≠ 信令旁路公钥 → MITM
        };
    }
    // 尚无信令公钥：宽限期内等待；过期则回落 relay-trust（对端疑为旧端，不发信令公钥）。
    if grace_expired {
        KeyBindingDecision::Accept
    } else {
        KeyBindingDecision::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> [u8; PUB_KEY_LEN] {
        [b; PUB_KEY_LEN]
    }

    #[test]
    fn constant_time_equal_basic() {
        assert!(constant_time_equal(&k(7), &k(7)));
        assert!(!constant_time_equal(&k(7), &k(8)));
        assert!(!constant_time_equal(&[1, 2, 3], &[1, 2])); // 长度不同立即 false
    }

    #[test]
    fn accept_when_signaling_matches_handshake() {
        // 信令公钥 == 握手公钥 → enforced accept。
        assert_eq!(
            decide_key_binding(&k(1), Some(&k(1)), false),
            KeyBindingDecision::Accept
        );
    }

    #[test]
    fn reject_when_signaling_differs() {
        // 信令公钥 ≠ 握手公钥 → MITM → reject。
        assert_eq!(
            decide_key_binding(&k(1), Some(&k(2)), false),
            KeyBindingDecision::Reject
        );
    }

    #[test]
    fn reject_on_bad_length_signaling() {
        assert_eq!(
            decide_key_binding(&k(1), Some(&[1, 2, 3]), false),
            KeyBindingDecision::Reject
        );
    }

    #[test]
    fn wait_before_grace_when_no_signaling() {
        // 宽限期未过、信令公钥未到 → 暂缓。
        assert_eq!(
            decide_key_binding(&k(1), None, false),
            KeyBindingDecision::Wait
        );
    }

    #[test]
    fn accept_after_grace_without_signaling_relay_trust() {
        // 宽限期过仍无信令公钥 → 回落 relay-trust（accept，兼容旧端）。
        assert_eq!(
            decide_key_binding(&k(1), None, true),
            KeyBindingDecision::Accept
        );
    }
}
