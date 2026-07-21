# Remote / Rdg 产品-可用性-工程 路线图（战略稿）

> 日期：2026-07-11 · 状态：战略框架，待逐阶段拆 spec 迭代
> 对齐：`docs/ROADMAP.md`（产品定位=AI 开发控制平面）
> 覆盖：主线 A（Remote 产品/UX）· 主线 B（Rdg 可用性）· 主线 C（工程基座解耦）

## 0. 一句话定位（把三条线钉到北极星）

`docs/ROADMAP.md` 说产品是 **AI 开发控制平面**，护城河=「Agent 运行时可见 + 人类可接管 + 上下文不丢 + 多 Agent 编排」。据此重新归位：

> **Remote / Rdg 不是「远程桌面」，是「AI 控制平面的随处接入层」**——让你在**离开工位时**（手机、别人的浏览器、SSH 进来的终端）依然能**看住 agent 在干什么、随时按下暂停/接管、给组长派活**。

这句话是本路线图所有取舍的准绳：**凡是强化「移动端也能可见+可接管 agent」的，优先；凡是把 Remote 做成通用 VNC 的，砍。** 与 ROADMAP「绝对不做：万能 IDE / UI 打磨地狱」一致。

## 1. 现状成熟度矩阵（诚实版）

| 能力域 | 成熟度 | 半成品缺口（gap 扫描已确认，任务号） |
|---|---|---|
| **Remote-LAN**（桌面 app 局域网远控） | 🟢 可用 | 选区/IME 闪烁默认路径未修(#22)、resize scrollback 不 reflow(#25) |
| **Remote-Cloud**（公网 WebRTC 远控） | 🟡 能连但不稳/不纵深 | 零信任 fail-closed 未翻闸(#7)、authState 未收敛(#8)、**多控制方无多路复用**(#9)、终端流不压缩弱网卡(#10)、E2EE 绑定桌面 host 侧待补(#11)、D10 精确重连快照仅 scaffold(#20) |
| **Rdg**（终端原生 CLI/TUI 控制端+无头 host） | 🟡 骨架成、能力残 | **无头 host 仍单控制方**(#9)、**连远端主机只登记不流 PTY**(#12)、LAN 懒加载 scrollback 缺(#21)、公网 offerer 路径未落 |
| **工程基座** | 🟡 分层已起但内核不全 | **git/workspace/pane/terminal 命令未收口 ridge-core**(#19)→ rdg 无头 host 对这些返 MethodNotFound；ridge-remote 未发 crate、ridge-cloud ua 手工镜像(#4) |
| **产品层（可见+接管）** | 🔴 移动端几乎空白 | Remote/Rdg 目前只把「终端像素」搬过去，**没有把 agent 花名册/状态/暂停/接管/派活搬到移动端**——控制平面的核心价值在移动端缺席 |

**最刺眼的一条**：Remote 把「终端」远程化了，却没把「**控制平面**」远程化。手机连上看到的是一块终端画布，而不是「3 个 agent 在跑、A 卡在测试、点一下暂停 A」。这正是下一步产品设计的最大机会，也是本路线图主线 A 的核心。

## 2. 三条主线

### 主线 A —— Remote = 控制平面的移动层（产品/UX，最高杠杆）

把桌面已有的「智能体指挥部」（`AgentCenterPanel` 花名册/状态/编组/HITL）**投影到 Remote 双端**，让移动/浏览器控制端不止是终端，而是**随身的 agent 控制台**。

- **A1 移动端 Agent 控制台（旗舰）**：Remote SPA 加一屏「Agents」——列出当前工作区 agent（名/能力档/Idle·Working/Leader）、一键切到某 agent 的 pane、**暂停/恢复**（对齐 ROADMAP Phase 3 Human Control）、**给组长派活**（复用 `ridge_join_group`/dispatch）。数据源已有（`get_teammate_topology` + teammate 事件桥），缺的是移动 UI 投影。
- **A2 接管流（Human Takeover on mobile）**：agent 触发 HITL/高危动作时，**推**到移动端（通知 + 审查卡片 Approve/Reject），人在地铁上也能放行/拦截。依赖 Remote 的服务端推送（与 #15 notifications/progress 同源）。
- **A3 可见性最小集**：移动端 pane 顶部显示 agent 状态灯 + 最近一条「组任务」，让「看一眼就知道 agent 在干嘛」成立。
- **A4 弱网可用**：#10 流压缩 + #20 精确重连快照——弱网/切后台回来不黑屏、不丢历史。这是「移动端」成立的地基。

> A 线把 ROADMAP 的 Phase 2（Agent Awareness）/Phase 3（Human Control）**延伸到移动端**，是 Remote 从「远程桌面」升级为「控制平面接入层」的关键。

### 主线 B —— Rdg = 终端原生的控制平面入口（可用性/能力对齐）

Rdg 是「不开图形界面，纯终端里也能接入控制平面」的入口（SSH 进服务器也能用）。当前是骨架。

- **B1 无头 host 能力补全**：#19（git/workspace/pane/terminal 收口 ridge-core）让 rdg 无头 host 不再对半数命令返 MethodNotFound——**这条同时是工程主线 C 的产出**，一石二鸟。
- **B2 多控制方**：#9 daemon 按 cid 多路复用，多人/多端同时接一个 host。
- **B3 连远端主机**：#12 connect_host live PTY 传输——rdg 真正能「连上另一台机器的 host 并流终端」，而非只登记。
- **B4 懒加载与体验**：#21 LAN scrollback seq 游标 + rdg-interactive 遗留（pager/交互）。

### 主线 C —— 工程基座：ridge-core 内核化（高内聚·低耦合的地基）

**核心工程赌注**：把 `src-tauri`（桌面胖壳）里的领域逻辑**下沉为 `ridge-core` 纯内核**，各端（桌面/rdg/云）退化为**薄 I/O 壳**。

```
                 ┌──────────────── ridge-core（领域内核，纯、平台无关、可测）────────────────┐
                 │  teammate/mcp/risk/topology(已在) + git/workspace/pane/terminal(待收口 #19) │
                 │  + fs/search/shell/settings/theme(已迁) —— 命令=纯函数，dispatch 表统一      │
                 └───────────────▲───────────────▲───────────────▲──────────────────────────┘
     薄壳：桌面 src-tauri(Tauri IPC)  │   rdg ridge-cli(TUI/headless) │   ridge-cloud(HTTP/relay，跨仓)
                 └ 只做：I/O 绑定、平台 API、事件桥。**零领域逻辑**。
     横切共享：ridge-remote(传输/UA/serve，双端+云共用) · ridge-term(渲染) · ridge-signaling(线协议 SSOT)
```

- **C1 命令内核化（#19 升级为主线）**：把命令按领域抽成 `ridge-core` 纯 handler + 统一 dispatch 表；壳只注册 I/O。判据：**同一条命令，桌面/rdg/云三端走同一份 core 逻辑，差异只在 I/O 适配**。这直接消灭「rdg 缺能力」(B1) 且让新端零成本接入。
- **C2 契约 SSOT 收口**：ridge-remote 发 git crate(#4)、ridge-cloud 真复用（消手工镜像）；信令已 SSOT(#已完成)。目标：**跨端跨仓零手写副本**（部署解耦稿已示范这条路）。
- **C3 领域边界显式化（D11）**：为 workspace/pane/terminal 定义领域模型（trait 边界），让「桌面有状态实现」与「rdg 无头实现」是同一 trait 的两个 impl，而非两份分叉代码。
- **C4 分层门禁**：加 CI/lint 断言「壳不得直接依赖平台细节以外的领域逻辑」，防回潮（可后置）。

### 工程质量双柱：健壮性 + 可复用性（贯穿 C1-C4，是内核化的验收标准而非额外工作）

内核化不是「搬代码」，是借搬迁把**健壮性**与**可复用性**一次性钉进地基：

- **C5 健壮性（robustness）**——领域内核的硬规矩：
  1. **错误显式、领域内核零 panic**：命令 handler 一律 `Result<T, DomainError>`（`thiserror` 分类错误），不 `unwrap`/`expect`/越界索引；I/O/平台错误在壳层脱敏（对照部署解耦稿 `ArtifactError`→`ApiError` 的分层）。
  2. **不变式钉成测试**：迁一条命令=补一批 host-side 单测把其 load-bearing 行为固化（承接项目已有的「pin invariants」文化，见 ridge-term 237 测试），杜绝跨端重构悄悄改语义。
  3. **防御式输入边界**：所有外部输入（MCP 参数、远端载荷、路径）在内核入口 sanitize + 校验（对照 `sanitize_rel`/`parseGroupAddMember` 已有范式）。
  4. **确定性**：内核纯函数不依赖隐藏全局/时钟/随机（需要时经入参注入），保证可测、可复现（对照 `elect_leader` 的 id 平局裁决）。
- **C6 可复用性（reusability）**——一次编写、三端复用：
  1. **纯领域逻辑与 I/O/框架彻底隔离**：core 不 `use tauri`/`use axum`/不碰 fs 直连（经 trait 端口注入），从而天然可被桌面/rdg/云/测试四处复用。
  2. **SSOT 契约 crate 化**（C2）：类型/线协议/UA/dispatch 表单一来源，消副本漂移。
  3. **小而专的单元**：一个文件一个清晰职责（brainstorming 原则），公共 API 有签名契约 + docstring，便于跨端引用与独立测试。
  4. **端口-适配器（hexagonal）**：内核定义 trait 端口（如 `WorkspaceStore`/`PtySink`），桌面/rdg 各出 adapter——新端=写 adapter，不碰内核。

> 判据落到可验收：**每迁移一个领域，产出 = core 纯 handler + `DomainError` + trait 端口 + host-side 单测 + 至少两端 adapter（桌面 + rdg）**。达不到即不算收口。

## 3. 战略下注（需你点头/否决，我按推荐先走）

1. **下注一：Remote 的下一步是「控制台化」而非「远程桌面打磨」**（主线 A）。推荐 ✅——这是与北极星最对齐、竞品（VNC/Warp）最难抄的差异点。
2. **下注二：工程基座内核化（C1/#19）作为「地基优先」先行**，因为它同时解锁 Rdg 能力(B1)、让 A 线的 agent 数据在三端一致、降低后续所有远控功能的边际成本。推荐 ✅ 作为**第一个动手的 Phase**。
3. **下注三：移动端引入服务端推送（A2/#15）** 作为 Human Control 移动化的地基——需要 WS split sink，投入中等。推荐排在 A1 之后。
4. **砍**：通用文件传输、多显示器、Remote 端插件系统、rdg 图形化——都偏离「控制平面接入」。

## 4. 阶段化迭代（每阶段独立 spec→plan→执行，可验收）

| Phase | 主题 | 内容（任务映射） | 产出/验收 |
|---|---|---|---|
| **R0（地基·先行）** | 工程基座内核化 | C1/#19（git/workspace/pane/terminal 收口 core）+ C3 领域 trait；顺带 B1 | rdg 无头 host 命令不再 MethodNotFound；三端同源 core（cargo test 绿 + rdg 实测） |
| **R1** | 移动端 Agent 控制台 | A1 + A3（花名册/状态/切 pane/暂停/派活投影到 Remote 双端） | 手机连上能看 agent 列表+状态、能暂停/派活 |
| **R2** | 弱网与重连地基 | A4：#10 流压缩 + #20 精确快照 + #8 authState | 弱网/切后台回来不黑屏不丢历史；连接态单一信号 |
| **R3** | 接管移动化 | A2：#15 服务端推送 + HITL 卡片推到移动端 | 高危动作能推到手机审批 |
| **R4** | Rdg 能力补全 | B2 #9 多控制方 + B3 #12 connect_host live + B4 #21 | rdg 多端接入 + 连远端主机流终端 |
| **R5** | 安全纵深 | #7 fail-closed + #11 E2EE 桌面 host + #4 crate 化 + C2 | 零信任关阀、跨端零副本 |

（R0 先行是因为它降低 R1-R5 每一项的成本；R1 紧随，因为它兑现最大产品价值。）

## 5. 立即起点

**Phase R0 = 工程基座内核化**（下注二）。第一个 spec：把一个领域（建议 **workspace/pane 快照 + git 只读**）作为**样板**收口进 ridge-core，跑通「桌面壳 + rdg 无头壳共用同一 core handler」的闭环，**同时确立 C5 健壮性 + C6 可复用性的可验收范式**，再滚动推其余命令。

样板选 git 只读 + workspace/pane 快照，因为它们**只读、无副作用、rdg 最急需、风险最低**，最适合确立范式。样板必须交齐这套（成为后续每个领域的模板）：
1. `ridge-core` 里的纯 handler（不 `use tauri`/`axum`，经 trait 端口取数据）；
2. `DomainError`（thiserror 分类）+ 零 panic；
3. trait 端口（如 `WorkspaceSnapshot`/`GitReader`）+ 桌面 adapter + rdg adapter 两个 impl；
4. host-side 单测钉住 handler 的输入校验、错误分支、不变式；
5. 桌面与 rdg 各自跑通同一命令、结果一致（rdg 不再 MethodNotFound）。
这 5 件齐了才算「一个领域收口」，也就把健壮性/可复用性做进了地基而非事后补。

## 6. 与既有任务的关系

本稿是 #4-#27 之上的**统领框架**：把散点缺陷/缺口编织成产品叙事 + 工程地基叙事，并给出先后。逐阶段落地时各自开 `docs/superpowers/specs/` 子稿 + plan。#2（部署解耦）已完成，是 C2「跨端零副本」的先例样板。

## 7. YAGNI / 反目标

- 不做通用远程桌面（多显示器/剪贴板同步/文件管理器化）。
- 不做 Remote 端插件市场、rdg 图形界面。
- 不为「炫」加动画/主题；UI 够用即止（ROADMAP 明令）。
- 内核化不追求一次性大重构——**样板先行、滚动推进**，每步可回退、可验收。
