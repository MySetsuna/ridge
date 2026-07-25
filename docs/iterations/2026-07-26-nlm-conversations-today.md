# NLM 对话记录（Ridge 笔记本，抓取于 2026-07-26，今天以来全量 20 条）

> 来源：NotebookLM UI 聊天面板（notebook 66919cb9），CDP 抓取。USER = 用户原文（原意基准）。

## [0] USER

现在git进程有没有abort能力。我希望能做abortcontroller的管理。

---

## [1] NLM

根据架构现状与迭代记录，关于 Git 进程的 Abort（中止）能力，目前的情况如下：
1. 现有的 Abort 能力：超时硬护栏
目前 Git 进程已经具备了基于超时的自动中止能力
1
。
实现机制：在最近的迭代（Iter 19, 22-31）中，项目引入了 “Git 进程硬护栏”，通过 git_output 包装器实现
1
。
中止行为：该护栏设定了墙钟超时时间（Wall-clock timeout），一旦 Git 进程运行超时，后端会执行 “超时杀树（timeout kill tree）” 操作，强制清理掉堆积的 git.exe 进程及其子进程
1
。
2. 关于引入 AbortController 的管理
您提出的使用 AbortController 进行管理非常符合 Ridge “可控（Controllable）” 的核心愿景
2
。
当前缺口：虽然后端具备了超时自动 Abort 的能力，但目前在前端 UI 侧可能还缺乏一个直接关联 AbortController 的手动中止机制。根据您的反馈，v0.1.2 版本中仍存在 git.exe 多次启动并卡死页面的问题
1
。
下一迭代规划：在 v0.1.2 之后的 Iteration 21 规划中，已经明确了以下改进方向，这为您实施 AbortController 管理提供了基础：
进程池管理：将 Git 仓库检索与更新接入进程池，强制限制最高并发为 3 个
1
。
信号量与超时：引入信号量（Semaphore）获取超时逻辑，防止前端请求在后端无限期排队
1
。
3. 实现建议
若要实现基于 AbortController 的管理，可以考虑在 packages/ridge-core（负责承接 Git 命令
3
）与前端 RpcClient 之间建立取消信号的映射：
前端：在发起 Git RPC 调用时关联一个 AbortSignal。
后端：当收到取消信号或连接断开时，触发 Rust 侧对对应 Child 进程的 kill() 操作。
目前项目的架构已经支持 “能力先协商” 和 “RPC 调用”
4
，您可以将此手动 Abort 能力作为一种新的 Capability（能力） 注册到 capability.rs::REMOTE_ALLOWLIST 中，以确保桌面端和远端都能安全地管理这些进程
4
。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [2] USER

我希望做的是，进入某个cwd再快速切换，那么上一个git检索进程就能abort掉

---

## [3] NLM

