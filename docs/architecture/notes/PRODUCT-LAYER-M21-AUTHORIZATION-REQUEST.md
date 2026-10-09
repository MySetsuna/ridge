---
id: G1-G3-STATUS-M21
title: "M2.1 §6 G1/G3 Authorization Gate — Precise Gap Analysis"
status: BLOCKED
date: 2026-10-04
---

# M2.1 §6 — G1 / G3 授权门状态

按 M2.1 §6 要求产出此文档。本文件不作新实现；只精确列出 G1、G3 的具体缺口 / 涉及文件 / 是否需契约变化 / 当前阻塞原因。

## G1 — packages/ridge-acp 正式入 workspace

### 缺口
`C:\code\wind\Cargo.toml` 的 workspace members 当前不含 `packages/ridge-acp`，导致：
- `cargo check/test/clippy --workspace` 不覆盖 ridge-acp（仅 M2 独立 build）
- src-tauri/ 不能 `path = "../../packages/ridge-acp"` 依赖（workspace 才能解析）
- 任何调用方必须脱离主 workspace 独立构建 ridge-acp（违反 NS-1 验收）

### 涉及文件
- `C:\code\wind\Cargo.toml` — workspace `members` 数组增 `"packages/ridge-acp"`
- `C:\code\wind\Cargo.lock` — 锁文件重新生成（cargo 自动）
- 不涉契约：`AgentSessionEvent` / `AgentSessionSnapshot` 已是公开类型，无需变更

### 是否需契约变化
否。Spec `L2-RIDGE-PRODUCT-M1` 的 code_targets 已含 `Cargo.toml`/`Cargo.lock`/`packages/ridge-acp/**`，CHG-053 已批准（2026-10-03T07:50:48Z）。契约层已就位。

### 当前阻塞（POLICY_E_UNAUTHORIZED_DIFF）
`stc lock CHG-053` 失败：

```
POLICY_E_UNAUTHORIZED_DIFF: .github/workflows/release.yml, .stcignore,
docs/operations/remote-cloud-release.md, package.json,
packages/remote/src/shared/cloud/cloudHostBridge.ts,
packages/remote/src/shared/cloud/controllerIdentity.ts,
packages/ridge-cli/Cargo.toml, packages/ridge-cli/src/host.rs,
packages/ridge-cli/src/kernel_host_impl.rs, packages/ridge-cli/src/main.rs,
packages/ridge-cli/src/protocol.rs, packages/ridge-cli/src/session.rs,
packages/ridge-cli/src/tui/lan_host.rs,
packages/ridge-cli/tests/kernel_lifecycle_e2e.rs,
packages/ridge-core/Cargo.toml, packages/ridge-core/src/device_identity.rs,
packages/ridge-core/src/grant_store.rs, packages/ridge-kernel/src/lib.rs,
packages/ridge-kernel/src/pty.rs, packages/ridge-kernel/src/server.rs,
packages/ridge-remote/Cargo.toml, packages/ridge-remote/src/host.rs,
packages/ridge-remote/src/lib.rs, packages/ridge-remote/src/server_app.rs,
scripts/publish-remote-cloud.mjs, src/app.html,
src/lib/components/FileEditor.svelte, src/lib/i18n/messages/main.ts,
src/lib/i18n/messages/mobile.ts, src/remote/App.svelte,
src/remote/AuthScreen.svelte, src/remote/CloudAuthScreen.svelte,
src/remote/lib/TerminalCanvas.svelte, src/remote/MainApp.svelte,
src/routes/+layout.svelte, src/service-worker.test.ts, src/service-worker.ts,
src-tauri/Cargo.toml, src-tauri/src/remote_bridge.rs,
src-tauri/tauri.conf.json, vite.remote.config.js,
docs/architecture/notes/PRODUCT-LAYER-M2-ACCEPTANCE.md,
packages/ridge-cli/src/session_control_tests.rs,
packages/ridge-cli/src/session_control.rs,
packages/ridge-kernel/src/metrics.rs,
packages/ridge-remote/src/lan_trust_tests.rs,
packages/ridge-remote/src/lan_trust.rs,
src/lib/agent-chat/AgentChatPanel.svelte, src/lib/agent-chat/agentEvents.test.ts,
src/lib/agent-chat/agentEvents.ts, src/lib/monaco/workers.ts,
tools/soak-harness/Cargo.lock, tools/soak-harness/Cargo.toml,
tools/soak-harness/src/main.rs ... [~60 paths]
```

