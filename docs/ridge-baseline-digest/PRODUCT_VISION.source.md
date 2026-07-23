# PRODUCT_VISION.md

> 由 NotebookLM 来源导出归档（2026-07-23），原 char_count=4983

Ridge 产品与架构愿景

状态：方向性约束，不是承诺清单。




配套来源：

PROJECT_STATUS.md

 描述现在，

GAP_PORTFOLIO.md

 描述差距与优先级。




时间尺度：以可验证用户结果为阶段，不绑定旧路线图中的版本号与完成百分比。

1. 北极星

Ridge 要成为一个

本地优先、随处可接入的人机协作开发控制平面

：

人能看见每个开发智能体在做什么，能在关键时刻拦截、接管和恢复；多个智能体能在同一工作空间中协作；工作上下文不因终端、设备、模型或会话切换而丢失。

Ridge 的差异化不在“又一个聊天框”或“又一个远程桌面”，而在四个结果：

可见

：Agent 身份、状态、任务、改动与故障可被理解。

可控

：危险动作可审批，单个 Agent 可暂停、恢复、接管和回滚。

可续

：工作区记住目标、约束、决策、任务和运行状态。

可协作

：多个本地 CLI Agent 经可见的 Pane、tmux 与 MCP 共同工作，人始终拥有最终裁决权。

Remote 与 

rdg

 是“控制平面的随处入口”：离开工位后，用户仍能看状态、切到对应 Pane、处理审批和接管故障；它们不是通用 VNC。

2. 目标用户体验

2.1 桌面：主控制室

用户打开一个 Ridge 工作区后，应能在同一处：

运行本地 shell、Claude Code、OpenCode 等现有 CLI，不迁移 API key，不被 Ridge 绑定模型供应商；

看到各 Agent 的身份、任务、状态、相关 Pane、关键文件改动和最近失败；

在终端、文件、Git diff、测试结果与审批之间快速切换；

对危险操作批准、拒绝或改写；对异常 Agent 暂停并人工接管；

关闭再打开后恢复稳定的工作区上下文，而不是依赖某个聊天会话记忆。

2.2 手机/浏览器：随身控制台

Remote 的目标不是复制全部桌面 IDE，而是提供高杠杆最小集：

一眼看到 Agent roster、Working/Idle/Blocked、当前任务与异常；

一键切到某个 Agent 的终端 Pane；

接收并处理 HITL 审批；

暂停/恢复或向 Leader 派发简短任务；

在弱网、换网、切后台后恢复到正确会话，不黑屏、不串 Pane、不丢必要历史。

完整 Monaco、插件市场、多显示器和通用文件管理不是移动端成功条件。

2.3 

rdg

 / SSH：终端原生入口

在无图形界面的 VPS、服务器或 SSH 会话中，用户应能：

运行与桌面一致的工作区/Pane/Remote 核心语义；

托管无头 Agent 或连接已有 Ridge host；

查看团队状态、处理必要控制动作；

不因宿主不同而遇到同名命令 

MethodNotFound

 或行为分叉。

2.4 多 Agent：可见的协作网络

一个 Agent 对应一个真实运行环境与可见 Pane，不是后台不可观察的抽象任务。

Leader/Worker/Observer 是协作角色，不是不可变等级；用户可调整和接管。

MCP 提供结构化通信与资源；tmux/PTY 提供最低兼容层。Agent 不支持 MCP 时仍能加入基本协作。

后台 Worker 可被召唤到前台，输出和进程不因 UI 挂载而重启。

编排首先服务真实任务依赖和冲突处理，不为展示拓扑而制造复杂图系统。

3. 目标架构

┌────────────────────────── 表现与入口 ──────────────────────────┐
│ Desktop Svelte/Tauri │ Remote desktop/mobile │ rdg TUI/daemon │
└───────────────┬──────────────────┬──────────────────┬──────────┘
                │                  │                  │
┌───────────────▼──────────────────▼──────────────────▼──────────┐
│            统一能力协议、命令表、错误与事件语义               │
│        ridge-core + ridge-remote + ridge-signaling types       │
└───────────────┬──────────────────┬──────────────────┬──────────┘
                │                  │                  │
┌───────────────▼──────┐  ┌────────▼────────┐  ┌─────▼──────────┐
│ Desktop platform ports│  │ Headless/CLI ports│  │ Remote adapters │
│ PTY/fs/git/Tauri/event│  │ PTY/fs/config/TUI│  │ LAN WS / cloud  │
└───────────────┬──────┘  └────────┬────────┘  └─────┬──────────┘
                └──────────────────┼──────────────────┘
                                   ▼
                    ridge-term terminal semantics

Cloud control plane: identity, device, entitlement, signaling, TURN,
static entry and artifact activation; never owns terminal plaintext.


text

3.1 内核与适配器边界

目标不是把所有代码塞进 

ridge-core

，而是让

稳定领域语义只有一份

：

core 定义命令、领域错误、输入校验、不变量和端口；