针对您希望在快速切换 CWD（当前工作目录）时中止（Abort）上一个 Git 检索进程的需求，基于《ARCHITECTURE.md》中的架构现状及我们之前的讨论，以下是具体的实现路径与技术建议：
1. 现状分析与缺口
现有能力：v0.1.2 已经具备了基于**墙钟超时（Wall-clock timeout）**的“硬护栏”，通过 git_output 包装器实现“超时杀树（kill tree）”
1
。
缺失能力：目前系统虽然能防止 Git 进程无限期运行，但缺乏**“请求级预取/覆盖”**逻辑。当您快速切换 CWD 时，旧的 Git 请求仍在后端排队或运行，占据了 3 个并发名额（Semaphore），导致新请求被阻塞，产生您提到的前端卡死感 。
2. 推荐实现方案：基于“Last-Win”的中止管理
为了实现“进入新 CWD 即中止旧进程”，建议在 packages/ridge-core 层引入以下机制：
A. 句柄追踪与覆盖 (Process Handle Mapping)
在 ridge-core 的 Git 命令承接模块中，建立一个 PaneId -> ChildProcessHandle 的内存映射：
动作：每当发起针对某个 Pane 的 Git 检索时，先检查该映射表中是否已存在该 PaneId 的活跃句柄。
中止：如果存在，立即触发 kill() 操作并清理该进程树（利用现有的杀树逻辑）
1
。
替换：启动新进程，并将新句柄存入映射表。
B. 联动信号量（Semaphore）释放
由于我们设定了 最高并发 3 个 的硬限制，主动 Abort 必须能够即时释放信号量许可。
优化：确保 AbortController 或后端的取消信号能强制退出 acquire 排队逻辑，使最新的 CWD 请求能立即获得执行位置。
3. 确定性验收信号 (Iteration 21 补强)
您可以按照以下标准验证该功能是否闭环：
进程计数验证：
操作：在终端执行脚本或手动快速连续切换 10 次目录（cd .. && cd ..）。
预期：通过 ps 或任务管理器观察，git.exe 的瞬时并发数严格 ≤ 3，且旧目录的 Git 进程应在切换瞬间消失。
UI 响应验证：
操作：在 Git 重负载目录下快速切换。
预期：侧边栏 Tab 和工作区切换保持流畅，不再出现 v0.1.2 中的“卡死无反应”现象 。
退出码断言：
测试：编写 Vitest 用例模拟 CWD_CHANGED 事件的高频触发，断言 RpcClient 只收到最后一次请求的正确返回，而前几次请求应返回自定义的 ABORTED 错误码。
4. 架构守卫建议 (§3)
SSOT 守卫：此中止逻辑必须收口在 packages/ridge-core 的异步 dispatch 层，严禁在前端 UI 组件中分散管理进程生命周期
2
。
减法审计：实现此功能后，可以考虑缩短 git_output 的默认硬超时时间，因为主动 Abort 已经解决了大部分由于用户操作导致的堆积问题。
通过这种**“请求关联-覆盖中止”**的策略，可以彻底解决 v0.1.2 中 Git 检索进程由于“反馈环”导致的堆积问题，从而实现您追求的“无拘无束”的交互感。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [4] USER

Agent自动发现要怎么实现，我希望轻量化，并且性能要好

---

## [5] NLM

基于《ARCHITECTURE.md》中的架构现状及我们此前关于 v0.1.2 性能瓶颈（如 Git 进程堆积）的讨论，实现轻量化且高性能的 Agent 自动发现方案应遵循以下技术路径：
1. 核心发现机制：从“深度扫描”转向“进程指纹 + CWD 嗅探”
为了避免重蹈 Git 递归遍历导致卡死的覆辙，自动发现必须严格限制 IO 深度：
进程名指纹（Process Fingerprinting）：
实现：由 ridge-core 维护一个轻量级的观察者（Observer），定期（如每 5-10 秒）或在 CWD 切换时扫描当前的进程树
1
。
轻量化方案：仅匹配已知的主流 Agent 二进制名称（如 claude-code、aider、open-interpreter）。这种内存级匹配对性能几乎无影响
2
。
单层 CWD 探测（Single-level Sniffing）：
实现：当用户进入某个工作区或 Pane 切换目录时，仅在当前目录下寻找特定的配置文件（如 .aider.conf.yml）。
性能护栏：严禁向下递归。只在当前层级进行一次 fs::metadata 调用，确保 IO 开销为 O(1)
3
。
2. 性能与并发优化方案
无头终端（Headless PTY）联动：
利用现有的 packages/remote/src/shared/terminal/manager.ts 统一管理能力，将识别到的 Agent 后台进程标记为 headless_pty
2
4
。
高性能保障：发现逻辑仅产生元数据（JSON），不启动新的 PTY，也不建立实时流，直到用户点击“唤起”时才执行 attach_pane
1
2
。
自适应频率与防抖：
复用 GitWatcher 的防抖逻辑（Bug 4 经验）。在高频切换目录时，自动发现任务应进入 AbortController 管理，上一个探测任务会被立即中止，确保只有一个探测进程在运行
5
。
3. 架构接入点（Iteration 21 规划）
后端（ridge-core）：
在 src-tauri/src/teammate/ 目录下新增 discovery.rs。
将发现结果注入到 get_teammate_topology 投影中，作为“待确认（Discovered）”状态返回
1
。
能力门控（Capability Gate）：
将 is_auto_discovery_enabled 作为一个新的配置项放入 Settings 页面。
该功能必须在 capability.rs::REMOTE_ALLOWLIST 中注册，确保 Remote 端（手机/Cloud）也能同步获取到自动发现的 Agent 列表
6
。
4. 确定性验收信号
您可以根据以下标准判定实现是否达标：
性能断言：在拥有 10 万个文件的目录下启动 aider，Ridge 界面不应出现任何超过 16ms 的卡顿；ps 观测发现逻辑的 CPU 占用率应接近 0%。
逻辑断言：
手动/自动对齐：关闭设置中的“自动发现”开关，TeammateRegistry 应立即停止进程扫描。
正确识别：启动 claude-code 后，侧边栏 Tab 应在 5 秒内自动出现其图标及当前执行命令的轻量摘要
1
。
这种方案完全剥离了 LLM 开销，通过进程级指纹识别实现了极致的轻量化，同时利用 CWD 触发机制 保证了响应的实时性。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [6] USER

