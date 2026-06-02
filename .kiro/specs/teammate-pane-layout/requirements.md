# 需求文档：teammate 分屏放置与自适应（teammate-pane-layout）

> 状态：定稿（全部决策锁定，已吸收 reviewer round-1/2）。语言 zh。
> 决策锁定：DA=A1（自动放置 + 显式 `auto_place` 契约）；DB=H1（锁定发起工作区 + fail-closed，#4 实做）；DC=5b（终端跟随 pane resize，`resize-pane` 维持 no-op）；DE=启动即 Busy（agent_id 可选）；DF=② child-exit→Idle 可复用；识别口径=意图驱动不做自动检测。

## 项目概述（Project Description）

- **谁遇到问题**：在 Ridge GUI 内通过 `teammateMode: tmux` 拉起 Claude Code 子 agent（teammate）的用户。
- **当前状况**：harness 启动 teammate 时固定执行 `tmux split-window -t 0 -h -l 70%`。Ridge 的 tmux shim 把 `-t 0`→`pane_index=Some(0)`、`-h`→`horizontal=true` 原样转发，导致后端「空闲复用 / 最大-pane / 最长边方向」三套智能放置（均以 `pane_index.is_none()` 为前置）被整体旁路；teammate pane 在创建后终端不跟随 pane 尺寸自适应（卡在 80×24）；teammate pane 状态 badge 卡在 `Starting`，实际 agent 已运行。
- **期望改变**：teammate 分屏一律由 Ridge 自动放置（idle 复用→最大 pane→最长边）；终端尺寸跟随 pane resize；状态 badge 真实反映 agent 生命周期。

## 术语

- **shim**：`src-tauri/src/bin/tmux.rs`，把 `tmux` 子命令翻译成 Ridge 本地 HTTP 的可执行替身。
- **GUI 路径**：shim 中 `use_native()` 返回 false 的分支（针对 Ridge GUI 工作区）；**native 路径**：`socket()!="default"`（自定义 `-S`）或会话限定 `-t`（如 `sess:win.pane`）。
- **样板参数（boilerplate）**：harness 自动注入、不代表用户空间意图的 `-t 0` / `-h` / `-l 70%`。
- **发起方工作区**：由 `X-Ridge-Workspace` 头（继承自 pane 注入的 `RIDGE_WORKSPACE_ID`）标识、发出 split 的 agent 所属工作区。

---

## 需求 1：teammate GUI 分屏一律走后端智能放置（修复观察项 1/2/3）— DA=A1

**用户故事**：作为使用 teammate 的用户，我希望子 agent 的分屏自动落在最合理的位置（先复用空闲面板，否则在最大面板上按最长边切分），无需我手动整理布局。

### 验收标准（EARS）

1. WHERE 请求走 GUI 路径，THE shim **SHALL** 以**显式 `auto_place=true` 契约**发起自动放置（而非用 `pane_index=None` 隐式编码意图），且 **SHALL NOT** 转发样板 `-t`/`-h`；后端 **SHALL** 在 `auto_place` 时忽略 `pane_index`/`horizontal` 做选择。
2. IF 发起方工作区存在空闲 shell 面板 且 `allow_idle_reuse` 为真，WHEN 收到 teammate split，THEN 后端 **SHALL** 复用该空闲面板（响应 `reused=true`），不新建叶子。
3. IF 无可复用空闲面板，WHEN 收到 teammate split，THEN 后端 **SHALL** 选「发起方工作区内面积最大的叶子面板」为目标，并 **SHALL** 沿其较长像素轴切分（宽→左右、高→上下）。
4. WHERE 请求走 native 路径（自定义 `-S` 或会话限定 `-t`），THE 系统 **SHALL** 保持现有对 `-t` 的尊重，不受本需求影响。
5. THE 实现 **SHALL** 以平台无关的参数解析完成，**SHALL NOT** 硬编码任何平台特定路径或魔法值。

---

## 需求 2：工作区隔离锁定 + fail-closed（修复观察项 4）— DB=H1

**用户故事**：作为同时开多个工作区的用户，我希望某工作区里 agent 的分屏始终落在它自己的工作区；当无法确定发起工作区时，宁可明确报错也不要误放到我当前正看的工作区。

### 验收标准（EARS）

1. WHEN split 请求携带 `X-Ridge-Workspace=WS-A` 而 GUI 焦点在 WS-B，THEN 系统 **SHALL** 在 WS-A 内创建/复用面板。
2. IF `X-Ridge-Workspace` 头缺失/解析失败，THEN 系统 **SHALL** 拒绝该 teammate 放置并返回明确错误（指明 `RIDGE_WORKSPACE_ID` 未传播），**SHALL NOT** 回退 `active_workspace_id()`。
3. IF 头指向不存在/已关闭的工作区，THEN 系统 **SHALL** 拒绝并返回「originating workspace no longer exists」类错误。
4. WHEN 发生上述拒绝，THE 系统 **SHALL** 递增可观测指标并输出结构化日志（含拒绝原因判别）。
5. THE fail-closed **SHALL** 仅作用于异常路径：正常注入（`RIDGE_WORKSPACE_ID` 已设，`terminal.rs:551`）的 spawn **SHALL NOT** 受影响。前提：任何 agent 启动路径都经该注入点（已是现状，须加注释钉死该不变量）。

---

## 需求 3：终端尺寸跟随 pane resize（terminal-follows-pane-resize，修复观察项 5b）— DC

