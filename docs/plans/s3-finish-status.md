# S3 收尾状态审计（2026-06-07）

> 目的：回答"S3 还差什么、能不能开启"。**核实当前 `develop` 代码**（非旧日志）后逐项定状态，区分**可自主完成并本地验证**与**外部阻塞**（跨仓库 / 需 cloud 真机 e2e）。
> 背景：前一轮 2026-06-04 多 agent 大重构（Wave 1–3）已把 **S0/S1/S2/S3-主体/S4/S5/S8** 落入 develop 并随后 push 到 origin。本审计是其上的收尾盘点。

---

## 结论先行

**S3 的前置（S0/S1/S2）已就绪，S3 主体（JSON-RPC-native server + transport 层 + D9 握手 + 错误码透传 + $/cancel + 背压）已建并在 develop。** 所以 S3 不是"未开始"，而是"主体已成、收尾若干"。收尾项里：

- ✅ **可自主完成的已完成**（本轮）：cli 协议收敛核实 = 已达成；**S7 conformance cloud-WebRTC arm 已补齐**（commit `419908d`）。
- ⚠️ **大且耦合**（自主可做但非干净隔离）：D10 渲染快照。
- ❌ **外部阻塞**（跨仓库 / 需 cloud 真机 e2e 才能诊断或验证）：get_directory_children 经云返回空、终端经云 D-GM-11、E2EE 身份绑定 D-GM-10、S6 部署。

---

## 逐项

### A · cli↔controller 协议收敛 —— ✅ 已达成（核实）
`packages/ridge-cli/src/rpc.rs` 已是 **JSON-RPC 2.0 native + D9 `$/hello` 能力协商**（reduced-capability terminal host：`pane`/`fs`/`search`，刻意是桌面 `HOST_CAPABILITIES` 子集，controller 灰掉 IDE 面板优雅降级）。cloud e2e 在 2026-06-04 已 LIVE 验证（host 经云 + WebRTC + E2EE，controller 浏览器渲染出 host 真实文件树）。旧记忆里"Wave4 cli↔controller 协议收敛根因"已由该轮解决。**无剩余动作。**

### S7 · conformance cloud-WebRTC arm —— ✅ 本轮完成（`419908d`）
`conformance.test.ts` 重构为 `describe.each` 跑在 **LAN-WS + cloud-WebRTC** 两腿（决策 D6 两腿不漂移）：共享 `hostReply` + 两 harness（`LanWsAdapter`+fake `RemoteConnection` / `CloudWebrtcAdapter`+fake `RemoteConnectionProvider`，经 `cloudMux` 收发）。覆盖 D9 握手/能力交集/$/bye、JSON-RPC 往返、结构化错误码透传(D-GM-2)、read-only/cap-denied 码、$/cancel、pane 字节、事件通知；legacy fallback 保留 LAN-only。**vitest 32 passed。**

### D10 · attach 渲染快照 —— ⚠️ 大且耦合（前驱已工作，全量是子项目）
- **现状（工作前驱，已 ship）**：`server.rs` subscribe-pane 回放最多 64 KiB raw scrollback，足够 kernel 重绘——LAN/cloud 终端重连可用。
- **全量缺口**：渲染屏幕快照（alt-screen/cursor 精确），scaffold 见 `server.rs:2992`（`PaneSnapshotFrame` 类型 + 序列化测试已在）。
- **为何非干净隔离**：①触及 PaneParser（`engine/parser.rs` 已有 `full_reframe_with_scrollback()` 可作快照源，但 raw-byte 客户端是另一条渲染路）；②`PaneSnapshotFrame` 携带**锁定尺寸**，而锁定尺寸属 D11 Wave B 桌面 `WorkspaceGraph` 采用——**已按定稿延后到 S3 旁**；③客户端需新增"从快照重绘"。三层耦合 + 软依赖延后项 → 半成品有回归风险。
- **建议**：与 D11 Wave B 桌面采用同期做（locked-size 一并落），不要孤立强推。前驱已满足当前用户用法。

### B · get_directory_children 懒加载经云返回空 —— ❌ 需 cloud e2e 诊断
- **本地代码路径核实为正确**：前端 `fileExplorer.ts:481-486` 正确传 `{path, offset, limit}`；`ridge_core::dispatch` 的 arm（`dispatch.rs:195`）正确 `opt_usize` 读 offset/limit → `fs::tree::page_children`。cli 侧 `rpc.rs` 的 `DirectoryChildren` 忽略 offset/limit（用 `usize::MAX` 返回全部，非空）——非本 bug。
- **结论**：bug 只在 cloud 传输上显现（疑似 host-context 路径或 `DirectoryPage` 跨云序列化/分页特化），**本地无法复现 / 不能盲改正确代码**。需 cloud 真机 e2e 抓现场（host 实际收到的 path + 返回的 DirectoryPage）。**外部阻塞（验证）。**

### B · 终端经云 D-GM-11 —— ❌ 需 Tauri-event 桥 + cloud e2e
PTY 字节经云到 controller 需 src-tauri 侧把 Tauri-event PTY 流接入 cloud host onFrame 编码器。可写但**验证需 cloud WebRTC e2e**（真机）。**外部阻塞（验证）。**

### B · E2EE 公钥↔身份绑定 D-GM-10 —— ❌ 跨仓库
密钥认证核实跨 `C:\code\ridge-cloud`。**外部阻塞（跨仓库）。**

### C · S6 部署 —— ❌ 跨仓库 + dokku 历史分叉
ridge-cloud 的 app.* 子域 serve desktop SPA 代码已提交（`fff01da`，**在 ridge-cloud 仓库，不在本 develop**），但 `git push dokku` 受阻于本地 clone 与已部署 dokku/main 历史分叉（E 组 force-push），需与 E 组对齐后再推。**外部阻塞（跨仓库 + 运维协调）。**

---

## 能不能"开启 S3"

能——且**主体已在**。本轮把唯一干净可自主完成的收尾项（S7 cloud arm）做掉了。余下要么是**需 cloud 真机 e2e / 跨仓库**（dir-children、终端经云、E2EE、S6），要么是**与 D11 Wave B 桌面采用耦合的大子项**（D10 全量）——这些的"完成"不在单仓库本机自主能力内，需：① cloud 真机 e2e 环境（诊断 dir-children / 验证终端经云）；② 跨仓库协调（ridge-cloud 的 E2EE + S6 dokku 推送）；③ 排期 D11 Wave B 桌面采用以解 D10 的 locked-size 软依赖。

**下一步建议优先级**：S6 部署对齐（解锁公网入口，价值最高但需 E 组）→ cloud e2e 抓 dir-children 现场（小修）→ D10 随 D11 Wave B → 终端经云 / E2EE 随 cloud 迁 Rust。