内置mcp现在有哪些功能，我希望能实现多agent，不同类型agent的协同工作，编组协同

---

## [7] NLM

根据最新的架构文档（v0.1.2 及其之前的迭代事实），Ridge 内置的 Teammate/MCP（智能体控制面） 目前已具备基础的拓扑感知、生命周期管理与安全网关功能，但关于“深度编组协同”与“跨类型感知”仍处于规划实施阶段
1
。
以下是当前功能的详细清单及实现您愿景的技术路径：
1. 内置 MCP/智能体控制面当前功能（v0.1.2 现状）
目前的内置功能主要聚焦于观测与安全管控，确保 Agent 在本地或远端运行时的可控性：
状态与编排健康度监测：通过 get_orchestration_health 接口，系统可以获取所有智能体的运行状态快照，包括 suspended（已暂停）或 pending（等待审批）状态
1
。
安全审批流（HITL）：实现了 只读脱敏投影。当 Agent 触发敏感操作时，系统会生成一个 nonce（随机数），支持通过远端或桌面端进行单次消费裁决 (resolve_hitl_remote)，但严禁远端修改操作内容
1
,
2
。
统一写路径收口：所有智能体的写操作均收口于 agent_pty_write。当智能体处于 suspended 状态时，系统会强制拒绝其写入请求，确保人类的最终控制权
1
。
基础拓扑发现：系统通过 get_teammate_topology 提供基础的 Roster 视图，能够识别当前连接到工作区的智能体实例
3
。
2. 实现多 Agent 与编组协同的规划路径
您提到的“多 Agent 协同”和“编组 MCP 融合”是下一迭代（Iteration 21）的核心任务，以下是结合当前架构的实现建议：
编组 MCP 融合与“队友意识”：
现状：目前 MCP 主要作为环境嗅探的汇总点
4
。
改进：在下一迭代中，需要将**编组元数据（组名、成员列表、组目标）**注入到内置 MCP 的 Context 中。
效果：通过 MCP Tool，Agent 可以主动查询当前编组内的“队友”是谁、正在做什么（基于 summary_snapshot），从而实现任务分配
1
。
跨类型 Agent 自动识别：
技术手段：利用 进程指纹识别（Process Fingerprinting） 识别如 claude-code、aider、open-interpreter 等主流 Agent 进程。
轻量化实现：仅通过扫描 CWD 下的特定配置文件（如 .aider.conf.yml）或进程名，将这些异构 Agent 映射为统一的 Teammate 模型，无需为每种 Agent 编写独立的插件。
打破工作区隔离的全局展示：
目前的 UI 已具备多 Host 与多 Pane 管理能力
3
。
下一步：在 AgentCenterPanel.svelte 中移除工作区过滤，实现全量 Agent 展现。利用 竖向色条 区分不同编组，并允许 Agent 通过 MCP 请求创建/召集新队友，实现动态编组协同。
3. 架构建议：如何保持“无拘无束”的协同
为了实现您要求的“大方、简约、实用”的协同感，必须遵守以下架构约束：
坚持协议 SSOT：所有的编组与协同状态必须先定义在 ridge-cloud-protocol.md 契约中，由 ridge-core 统一广播，严禁 UI 侧私自管理队友状态副本
2
。
利用 H1 出站能力：利用已实现的 OutboundTransport，可以支持一个 Agent 在 Host A 工作，而其指令和协作信号广播到 Host B 的 Controller 上，实现真正的跨端协同
5
。
轻量化总结替代重型 LLM：在智能体列表中，直接截取 PTY 输出的末行命令或任务标题作为摘要，避免调用额外 LLM 导致性能下降
6
。
总结：目前 v0.1.2 已经打好了 “安全门控” 和 “拓扑感知” 的底子。实现“编组协同”的杠杆点在于将编组逻辑通过 MCP 协议暴露给 Agent 自身，让它们能够“看到”并“谈论”彼此，而不仅仅是被动地在侧边栏展示。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [8] USER