**用户故事**：作为使用 teammate 的用户，我希望 teammate 面板的终端在创建后、以及在布局/尺寸变化后，都能把新尺寸（rows/cols）送达 PTY 并重绘，填满 pane，而不是卡在创建尺寸（80×24）。

### 验收标准（EARS）

1. WHEN 一个 teammate 面板创建/复用并完成挂载，THEN 系统 **SHALL** 保证该面板 PTY 的首次有效 fit **发生在 pty-delta Channel 注册（`setPaneDeltaMode`）之后**（确定性消除 attach↔Channel 竞态），使 kernel grid 脱离 80×24 默认。
2. WHEN GUI 布局变化（分隔条拖动 / 相邻面板增删）改变 teammate 面板行列数，THEN 系统 **SHALL** 把新尺寸同步到该面板 PTY 并触发 TUI 重绘；复用路径创建的面板同样适用。
3. WHEN teammate 面板从隐藏/0×0 工作区切回可见，THEN 系统 **SHALL** 强制重 fit，避免停留在 0×0 期间残留的错误尺寸。
4. THE 尺寸同步 **SHALL** 在 Unix PTY 与 Windows ConPTY 下均生效，**SHALL NOT** 破坏现有 ConPTY reflow 静默窗口对 Claude Code(Ink) 重绘的处理（§1.24/§A.3/§1.26），且 **SHALL** 在「窄 pane」工况下保持重排正常。
5. （范围说明）agent 发起的 `tmux resize-pane` **SHALL** 维持 no-op（GUI 拥有布局）；5a 不在本期改动范围。

---

## 需求 4：teammate pane 状态卡在 Starting（修复观察项 6）

**用户故事**：作为编排者，我希望 teammate pane 的状态 badge 真实反映 agent 生命周期（启动中→运行中→空闲），而不是 agent 实际已运行却永远停在 `Starting`。

### 验收标准（EARS）

1. WHEN teammate 面板以「无结构化 program」方式创建（裸 shell / command 串），THEN 其初始状态 **SHALL** 为 `Starting`。
2. WHEN 真实 agent 进程随后被启动进该面板（split 的 command、`spawn-process` 或 send-keys 结构化启动），THEN 系统 **SHALL** 把该面板由 `Starting` 提升为 `Busy`，并 **SHALL** emit `teammate-layout-changed` 使前端 re-sync。
3. WHEN 面板变为 `Busy` 且存在可解析 `agent_id`，THEN `get_pane_layout` **SHALL** 回传 `agent_state="busy"` 与该 `agent_id`，前端 **SHALL** 渲染 AGENT badge（含 id）。
4. WHEN agent 进程退出，THEN 系统 **SHALL** 按决策 DF 达到一致终局（销毁 / 保留为 Idle 可复用），且 `teammate_pane_states` **SHALL NOT** 残留 `Busy`/`Starting`（消除反向泄漏）。
5. THE F1 提升入口 **SHALL** 覆盖 harness 真实主路径 `send-keys → spawn-process`，而不仅是「program 内嵌于 split-window」的子集（后者真实极少命中）。
6. IF 提升信号缺失（边界），THEN 看门狗 **SHALL** 在 PTY active + 宽限期后避免永久 `Starting`，且 **SHALL** 按 pane 所属 workspace 操作（**禁读** `active_workspace_id`）。
7. WHEN agent 在 Windows ConPTY 与 Unix PTY 下退出，THEN 状态终局 **SHALL** 一致（覆盖 reader EOF 两平台差异）。
8. WHEN N 个 split-window 并发到达，THEN 系统 **SHALL** 产生 N 个不同叶子、无 pane_index 串号、无状态错配。
9. THE **内嵌-program** split 路径 **SHALL** 继续直接置 `Busy`；GUI「在此运行 agent」按钮（`register_teammate_agent`）**SHALL** 保持可用；`new-window` 恒 Busy 的第三态行为 **SHALL** 按 F1 决议统一。

10. **（征用空闲 pane，核心 · DA=A1 后为主导路径）** WHEN agent 被启动进一个**被征用的空闲 pane**（`route_split` idle-reuse 分支，`program` 常为 None），THEN 系统 **SHALL** 凭意图位立即把该面板置 `Busy` 并显示 AGENT badge；WHEN 其 agent 退出，THEN **SHALL** 转 `Idle`。**与新 split pane 共用同一提升/降级逻辑。**

### 决策锁定（识别口径与生命周期）

- **DE=启动即 Busy**：teammate 模式下「启动即 Busy」，`agent_id` 为可选元数据（能解析则填，否则 Busy 无 id）。
- **DF=② child-exit→Idle 可复用**：agent 退出后面板置 `Idle`、清 agent 映射、可被需求 1 复用。
- **识别口径=意图驱动，不做自动检测**：手动在普通 pane 敲 `claude` 不要求识别；agent 身份完全由 F1 意图位驱动（前台进程嗅探 F2 移出范围）。
- 整条 `Starting→Busy→Idle` **SHALL** 由 Ridge 自身信号（spawn 意图位 + reader EOF）驱动，**不依赖 harness** 调 register-agent/release-pane。

---

## 可追加观察项区（用户后续补充）

> 本区用于追加新观察项；每条按「现象 → 期望 → 关联需求」记录，设计阶段再补根因与方案。

- （占位）`-l 70%` 尺寸提示当前被丢弃：自动放置后新面板为 50/50 树切分。是否需要尊重百分比尺寸？— 关联需求 1，候选「分期 P3 / 暂列范围外」。