根因：仓库积累 60+ pre-existing diff（非 M2.1 工作），含：
- `packages/ridge-kernel/**` 修改（v0.1.92 / trust-grant 0.1.89/0.1.90/0.1.91 残留）
- `packages/ridge-cli/**` 修改（trust-grant CLI / LAN host 残留）
- `packages/ridge-remote/**` 修改（LAN trust 残留）
- `src-tauri/**` 修改（remote_bridge 残留）
- `src/remote/**` / `src/routes/**` 修改（前面 round 残留）
- **未追踪**：`packages/ridge-acp/`（M2.1 工作）、`src/lib/agent-chat/`（M2 工作，未 spec 覆盖）、`tools/soak-harness/`、`docs/architecture/notes/PRODUCT-LAYER-M2-ACCEPTANCE.md`、`packages/ridge-kernel/src/metrics.rs`

CHG-053 的授权范围不含上述路径；任何其中之一未授权 → `stc lock` 整体拒绝。

### 解锁所需授权（须用户决定）
A. **为 pre-existing diffs 各建独立 CHG**（最少 4-5 个 CHG：trust-grant 系列、ridge-cli 重构、src-tauri/remote_bridge、src/remote、tools/soak-harness）。工作量大。
B. **回退 pre-existing diffs**（git checkout -- <path>）。会丢失 0.1.89-0.1.92 已发布特性（trust-grant CLI、LAN trust、remote bridge 等）。高风险。
C. **绕过 governance 直接编辑 Cargo.toml**（违反 §6 + 违反 SpecTree spec_hash 校验）。不可取。
D. **维持现状**：M2.1 §1 NS-1 仅满足"`cargo build -p ridge-acp` + 单 crate 测试"，不满足"workspace 覆盖"。降级记录于 docs。

---

## G3 — Desktop Tauri Bridge

### 缺口
- `src-tauri/Cargo.toml` 无 `ridge-acp` 依赖
- `src-tauri/src/bridge/` 不存在（无 commands 模块）
- `src-tauri/src/main.rs` / `lib.rs` 无 `AgentSession` 命令注册
- Tauri `event::emit` channel 未建立（前端 → `AgentSession.subscribe()`）

### 涉及文件
- `src-tauri/Cargo.toml` — `ridge-acp = { path = "../../packages/ridge-acp" }` 增 dep
- `src-tauri/src/bridge/acp_bridge.rs`（新）— Tauri command 集
- `src-tauri/src/main.rs` — `tauri::generate_handler!` 注册 bridge commands
- `src/lib/agent-chat/AgentChatPanel.svelte`（已存，M2 留）— `onPrompt`/`onApprove`/`onDeny` 接线

### 是否需契约变化
否。`AgentSession` 已是公开 API（`prompt` / `approve_tool` / `cancel` / `subscribe` / `shutdown`）。Tauri command 只是薄包装。

### 当前阻塞
同 G1：`src-tauri/**` 在 `L2-REMOTE-MOBILE-SEMANTIC-V2` (CHG-052) 锁定范围。CHG-053 不覆盖 `src-tauri/**`。锁必须先放。

---

## M2.1 §6 决策点（请求用户授权）

按 M2.1 §6："如果需要扩大 frozen Runtime / ACP contract，再停下请求授权" — G1 + G3 均需扩大 frozen Runtime 路径，**本 round 自停止于此**，请求以下之一授权：

1. **路径 A**：批准 CHG-054+ 覆盖 pre-existing diffs；本人按顺序 apply→approve→lock CHG-053，再实施 G1+G3。
2. **路径 B**：用户回退非 M2.1 pre-existing diffs（保护 M2.1 范围）；本人实施 G1+G3。
3. **路径 C**：M2.1 §1 NS-1 降级为"`cargo build/test/clippy -p ridge-acp` 独立绿"（不满足 workspace 覆盖）；NS-2/3/4/5 同等降级。
4. **路径 D**：用户给 pre-existing diffs 一并授权，本人批量处理。

`未经授权不 push/tag/release/deploy` 硬约束遵守中。

## 不可自执行的清单（明示）
- 修改 `Cargo.toml` workspace members（G1）
- 修改 `src-tauri/Cargo.toml` + `src-tauri/src/**`（G3）
- 修改 `src/routes/+layout.svelte`（NS-3 wiring 接线点；当前 M2 残 diff 阻塞）
- 解决 `stc lock CHG-053` 的 60+ unauthorized diff 阻塞

## 可自执行但被 NS-2 卡住的清单（明示）
- `src/lib/agent-chat/AgentChatPanel.svelte` 已存；NS-3 仅缺 Tauri 命令接线
- `packages/ridge-acp/src/session.rs` 已就绪；可对接任意上层

## 建议路径
路径 D：用户一次性授权 pre-existing diffs（CHG-054 覆盖 trust-grant 系列 + ridge-cli 重构 + src-tauri/remote_bridge + src/remote + tools/soak-harness + M2/M2.1 工作）；本人顺序 apply→lock CHG-053→实施 G1+G3→NS-3→NS-4→NS-5→M2.1 整体收口。