这个mcp有名字吗

---

## [9] NLM

根据提供的来源，这个内置的 MCP（Model Context Protocol）系统在架构中被称为 “Teammate” 或 “Agent 控制面” (Agent Control Plane)
1
2
。
以下是关于其命名的具体细节：
统称：在描述环境嗅探和状态汇总时，来源将其称为 “Teammate/MCP” 体系
1
。
代码模块名：在后端架构中，该功能主要位于 src-tauri/src/teammate/ 目录下，直接以 teammate 命名
2
。
功能定义：它被定义为“Teammate / Agent 控制面”，负责处理智能体的软暂停、生命周期管理（Orchestration Health）以及安全审批（HITL）逻辑
2
。
简而言之，虽然它使用了 MCP 协议，但在 Ridge 项目内部，它的正式名称和功能标识符是 Teammate
2
。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [10] USER

我希望在下个迭代中，将这个mcp命名为人民公社(英文名)

---

## [11] NLM

好的，根据您的要求和当前的架构定义，我将在 Iteration 21 的规划中正式将现有的 Teammate / Agent 控制面 系统更名为 People's Commune（人民公社）。
以下是基于来源文档的详细更名与调整规划：
1. 命名更迭与概念对齐
当前名称：Teammate / Agent 控制面
1
。
新名称：People's Commune。
愿景映射：这一命名将进一步强化您提出的“编组协同”与“跨智能体协作”愿景。在“公社”概念下，智能体不再是独立的个体，而是编组内的成员，通过共享的 MCP 能力实现队友感知与任务分担。
2. 技术实施路径（Iteration 21）
由于更名涉及核心架构分层，将按照以下步骤执行：
代码仓库重构：
将后端目录 src-tauri/src/teammate/ 重命名为 src-tauri/src/commune/ 或 src-tauri/src/peoples_commune/
1
。
将核心 Rust 符号如 TeammateRegistry、orchestration_health 等映射为公社语境下的 CommuneRegistry 等
1
。
协议 SSOT 更新：
根据 “协议 SSOT 唯一” 原则，必须首先修改 ridge-cloud-protocol.md 中的能力宣告与方法名（例如将 get_teammate_topology 更新为 get_commune_topology）
2
。
同步更新 remoteAllowlist.ts 中的 TS 镜像，确保云端与桌面端的能力门控（Capability Gate）一致
2
。
UI 与交互层对齐：
侧边栏的 Agent Center 将正式更名为 Commune Center（公社中心）。
卡片布局中的“编组”将直接体现“公社成员”身份，竖条色块将作为公社内部不同协同小组的视觉标识。
3. 约束与审计 (§3, §6)
能力先协商：更名后的所有新 RPC 指令必须在 capability.rs::REMOTE_ALLOWLIST 中重新注册，防止公网手机端因版本偏斜导致指令被拒
2
。
减法审计：在更名过程中，将顺带清理掉您之前要求删除的“工作区目标（Workspace Goals）”相关代码，实现更简约的架构
1
3
。
4. 验收信号
代码事实：执行 grep -r "Teammate" src-tauri 结果应仅限于旧版本兼容性注释，核心逻辑全部替换为 Commune 或 People's Commune。
协议事实：ridge-cloud-protocol.md 权威契约完成更名修订。
运行事实：Iteration 21 启动后，侧边栏 Tab 标签显示为 Commune。
这一改动将作为 Iteration 21 “智能体控制中心重构”的主轴，旨在通过更具辨识度的命名体系，支撑您后续关于“智能体自行创建队友”与“跨类型协同”的高阶需求。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [12] USER

叫mcp叫agent's commune吧

---

## [13] NLM

