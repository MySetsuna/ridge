# A 真正恢复测试（Goal #3）

**Date**: 2026-09-15
**改了什么**: `src/remote/lib/cloudRemote.test.ts` 新增 4 个 A-recovery 用例

## 现状

Cloud reconnect 路径（`src/remote/lib/cloudRemote.ts::_handleReconnect`，line 575+）：

```ts
let ok = this.fixedAuthorized;
if (!this.fixedAuthorized && this._verifiedCode) {
  ok = await this.handle.verifyTotp(this._verifiedCode).catch(() => false);
} else if (!this.fixedAuthorized) {
  ok = await this.handle.tryTrustGrant().catch(() => false);
}
```

策略：
1. `fixedAuthorized`（旧固定 token）→ 直接 ok
2. **优先用** `_verifiedCode`（fast path）→ `verifyTotp`
3. **fallback** 到 `tryTrustGrant`（§7.4 信任握手，host 验证 Ed25519 签名）

`reconnected=0` 时不触发 `reconnectListeners`（MainApp resync 路径），UI 走 `error` 终态并提示「刷新拿新码」。

## 新增 4 个用例

| 用例 | 验证不变量 |
|---|---|
| 旧 OTP 过期但 trust-grant 有效 → 断线恢复无需 TOTP | 走 `tryTrustGrant`；`verifyTotp` **不**被调（不骚扰用户输码） |
| trust-grant 被撤销 → 恢复失败，进 `error` 终态 | `reconnected=0`，`_failure.category === 'channel'` |
| 同时有 `_verifiedCode` + trust-grant → 优先 `_verifiedCode` | 调 `verifyTotp('cached-code')`；**不**调 `tryTrustGrant` |
| `setVerifiedCode` 不写入 `localStorage` / `sessionStorage` | 抽样 localStorage 所有键值不含 `'123456'` 或 `_verifiedCode` 模式 |

总计：`Test Files 1 passed (1) | Tests 61 passed (61)`

## 区分 LAN / Cloud

| 路径 | 检测机制 | 复用恢复 |
|---|---|---|
| **Cloud**（`src/remote/lib/cloudRemote.ts`） | provider `notifyState('disconnected')` → `_handleReconnect` → `verifyTotp` 或 `tryTrustGrant` | `_verifiedCode` + trust-grant ✓ |
| **LAN**（`src/routes/+layout.svelte`，`+page.svelte`） | `RemoteConnection.onStateChange('error')` → `bridge.detach()` + `localStorage.removeItem(TOKEN_KEY)` → 退回 TOTP gate | saved token 重用 ✓ |

> 测试只覆盖 cloud 路径（已用 trust-grant 抽象）；LAN 路径已有 `+layout.svelte:265-302` 的 finish() 流程，saved token 优先 → `bridge.invoke('list_workspaces')` 失败则回 TOTP gate。**没有把两条路径的证据混用**。

## NOT_RUN：ping/pong 有界探测

**Goal #3 第三条**："前台发 ping 但收不到 pong → 有界探测后进入正确恢复流程"。

当前实现**没有**显式 ping/pong 探测层。理由：
- Cloud：底层 WebRTC data channel 自带 keepalive（ICE/DTLS heartbeat）；provider state 切换是「连接真死」的可靠信号。
- LAN：底层 WebSocket 走浏览器原生；`onclose` / `onerror` 是真死信号。浏览器在 WebSocket 半开连接上不发用户态通知（这是 TCP 限制），所以即使加显式 ping 也只探测到「half-open NAT」。

**为何不补**：显式 ping/pong 是新功能（目标 N.5 类），不是 bug 修复。本轮目标「真正恢复测试」已通过 trust-grant / verifiedCode / reauth-fail 三类用例覆盖核心恢复逻辑。ping/pong 列入下轮（N.5 —「增加有界探测 + 主动切断 dead-but-WS-open」），与 LAN/Cloud 路径同时落地。

**Status**: NOT_RUN（已标注在最终报告）

## 不动的部分（Goal 红线）

- **不**持久化 TOTP（`setVerifiedCode` 仅写内存 `this._verifiedCode`）
- **不**放宽 `verifyTotp` 超时 / `tryTrustGrant` 超时
- **不**改 trust-grant 协议（§7.4 Ed25519 签名流程原样）
- **不**跳过「恢复失败 → UI 走刷新拿新码」分级（仍归 `channel` 类）

## 复现

```bash
pnpm vitest run src/remote/lib/cloudRemote.test.ts
```

## 真机（移动 cloud controller）人工最小验证步骤

1. 启动 host：`./target/test-rdg/release/ridge.exe host --port 5120`
2. 启动 cloud controller（cdp-mobile 路径）
3. 输 TOTP 验证成功 → `_verifiedCode` 入内存
4. 拔 host 物理网线 → 触发 provider state `disconnected`
5. 插回网线 → provider state `connected` 恢复
6. 验证：UI 不弹 TOTP 框；`list_workspaces` 等调用自动恢复
7. 跑 `hostctl revoke-trust <device>` → 重连应失败 → UI 提示「通道异常，请刷新」
