# GAP_PORTFOLIO.md

> 由 NotebookLM 来源导出归档（2026-07-23），原 char_count=6235

Ridge 愿景差距组合与优先级

基线日期：2026-07-21




计算方式：

PRODUCT_VISION.md

 的目标减去 

PROJECT_STATUS.md

 的代码/运行事实。




用途：帮助 NotebookLM 做取舍，不是自动生成待办；每项进入开发前仍需 CodeGraph 与运行证据复核。

1. 排序方法

每项用五个因素判断，不做伪精确加总：

用户价值

：是否直接改善可见、可控、可续、可协作。

风险降低

：是否减少安全、数据丢失、发布或协议漂移风险。

解锁力

：是否降低多个后续目标成本。

证据置信

：差距是否由当前代码确认，而非旧文档猜测。

成本/可逆性

：能否小步验证、失败后撤回。

优先级含义：

P0

：不先解决就不能可信开发/发布，或有重大安全与事实风险。

P1

：高产品价值或高解锁力，应成为近期少量主线。

P2

：有价值，但应在 P0/P1 建立数据或基础后推进。

P3/实验

：先证伪；无量化收益则停做或删除。

2. 总览

ID

差距

类别

价值

风险降低

解锁力

置信

成本

优先级

T1

当前开发门禁不可运行

可信基线

高

高

高

高

低-中

P0

T2

Cloud 协议存在双 SSOT 漂移

契约

高

高

高

高

低

P0

T3

生产两条版本线缺统一状态证据

发布/运维

高

高

中

高

中

P0

S1

兼容安全回落未形成可观测退役闭环

安全

高

高

中

中-高

中

P0/P1

P1

Remote 没有 Agent 控制台

产品

很高

中

高

高

中

P1

P2

Remote HITL/接管没有闭环

产品/安全

很高

高

高

高

中-高

P1

G1

只有审批/熔断，缺单 Agent 暂停恢复与可恢复接管

治理

很高

高

中

高

中-高

P1

A1

多入口能力仍有重复/不一致

架构

高

中

很高

中-高

中

P1

A2

跨入口支持矩阵不显式

契约/测试

高

高

高

高

低-中

P1

R1

弱网与恢复有代码但缺当前指标

Remote 质量

高

中

中

高

低-中

P1

M1

工作区缺受控的长期项目记忆

连续性

很高

中

高

高

高

P2

M2

缺 Agent 归因的变更/进度事件模型

可见性

高

中

高

中-高

中-高

P2

H1

远端 host 目前仍偏“登记”，live PTY 未闭环

多主机

中

低

中

高

高

P2

C1

rdg

 与桌面行为一致性缺系统证据

CLI

中-高

中

中

中

中

P2

E1

WebGPU 收益未经当前真机验证

性能实验

未知

低

低

高

中-高

P3/实验

E2

高级自动编排/能力矩阵缺真实需求证据

协作实验

未知

低

低-中

中

高

P3/实验

3. P0：可信开发、契约与发布

T1. 恢复可运行门禁

现状证据

：本轮 

pnpm check

 / 

pnpm test

 因沙箱找不到 

pnpm

；两个 Rust 测试因 stable toolchain manifest 缺失而未启动。

愿景差距

：系统要求跨 desktop/Remote/CLI/cloud 的确定性行为，但当前无法生成新的绿灯，任何“已完成”都只能依赖历史记录。

最小动作

：

修复开发机 Node/pnpm 与 Rust toolchain 可见性，不改产品代码。

固化四个最低门禁：

pnpm check

、

pnpm test

、

cargo test --workspace

、

ridge-cloud cargo test

。

将入口 conformance、协议 drift 和关键 Remote E2E 纳入可选择的高风险门禁，不默认把所有 GUI 测试塞进每次小改。

验收

：四个命令从干净环境退出 0，失败日志保留首因；文档只记录当前命令与最近一次可追踪结果，不写永久测试数。

T2. 消除 Cloud 协议双 SSOT

现状证据

：

wind

 协议副本比云仓权威文档少 SSO、EdDSA、设备配额、结构化错误与多控制方等大量变化；diff 为 260 insertions / 162 deletions。

最小动作

：云仓保留唯一全文；

wind

 保留短入口与同步/校验说明。协议改变时先改云仓，再更新代码和生成类型/测试。

验收