好的，根据您的最新指令，我们将把内置的 MCP（即 Teammate / Agent 控制面）正式命名为 Agent's Commune（智能体公社）。
在当前架构中，这一系统主要负责智能体的生命周期管理、软暂停及安全审批（HITL）逻辑
1
。以下是基于现有源代码事实（ARCHITECTURE.md）的更名实施路径：
1. 后端目录与符号重构 (Gap ID: P1/M2)
物理更名：将后端核心目录 src-tauri/src/teammate/ 更名为 src-tauri/src/commune/
1
。
逻辑重构：
suspend.rs、orch_health.rs 和 hitl.rs 中的核心结构体将从 Teammate 体系迁移。例如，get_orchestration_health 提供的状态快照将属于 Commune 运行状态
1
。
Agent's Commune 将作为这些智能体协作的唯一合法实体。
2. 协议 SSOT 与能力门控 (Security Guard)
协议修订：根据“协议 SSOT 唯一”原则，必须首先修改 ridge-cloud-protocol.md（另仓源码），将所有 teammate 相关的方法名（如 get_teammate_topology）更新为公社语义
2
。
能力门放行：新的公社指令必须在 capability.rs::REMOTE_ALLOWLIST 中重新注册，并同步更新其 TS 镜像 remoteAllowlist.ts
2
。
教训吸收：吸取 v0.1.2 的教训，确保更名后的远程调用不仅仅加在路由表（CORE_MIGRATED_METHODS），还要正确进入能力门，以防止云控端（Cloud）调用被拒
2
3
。
3. UI 与交互逻辑对齐
侧边栏中心：src/routes/+page.svelte 中的 Agent Center 将更名为 Commune Center
4
。
编组协同：
智能体列表将按“公社编组”展示，利用竖向色条区分不同的小组
1
。
轻量化总结：Agent 将通过内置 MCP（现在称为 Commune API）共享自己的任务摘要（summary_snapshot），实现公社内的“队友感知”。
4. 架构不变量守卫
危险动作不可远达：即使更名为 Agent's Commune，关于公社成员的敏感操作（如 suspend_agent 或 resolve_hitl_remote 中的 modify）依然仅限桌面本地 IPC，严禁通过公网远达
1
2
。
E2EE 边界：公社成员之间的业务通信依然严格遵守端侧加密，中转服务器（Relay）不可读取明文
2
。
这一命名不仅是一个标签的更换，它将作为 Iteration 21 的核心，支撑起您要求的“跨智能体编组协同”与“自主创建队友”的功能愿景。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [14] USER

0.1.2版本的手机remote，终端尺寸和网页尺寸不匹配，这个问题在0.1.1迭代中首次出租。可能是改了和pty渲染尺寸重发相关问题

---

## [15] NLM

