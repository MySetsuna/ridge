# [待审核·下一里程] 完整 WS 出站 PTY 客户端

**状态**：不现在实现 · 仅备忘 + 供审核  
**差距 ID**：H1 完整段 / 历史 `V-H1-LIVE` 完整  
**何时进入迭代**：2026-07-24 **iteration 16** 评估后将「完整出站 PTY」纳入产品线，但只交付**最小闭环**；完整客户端刻意记为下一里程。此后 17–19 / 双报告均 **未当 open**。  
**相关设计**：`docs/specs/2026-06-30-multi-host-foreign-terminal-hosts-design.md` §2
**已实现（最小）**：TCP 探测 · attach 门控 · `remote_ref` / `live_sink` · foreign 登记 · `fanout_live_output`→parser

---

## 1. 产品目标（审核用一句话）

本机桌面 Ridge 作为 **多 Host 控制器**：在「主机 / Hosts」里连接远端 ridge/rdg，把远端 pane **像本地终端一样**看到 live 输出、键入与 resize 回灌；关闭本地视图 = **detach 不杀远端会话**。

**非目标（本里程不做）**：通用 VNC；改 cloud 协议 SSOT；前端直接挂多条 WebRTC（仍由本机 Rust 后端做出站客户端）。

---

## 2. 实现架构

### 2.1 角色与边界

```
┌──────────── 本机桌面 Ridge ────────────┐         ┌──── 远端 Host ────┐
│  Svelte UI (Hosts + Workspace panes)  │         │  ridge / rdg      │
│           │ invoke / events           │         │  /ws + PTY        │
│           ▼                           │  出站    │                   │
│  src-tauri hosts/*  (出站客户端)  ────────WS/mux──►│  subscribe-pane   │
│    · HostConnection 表                │◄─0x10───│  pane raw bytes   │
│    · ForeignRef 映射本地 pane         │  raw     │  write_to_pty     │
│    · 复用 delta / write 路由          │          └───────────────────┘
└───────────────────────────────────────┘
```

- **前端永不直连多 host 传输**：始终 `invoke` + 既有 pane delta/事件；与无头 native 领养一致。  
- **Rust 后端**持有出站连接与会话真相；`PtyHandle` 对 foreign 只是视图句柄 + 路由（`remote_ref`），无本地子进程。

### 2.2 分层（建议落点）

| 层 | 职责 | 复用 / 新建 |
|---|---|---|
| **Transport 出站** | LAN：`ws://host:port/ws` + 既有 auth；可选后续 cloud WebRTC host 角色 | 优先复用 `ridge-cli` **lan_session / mux / rpc** 语义与帧格式；新代码宜 `src-tauri/src/hosts/outbound_*.rs` 或下沉 `ridge-core` |
| **Session 映射** | `(host_id, remote_pane_id) ↔ local pane_id` | 扩展现有 `hosts` 注册表 / `ForeignAttachment` |
| **输出路径** | 远端 `0x10` pane raw → 本地 `feed` parser + 既有 `pty-delta` / 前端 TerminalManager | 已有 `fanout_live_output` 骨架；需接真 transport 而非仅测注入 |
| **输入路径** | 前端 `write_to_pty` / resize → 若 foreign → 出站 RPC `write_to_pty` / `resize_pane` | 已有 `live_sink` / `remote_ref` 分支；需真 send |
| **生命周期** | connect/disconnect host；subscribe/unsubscribe pane；detach 本地 leaf | Hosts 侧 terminate ≠ 关视图 |
| **能力** | `$/hello` capability；灰置不支持操作 | 对齐 `REMOTE_ALLOWLIST` / capabilityContract |

### 2.3 连接与数据流（LAN 优先切片）

1. **Host 登记**：用户「连接主机」→ 后端建 `HostConnection`（connecting→connected/error）；状态推 `hosts` store。  
2. **列会话**：`list panes/sessions`（或等价 RPC）填 Hosts 树。  
3. **接入**：用户「接入」→ `attach_host_session`（已有门控 Connected）→ 本地 split 新 leaf + `remote_ref` + 出站 **`subscribe-pane`**。  
4. **Live 输出**：出站读循环收 raw → `fanout`/parser → 前端与本地 pane 同渲染路径。  
5. **Live 输入**：焦点 foreign pane 键入 → `write_pty` 路由到 `live_sink` → 出站写。  
6. **Resize**：前端 claim/resize → 出站 `resize_pane`（与 cloud controller 语义对齐）。  
7. **Detach**：关本地 leaf → unsubscribe + 清 foreign 映射，**不断 host、不杀远端 PTY**。  
8. **断线**：host disconnect → 会话标 error；UI 可重连后 re-subscribe（策略可复用 `reconnect_policy` 退避）。