desktop、CLI、Remote 只实现平台 I/O、生命周期、权限和事件传输；

同一能力在多个入口复用同一 handler；入口不支持时显式声明，而非静默分叉；

平台特有功能留在适配器，不因“内核化”制造假抽象。

3.2 终端与工作区

ridge-term

 是终端语义 SSOT：parser、grid、selection、scrollback、search 和 render invariants 一致。

PTY/Pane 生命周期与 UI 挂载解耦；折叠侧栏、切 Tab、Remote 重连不应杀进程或丢状态。

工作区是持久边界，Pane 是表现容器，Agent runtime 是协作主体；三者不混为同一数据模型。

3.3 远控

LAN 和 cloud 复用能力协议与终端模型，只替换传输和身份。

cloud relay 只做认证、授权、房间和信令；业务数据端到端加密。

Remote UI 以控制平面任务为中心：状态、审批、接管优先于桌面功能搬运。

恢复协议必须显式包含身份、版本、Pane、历史游标和授权状态，避免靠页面重载碰运气。

3.4 云服务与发布

ridge-cloud

 的协议文档是跨仓唯一 SSOT；类型与 conformance 尽量由机器同步/验证。

账户、设备、订阅与房间状态不进入终端领域内核。

应用服务 SHA 与 Remote artifact version 分开部署、查询、回滚和审计。

安全默认趋向 fail-closed；兼容回落必须可观测、有退役条件，不能永久隐形存在。

4. 目标数据模型

4.1 Workspace Memory

每个工作区只持久化高价值、低噪声信息：

当前目标与成功标准；

稳定架构决策与约束；

活跃任务、依赖与负责人；

最近关键结果、阻塞与待验证假设；

运行资源引用，例如工作区、Pane、Agent、Git checkpoint，而不是聊天全文。

记忆必须有来源、更新时间和淘汰规则。自动写入不能把日志、完整对话和瞬时状态无限堆积。

4.2 Agent Runtime

目标最小模型：

Agent {
  stable_id, display_name, runtime_kind,
  workspace_id, pane_id?, role,
  state, current_task?, last_progress?,
  changed_files?, risk_state?, checkpoint?,
  last_seen_at
}


text

状态必须来自可验证信号；不要用标题文本或模型猜测伪造“正在思考”。允许 Unknown，并明确观测延迟。

4.3 Task 与事件

Task 表达目标、验收、依赖、负责人和状态，不保存冗长思维过程。

Event 只记录影响控制决策的事实：任务开始/完成、文件集合变化、测试结果、审批、熔断、接管、checkpoint。

Timeline 是派生视图；事件存储有容量和保留策略。

5. 安全与治理愿景

用户始终是最终权限主体；Leader 不能绕过用户安全策略。

L2/外部影响操作默认经明确授权；超时拒绝。

暂停/恢复、审批和回滚针对具体 Agent/进程/工作区，不做全局模糊开关。

Remote controller 只能访问白名单能力；本机 MCP token、TOTP seed、设备私钥不可远程读取。

身份绑定、TOTP bind 和 trusted-controller 在支持入口最终应 fail-closed；旧回落有遥测和退役计划。

所有自动回滚先保护用户未提交改动，并以可恢复 checkpoint 为基础。

6. 成功衡量

不以功能数量或文档完成度衡量。优先使用这些结果指标：

用户能否在 10 秒内看出“谁在做什么、谁卡住、是否需要我”。

从危险动作出现到用户完成审批的时间与误拦/漏拦率。

Agent/应用/网络重启后，恢复正确工作区与任务上下文的成功率。

desktop、LAN、cloud、

rdg

 对共享命令的 conformance 通过率。

弱网/切后台恢复时间、重复帧/丢历史/串 Pane 率。

每次发布能否同时证明 cloud SHA 与 Remote artifact version。

共享内核迁移是否实际减少重复实现和缺陷，而非只增加 trait 数量。

7. 明确非目标

不做通用远程桌面、VNC、多显示器平台。

不做万能 AI IDE 或照搬 VS Code 功能表。

不重新训练模型，不托管用户模型密钥，不绑定单一 Agent CLI。

不以聊天窗口、动画、主题数量或插件市场作为核心竞争力。

不让 Agent 自治凌驾于人类审批、数据安全和可恢复性。

不为未来假想端一次性大重构；每个抽象至少有两个真实入口与确定测试。

不永久维护手写重复协议、重复架构文档和不断增长的 handoff 墓地。

8. 决策过滤器

任何愿景提案先回答：

它是否明显增强可见、可控、可续或可协作中的至少一项？

用户是否能在真实工作流中感知价值，而非只让架构图更漂亮？

能否先复用现有终端、Remote、MCP、Git 与工作区能力？

是否引入新的协议副本、状态源或不可恢复写路径？

是否有低成本实验能先证伪？

删除或简化现有能力是否更接近目标？

若前两问为否，原则上不进入路线图。