针对您反馈的 v0.1.2 手机端终端尺寸不匹配问题，结合《ARCHITECTURE.md》中的架构现状，该问题确实与 v0.1.1 引入的底层重构高度相关。
以下是基于源代码事实的深度分析：
1. 根因追踪：P4 手机保活与内核管理逻辑变更
在 v0.1.1 版本中，手机端进行了重大重构：弃用了「单例 kernel + 旁路缓存」，改为接入共享的 TerminalManager 并实现多 kernel 保活（Park）
1
。
重构点：现在的逻辑是“切 pane 不 wipe”，即多个终端实例在后台“停泊”。
潜在失效点：当您在手机端切换 Pane 或重新连接时，TerminalManager 需要确保当前 UI 容器的 CSS 尺寸（网页尺寸）与后端 PTY 的逻辑尺寸（行列数）同步。如果“尺寸重发”逻辑在内核“停泊”或“复活”过程中未能准确触发 resize RPC 命令，就会导致渲染错位。
2. PTY 渲染尺寸重发的关联证据
您提到的“PTY 渲染尺寸重发”在架构上有以下相关变动：
能力一致性协议：Resize 语义在桌面、LAN 和 Cloud 之间是保持一致的
2
。
服务腿收敛：v0.1.1 实现了 ridge_remote::pane 的 SSOT 收敛
3
，这意味着尺寸更新指令必须通过统一的传输管道。
Resume（续订）机制：v0.1.1 激活了无 RIS 的续订（Resume）机制
1
。如果在 Resume 过程中，系统错误地沿用了旧的尺寸快照，而没有根据当前手机浏览器的 Viewport 实时重测并下发 resize 命令，尺寸不匹配就会发生。
3. 为什么在 0.1.1/0.1.2 中爆发？
验证真空（R-VERIFY）：目前的架构审计显示，手机保活功能（P4）虽然代码级信心（svelte-check）已通过，但 “运行时真机验证”仍处于空白状态（R1–R8 证据待采集）
1
4
。
桌面与 Remote 的差异：由于桌面端使用 SplitContainer.svelte 进行复杂的 Pane 布局管理，而手机端通常是全屏或简单的 Tab 切换，TerminalManager 在共享给手机端使用时，可能未充分考虑到移动端浏览器 Viewport 变化（如地址栏隐藏/显示、横竖屏切换）与 resize 触发的竞态关系。
4. 建议修复方向（Iteration 21）
针对这一偏差，建议在下一迭代（Iteration 21）中落实以下技术方案：
激活 R-VERIFY：执行真机 Smoke 测试，通过 validate-remote-smoke-evidence.mjs 采集真机尺寸同步的物理证据
4
。
审计 Resize 触发链：检查 packages/remote/src/shared/terminal/manager.ts 中 resize 方法的调用时机。必须确保在 Pane 激活（Unpark） 和 WebRTC 重新连接成功（Resume） 后，强制执行一次基于当前容器 rect 的尺寸对齐。
消除“静默分叉”：确保 resize 能力在 capability.rs::REMOTE_ALLOWLIST 门控中被正确宣告和执行
2
，防止因版本偏斜或权限问题导致尺寸指令在云端被拦截。
总结：该问题是由于手机端从单例模式转向多内核保活模式时，尺寸同步逻辑在 TerminalManager 的生命周期钩子中出现了处理断层，且缺乏真机证据（R-VERIFY）的守护。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [16] USER

还有个多次要求修，但都没有修复的bug。桌面浏览器的分屏终端，内部shell渲染区域和pane外框差距太大。shell渲染区域始终没有填满pane区域，并且点击resize按钮也没有用

---

## [17] NLM

针对您提到的桌面浏览器分屏终端渲染区域无法填满 Pane 且 Resize 按钮失效的问题，结合最新的 v0.1.2 架构现状 与 Iteration 21 规划，以下是深层技术分析与修复路径：
1. 核心根因：Resize 指令在“能力门（Capability Gate）”中的丢失
在 v0.1.2 的架构审计中，发现了一个关键的**“能力先协商”**漏洞。
指令被拒：Ridge 强制执行 capability.rs::REMOTE_ALLOWLIST 校验。即使方法在路由表中，如果未在能力白名单中明确放行，cloudHostBridge 会直接拒绝执行并返回 METHOD_NOT_FOUND
1
。
Resize 失效：点击按钮无效通常是因为 Resize RPC 调用在经过 TerminalManager 路由到后端时，由于权限校验失败被“静默拦截”。这导致后端 PTY 的行列数（Rows/Cols）始终停留在初始化状态，无法随 UI 容器的像素尺寸动态调整
1
2
。
2. 渲染区域差距过大（Rendering Gap）的病灶
布局与语义脱节：目前 SplitContainer.svelte 和 RidgePane.svelte 负责 Pane 布局，但终端内部的渲染由 ridge-term（WASM）负责
2
。
SSOT 尚未收敛：源文档中明确指出，桌面版的 RidgePane 仍存在“同源病（Homologous Disease）”，即其初始化和状态恢复逻辑尚未完全收敛到最新的 get_pane_resync_frame 命令上（目前标记为 P2 级差距 R-DESKTOP-RESYNC）
3
4
。这导致前端渲染网格（Grid）在分屏时无法准确获取容器的实时边界，产生明显的白边或空隙。
3. 为什么“多次要求修”却没修好？
验证真空：之前的修复大多集中在 ridge-term 的增量渲染逻辑（如 WebGPU/Canvas 自动回退）上
2
。
路由表误区：开发者曾误以为将命令加入 CORE_MIGRATED_METHODS 路由表就完成了修复。但 v0.1.2 的教训证明，必须在 REMOTE_ALLOWLIST（Rust）和 remoteAllowlist.ts（TS）中双向钉死，命令才能真正触达 PTY 并反馈给渲染层
1
5
。
4. 下一迭代（Iteration 21）的确定性修复方案
为了彻底消灭这个 Bug，接下来的动作将不再仅仅是微调 UI，而是从协议和生命周期层面进行“硬手术”：
动作 A：Resize 能力全量放行（P0 级）
在 capability.rs::REMOTE_ALLOWLIST 中强制注册并放行 resize 指令。
同步更新 remoteAllowlist.ts，确保 isRemoteAllowed 校验在浏览器环境下不再拦截尺寸重发信号
1
。
动作 B：桌面首屏逻辑收敛（R-DESKTOP-RESYNC）
将桌面端的 RidgePane 初始化改为调用 get_pane_resync_frame。这个命令会由后端构建包含 RIS 指令和完整模式前导的“标准同步帧”，确保终端在一启动时就处于正确的 TUI 鼠标和尺寸感知状态
3
4
。
动作 C：强制尺寸重发（Resize Re-trigger）
在 TerminalManager 的生命周期钩子中增加容器溢出校验。一旦 ResizeObserver 检测到 Pane 像素变化，必须确保 head_seq 同步后立即下发 PTY Resize，防止渲染网格滞后
6
7
。
验收信号：
确定性判定：执行 cargo test 验证 resize 命令在白名单中；通过 remoteAllowlist.test.ts 确保 TS 镜像同步
4
5
。
交互验证：在分屏状态下随意拖拽 SplitContainer 边缘，终端字符区域应实时跟随压缩/扩张，不再产生残留的空白外框。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