### 2.4 与「已有最小闭环」的差

| 能力 | 最小（已交） | 完整（本 note） |
|---|---|---|
| 可达性 | TCP probe | 全协议握手 + 会话列表 |
| attach | 门控 + 元数据 + sink 挂钩 | + 真 subscribe |
| 输出 | 测试/注入 fanout | 持续 transport 回灌 |
| 输入 | sink 可测 | 真 RPC 写远端 |
| 多 host 并存 | 注册表骨架 | 每 host 独立出站任务 + 隔离 |

### 2.5 风险与不变量

- 不引入第二协议 SSOT；帧/方法名跟 ridge-cli / REMOTE_ALLOWLIST。  
- foreign 关闭 = detach only。  
- 多 controller 看同一远端 pane 时：本机仍是**单 attachment 视图**（设计单活领养）；远端 host 自己的 fan-out 不由本里程重写。  
- 安全：凭据不进日志；token/TOTP 与现 remote 一致。

---

## 3. 前端交互方式（审核用）

### 3.1 入口：侧边栏「主机 / Hosts」

沿用已有 Hosts tab（非新 IDE）：

```
┌─ 主机 ──────────────── [+ 连接] [刷新] ─┐
│ ▼ 本机（无头）              N 会话       │
│     build-watch   [接入] [⋯]            │
│ ▼ 办公室-PC · connected     M 会话       │
│     main · ~/proj   [接入] [跳转] [⋯]   │
│     agent-2         [接入]              │
│ ▼ staging · error                       │
│     (重试连接)                          │
└─────────────────────────────────────────┘
```

- **+ 连接主机**：对话框（地址/端口/鉴权类型：code|token|…）→ 后台 connecting 态。  
- **接入**：把该远端会话接入**当前工作区**布局（默认 split 策略与现 attach 一致；可保留 dock 区域选择）。  
- **跳转**：若已 attachment → 切到对应 workspace/pane。  
- **⋯**：断开 host / 从列表移除 /（可选）在 Hosts 内终止远端会话——与「关视图」分离。

### 3.2 工作区内 foreign pane

- **标识**：leaf 带来源徽标（远端 host 名 / rdg / 无头），与设计 `PaneOrigin` 一致，避免与本地 PTY 混淆。  
- **渲染**：与本地相同 TerminalManager/WASM 路径；用户无感「另一套终端」。  
- **输入焦点**：点 foreign pane 即写入该 `remote_ref`；不误写本地 shell。  
- **关闭 tab/leaf**：确认文案建议「断开视图，远端会话继续」；不弹「结束进程」除非用户在 Hosts 选终止。  
- **断线**：pane 内状态条「主机断开 · 重连中/失败」；重连成功后自动恢复订阅与可选 scrollback 策略（首屏 tail + 滚顶 before，对齐 cloud history-pull 思想，可二期）。

### 3.3 与现有能力的关系（勿做两套 UI）

| 场景 | 交互 |
|---|---|
| 本机无头 | Hosts「本机」→ 接入（已有 summon 路径） |
| 手机/浏览器 Remote | **仍是**「整个 app 连一台 host」；本里程是**桌面多 host 出站**，不替代 Remote 控制台 |
| Agent Center | foreign pane 可出现在 roster 若远端跑 agent；本里程不强制改 HITL |

### 3.4 建议验收（实现时再闸，非现在）

1. 连 LAN ridge host → Hosts 列出 pane → 接入 → 见 live 输出。  
2. 键入 echo → 远端可见；resize 不撕裂。  
3. 关本地 leaf → 远端会话仍在；Hosts 可再次接入。  
4. 拔网 → UI error；恢复后 re-subscribe 不双订泄漏。  
5. 确定性：出站 mock transport 单测（subscribe/write/fanout 映射），不靠真机 flaky e2e。

---

## 4. 建议实现切片（供排期，非本合同）

1. **T1** 出站 LAN 客户端 + hello/list（只读）  
2. **T2** subscribe + raw→本地 parser/delta  
3. **T3** write/resize 回灌  
4. **T4** 断线/重订/detach 生命周期  
5. **T5**（可选）cloud WebRTC 出站对称  

---

## 5. 审核要点（请批）

1. 是否同意 **后端出站、前端只 invoke**（否决前端多 WebRTC）？  
2. 首切片是否 **仅 LAN WS**，cloud出站二期？  
3. 关 foreign pane 文案/是否二次确认？  
4. scrollback：首屏只 tail，还是接入时全量（不推荐）？  

**请批注意见：本 note 仅规划；未改代码；未开 open 愿景。**