：仓库内搜索只出现一个自称“唯一事实来源”的全文；CI/drift 测试能发现消息、错误码和字段副本不一致。

T3. 统一证明两条生产版本线

现状证据

：Cloud 服务代码与 Remote desktop/mobile artifacts 已解耦；当前上传/回滚 API 有实现，但本轮未验证生产 current 指针、文件和健康状态。

最小动作

：提供只读状态证据，至少返回/采集：cloud Git SHA、health、activated artifact version、manifest gitSha/builtAt、desktop/mobile index/hash、保留版本。

验收

：发布与回滚报告能用一次命令或固定 runbook 证明两条线；缺一项即失败。

S1. 兼容安全回落的可观测退役

现状证据

：桌面 cloud host 已注入 TOTP verifier、可用时注入 TOTP bind，并提供设备身份签名；取不到设备身份时会回落旧握手。通用 bridge 为兼容未注入 verifier 的构造点保留放行语义。

愿景差距

：目标是支持入口最终 fail-closed；当前无法仅凭能力代码证明所有 desktop/CLI/mobile 构造点都已启用，且回落没有统一生产状态证据。

最小动作

：先做构造点矩阵与遥测，不直接翻总开关。统计握手版本、binding mode、回落原因；确认旧客户端占比与升级路径后，按入口逐步 fail-closed。

验收

：每类 host/controller 有明确“必需 verifier + 回落政策 + 退役日期/条件”；E2E 证明中间人/错误身份/重放/明文 TOTP 不被接受。

4. P1：把控制平面带到任何入口

P1. Remote Agent 控制台 MVP

现状证据

：

AgentCenterPanel

 只由桌面 

+page.svelte

 渲染；

src/remote

 未出现同等 roster 控制面。Remote 主要呈现终端、工作区树和文件。

用户价值

：这是“远程桌面”与“AI 开发控制平面随处入口”的分水岭。

MVP 范围

：

当前工作区 roster：名称、角色、Working/Idle/Disconnected；

点击成员切换到其 Pane；

显示最近异常/熔断与当前组任务的最小摘要；

不加入能力编辑、复杂拓扑图、动画和移动端完整 Agent 配置。

依赖

：将只读 teammate topology 以 Remote 白名单安全暴露；不得暴露本机 MCP endpoint/token。

验收

：手机和桌面浏览器均能在 10 秒内定位正在工作/故障的 Agent 并切到对应 Pane；无团队时不增加噪声。

P2. Remote HITL 与接管闭环

现状证据

：桌面有 

resolve_hitl_request

 和审批 UI；该能力刻意不在 Remote allowlist，当前远端没有安全裁决通道。

设计要点

：不要简单把桌面 IPC 命令加入白名单。需要 controller 身份、审批请求 nonce、单次消费、过期、审计和多 controller 竞争裁决语义。

阶段

：

远端只读展示待审批与风险原因；

单 controller、一次性 approve/reject；

多 controller 首次裁决胜出，其他端收到已处理状态；

需要时再增加安全的“修改后执行”。

验收

：未授权 controller、重放、过期请求和重复裁决均失败；网络中断默认拒绝或保持挂起到明确超时。

G1. 单 Agent 暂停、恢复、接管与回滚

现状证据

：当前代码确认 HITL 裁决与循环熔断；CodeGraph 未发现愿景中的通用 pause/resume、SIGSTOP/SIGCONT 或 per-Agent checkpoint 主链路。

范围顺序

：

先定义跨平台状态机与可暂停边界，明确 Windows/Unix 差异；

暂停输入与任务派发，不误杀共享 PTY/工作区；

人工接管时记录所有权转换；

回滚只基于显式 Git checkpoint/补丁，并保护用户原有未提交改动。

验收

：只影响目标 Agent；暂停后无新副作用；恢复时能看到人工期间变更；回滚可预览、可拒绝、可恢复。

A1. 共享内核的减法审计

现状证据

：7 月提交已将大量 workspace/pane/Git 命令迁入 

ridge-core

，旧路线图“尚未迁移”的表述已过期；但 

src-tauri/commands

、前端 store 和 CLI adapter 仍有多层边界。

正确目标

：不是继续按文件数搬迁，而是找出仍有两份行为实现且导致入口差异的地方。

最小动作

：按能力逐项画调用路径；每次只删除一个有等价共享 handler、两个真实 adapter 和回归覆盖的重复面。平台特有逻辑保留在壳。

验收