## [18] USER

现在远端remote渲染退回了canvas2d，之前是支持webgpu的。我要继续以webgpu优先

---

## [19] NLM

针对您反馈的远端渲染退回到 Canvas2D 的问题，根据最新的《ARCHITECTURE.md》，系统架构明确规定了 WebGPU-first 的原则。以下是针对该现状的分析与优化建议：
1. 架构基准确认
根据现状文档，packages/ridge-term 的渲染策略被严格定义为：WebGPU-first + Canvas2D 自动回退
1
。
生产默认值：default=["webgpu"]
1
。
强制约束：文档明确标注“运行时 GPU 探测不得删除”，以确保在支持的环境下优先使用 WebGPU
1
。
2. 退回 Canvas2D 的可能根因分析
远端渲染回退通常是由自动回退机制触发的，可能的原因包括：
运行时探测失败：远端设备（如手机浏览器或特定的桌面浏览器环境）在执行 WebGPU 探测时未能通过硬件加速检查，导致系统自动切换到 Canvas2D 以保证界面可用
1
。
构建配置偏斜：虽然 default=["webgpu"] 是生产默认，但需检查 packages/ridge-term 在构建 WASM 时的体积优化（opt-z）是否意外干扰了 WebGPU 相关的绑定代码
2
。
缺乏物理证据（E1 差距）：目前 WebGPU 的真机收益测量属于用户轨 E1，且由于“连续约 10 轮无真机数据”，该部分可能存在被简化或清理的风险
1
3
。
3. 下一迭代（Iteration 21）的行动建议
为了确保 WebGPU 的优先地位，建议在下一迭代中执行以下操作：
强化 GPU 探测日志：在远端启动时，强制输出 GPU 探测的具体失败原因（如驱动不支持、内存限制等），而非静默回退。
激活 E1 证据采集：结合 R-VERIFY（P0 级手机保活真机验），在产生 evidence JSON 时同步记录远端的渲染后端状态
4
。
锁定构建门禁（R-TESTGATE）：在 CI 流程中加入对 ridge-term 的 WASM 符号检查，确保 webgpu 特性在构建产物中未被剥离
4
。
4. 减法逻辑预警
请注意，架构中提到如果 E1 WebGPU 遥测 长期无真机数据支持，会有“清冗余”的减法机会
3
。为了保留该特性，必须尽快在真机环境（iOS/Android）中产生运行证据，证明 WebGPU 的性能优势
1
4
。
如果您需要更详细的渲染层级调试说明或相关的性能基准报告，我可以为您创建一个定制化报告。
keep_pin
保存到笔记
copy_all
thumb_up
thumb_down

---

