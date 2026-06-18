这是让整个系统从“疯狂的脱缰野马”变成“可控的工业级数字员工”的最后一块拼图。

**Domain D (宿主交互层)** 的核心哲学是：**AI 可以自治，但人类拥有绝对的上帝视角与最高否决权。** 依托于 Ridge 原本就非常强大的 Tauri v2 和 Svelte 5 前端架构 ，我们将抽象的数据流转化为肉眼可见的连线，并将高危操作拦截在终端沙箱之外。

以下是 Domain D 的详细技术规范说明书（Specs）。

---

# 🛠️ Ridge 技术规范说明书：Domain D —— 宿主交互层 (UI/UX & Human-In-The-Loop)

## D1. 动态团队数据流向大盘 (UI Collaboration Graph)

在多智能体高速协作时，传统的纯文本终端会让人类用户感到“失控”——你不知道哪行代码是谁写的，也不知道是谁在给谁发消息。D1 模块在 `rg-split` 布局引擎之上叠加了一层可视化的监控视图。

### 1.1 状态驱动的 Svelte 5 浮层渲染

利用 Svelte 5 的响应式 Store，监听来自 Rust `ridge-core` 的拓扑状态更新。

* **Pane Header 状态微光**：为每个 Pane 的 Header 增加状态边框呼吸灯。
* `Thinking`：缓慢的脉冲呼吸效果。
* `Executing`：高频闪烁。
* `Idle`：静默。


* **通信连线 (Laser Beams)**：在绝对定位的 `div` 遮罩层中，使用 SVG 或 Canvas 动态绘制 Pane 之间的贝塞尔曲线。当 Pane 1 向 Pane 2 发送 MCP 消息或 TML 强灌文本时，触发一条带有光效的粒子流动画从 Pane 1 飞向 Pane 2。

### 1.2 Agent Center 侧边栏的 DAG 树

在原有的 SCM (Git) 侧边栏旁边，新增 Agent 专属视图 ：

* **当前目标 (Current Objective)**：展示 Team Leader 正在拆解的顶层任务。
* 
**团队花名册 (Roster)**：实时显示当前被 `Teammate Server` 纳管的所有 Agent 实例 。


* **审计日志 (Audit Trail)**：以类似聊天气泡的形式，降维展示智能体之间的底层 Tool Calls（将晦涩的 JSON 转换为诸如 *“Claude 召唤了 Hermes 并下发了测试任务”* 的人类可读文本）。

---

## D2. 工业级人类中间审批流规范 (Human-in-the-Loop Gateway)

这是整个 Domain D 的安全核心。当系统处于自治状态时，我们不能允许 Agent 随意执行 `rm -rf /` 或直接推送代码到线上环境。必须建立一套**分级拦截网关**。

### 2.1 高危指令的权限分级 (RBAC for Agents)

在 Rust 后端拦截所有的 PTY 输入流和 MCP Tool Calls，并对其进行模式匹配分级：

* **Level 0 (白名单)**：单纯的读取操作（`ls`, `cat`, `git status`, 读取 MCP Resources）。直接放行。
* **Level 1 (写沙箱)**：修改当前工作区内的代码文件。默认放行，但在 UI 上记录审计日志。
* **Level 2 (高危越界)**：删除大批量文件、安装未知系统依赖、执行提权脚本（`sudo`）、推送远端（`git push`）。**强制挂起，触发人类审批。**

### 2.2 Tauri 异步挂起与前端弹窗流 (Async Suspend & Resume)

当 `Teammate Server` 拦截到 Level 2 操作时：

1. **后端挂起**：Rust 端的执行线程（Tokio task）被挂起（利用 `tokio::sync::oneshot` channel 等待前端信号）。
2. **事件派发**：Tauri 触发全局事件 `teammate://hitl-approval-required`，携带上下文字段（发起者、目标动作、风险分析）。
3. **前端接管**：Svelte 5 立即弹出一个全屏的高优先级模态框（Modal）。
4. **人类仲裁决策**：
* **Approve (批准)**：前端向 Rust 发送确认信号，Rust 释放 Channel 阻塞，Agent 原始指令继续执行。
* **Reject (拒绝)**：前端发送拒绝信号。Rust 直接向 Agent 返回模拟的报错输出：`Error: Execution blocked by user authorization policy.`，迫使 Agent 重新思考。
* **Modify (修改并执行)**：人类发现 Agent 命令写错了（比如参数漏了），直接在输入框修改后提交，Rust 强行替换原指令并执行。



---

## D3. 异常级联熔断与冲突解决策略 (Circuit Breaker & Conflict Specs)

当两个 Agent 发生冲突，或者某个 Agent 陷入逻辑死循环时，系统必须能够自动熔断，防止消耗巨量 Token 算力或搞崩本地环境。

### 3.1 循环反馈熔断器 (Infinite Loop Breaker)

* **特征检测**：如果一个 Worker 在同一个文件中连续 3 次执行 `修改代码 -> 跑单测 -> 报错`，并且报错特征高度相似。
* **熔断执行**：`ridge-core` 触发硬件级中断（发送 `SIGINT / Ctrl+C` 到该 PTY），强制终止该 Worker 的当前操作。
* **上抛异常**：向 Team Leader 发送最高优先级的 MCP Notification：“*Worker_2 陷入逻辑死锁，已强制熔断，请接管或重新分派。*”

### 3.2 文件并发锁与冲突仲裁 (Concurrency Write Lock)

得益于 `ridge` 底层集成的 `notify` 文件监听 crate ：

* 当 Agent 尝试通过 MCP 写入文件，或在终端使用编辑器（如 `sed` / `echo`）时，`ridge` 会在内存中为该文件路径申请毫秒级的微锁。
* 如果 Pane 1 和 Pane 2 试图同时修改 `src/main.rs` 的不同函数，发生并发争抢，系统会拦截后手操作。
* 系统自动将后手操作转换为人类可读的 **Conflict 视图**（复用你现有的 Monaco Diff Editor Modal ），暂停所有 Agent 动作，提示人类用户：“*团队协作产生文件冲突，请您手动裁决合并。*”



---

为了更直观地展示 **D2 中最核心的“人类中间审批流（HITL）”** 是如何拦截并在前端表现的，我为你准备了一个交互式的模拟视图。你可以亲自体验拦截高危指令、拒绝或修改指令时的系统状态变化。