：重复实现或分叉减少；同一输入在 desktop/CLI/Remote 得到同一领域结果；不以新增 trait 数为成功。

A2. 跨入口能力矩阵与 conformance

现状证据

：Remote 有 allowlist，CLI/desktop/云各有适配；一些能力会显式禁用，另一些可能因缺 adapter 返回错误。

产物

：机器可读矩阵：能力、desktop、LAN、cloud desktop、cloud mobile、

rdg host

、

rdg controller

，每格为 supported / denied / degraded / not-applicable，并指向测试。

验收

：新增能力必须声明矩阵；允许的入口通过相同 contract test；禁止入口返回稳定错误而非超时或静默 no-op。

R1. 弱网与恢复的证据化

现状证据

：重连、ICE restart、watchdog、分片、背压、懒加载、multi-controller 都已有实现；旧战略稿中多项“未实现”已过期。

最小动作

：先测而不是继续写。构造换网、后台冻结、TURN-only、5% 丢包/高 RTT、大 scrollback、多 controller 同 Pane 场景。

验收指标

：恢复时间、错误状态准确率、重复/丢帧、历史缺口、内存上限、错误 controller 串流为零。只有指标失败再定具体代码任务。

5. P2：连续性、归因与多主机

M1. 有边界的 Workspace Memory

差距

：当前有 

.ridge

 工作区持久化、文档与 NotebookLM 外部知识，但没有统一、可淘汰、可供不同 Agent 恢复的项目记忆模型。

先做 discovery

：用一个真实项目验证最小 6 字段：目标、验收、约束、决策、活跃任务、最近阻塞。人工确认写入；不做自动向量库和全对话抓取。

停止条件

：若结构化摘要不能明显减少重启/换模型后的重复解释，就不建设更重的 RAG。

M2. Agent 归因事件

差距

：已有文件 watcher、Pane Git 状态、teammate 状态与熔断，但缺稳定的“哪个 Agent 因哪个任务改了哪些文件、跑了什么验收”的事件链。

最小模型

：task_assigned、files_changed(set)、verification_result、approval、blocked、completed。避免记录每条终端输出。

依赖

：Agent stable_id 与 Pane/进程生命周期先可靠；否则归因会误导。

H1. 远端主机 live PTY

现状证据

：

hosts.ts::connectHost

 注释明确当前仅登记与展示，真正出站连接/PTY 流是后续里程。

价值判断

：对多机器开发有价值，但不是当前控制平面差异化的最短路径。先确认用户是否频繁需要从桌面聚合多个远端 host；若需求成立，复用现有 LAN/cloud Remote provider，不发明第三套协议。

C1. 

rdg

 行为一致性

先用 A2 矩阵暴露真实缺口，再补最常用的无头工作流。不要以“CLI 必须复制完整桌面 IDE”作为目标。

6. P3 / 实验：先证明再投资

E1. WebGPU

只做对照实验：典型 1/4/10 Pane 的帧时间、CPU、GPU 内存、字形正确性、IME/selection/TUI 回归。若相对 Canvas2D 没有稳定显著收益，保留实验开关或删除，停止 shared-surface 扩张。

E2. 高级自动编排

动态能力矩阵、自动领任务、复杂 task graph、性格路由等都必须由真实多 Agent 瓶颈驱动。当前优先把 roster、任务、审批、接管与冲突处理做可靠。没有使用证据时不扩协议和 UI。

7. 建议近期组合

为了避免同时开太多主线，推荐组合为：

可信基线包（P0）

：T1 + T2 + T3；S1 先做审计与遥测设计。

一个产品主线（P1）

：Remote Agent 控制台 MVP。

一个工程护栏（P1）

：跨入口能力矩阵 A2；只在矩阵暴露真实重复时做 A1 减法。

测量而非开发

：R1 弱网场景基准。

在这四项完成前，不建议同时启动 Workspace RAG、WebGPU shared surface、复杂自动编排或新 Remote 桌面功能。

8. NotebookLM 评审要求

NotebookLM 对任何下一迭代建议必须输出：

对应愿景承诺与差距 ID；

当前代码证据或需补查符号；

用户价值、风险降低、解锁力、成本与可逆性；

至少一个减法/不做方案；

确定验收信号与停止条件；

会不会引入新状态源、协议副本、不可恢复写路径或生产运维负担。

若建议不能映射到本组合中的差距，默认不进入近期计划。
