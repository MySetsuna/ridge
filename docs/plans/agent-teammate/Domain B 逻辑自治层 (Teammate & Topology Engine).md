在那么多智能体（Agent）共存的 Ridge 物理工作区中，如果说 **Domain A（物理流传输与净化层）** 是疏通血管的管道工人，那么 **Domain B 逻辑自治层 (Teammate & Topology Engine)** 就是这个多智能体社会的“组织部”与“通信兵”。

它不关心 PTY 丢了多少个字节的回显，它只关心：**现在谁是老大（Team Leader）？谁精通 Rust？谁性格谨慎适合干 Code Review？它们打算怎么打配合？**

为了让物理层拦截到的 TML 信号和 Teammate API 呼叫真正转化为“有组织、有纪律”的团队协作，以下是 **Domain B** 的深度架构与设计规范。

## B1. Teammate Engine：智能体注册与画像管理

当一个外部 Agent（如 Claude Code、OpenCode 或本地运行的 Hermes）通过嵌入式 `tmux shim` 或直接调用 `teammate api` 注入到 Ridge 的某个 Pane（窗格）时，Teammate Engine 必须第一时间对其进行“人口普查”，建立动态画像。

### 1.1 智能体画像数据结构 (Agent Profile Scheme)

在 Rust Core 中，每一个活跃的 Agent 被抽象为一个 `Teammate` 实例：

Rust

```
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AgentRole {
    Leader,
    Worker,
    Observer,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentCapabilities {
    pub language_skills: HashMap<String, u8>, // 例如: {"Rust": 5, "TypeScript": 4} (1-5分)
    pub domain_skills: Vec<String>,          // 例如: ["Git", "Refactor", "UT-Generation", "Compile-Fix"]
    pub context_window: usize,               // 上下文窗口大小
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentPersonality {
    pub risk_tolerance: f32,  // 风险承受度: 0.0 (极度谨慎) ~ 1.0 (激进鲁莽)
    pub thoroughness: f32,    // 细致度: 0.0 (粗线条快速交付) ~ 1.0 (字斟句酌)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Teammate {
    pub id: String,                  // 唯一标识 (UUID 或 Pane_ID 衍生)
    pub name: String,                // 智能体代号 (如 "claude-code-01", "hermes-coder")
    pub pane_id: u32,                // 物理绑定的 Ridge Pane ID
    pub role: AgentRole,             // 当前团队角色
    pub capabilities: AgentCapabilities,
    pub personality: AgentPersonality,
    pub status: String,              // "Idle", "Working", "Disappeared"
}
```

### 1.2 动态发现与握手协议 (Handshake Protocol)

Agent 的生命周期与物理 Pane 强绑定。动态握手可以通过两种路径触发：

1. **被动识别**：`tmux shim` 拦截到特殊的启动环境变量（如 `CLAUDE_CODE=1`），通知 Rust Core：“检测到大牌路过，自动为该 Pane 创建 Worker 画像”。

2. **主动声明**：Agent 启动后，通过全局 CLI 工具 `ridge-cli teammate register` 向物理网关发送 JSON 声明。

## B2. Topology Engine：拓扑编排与角色竞选机制

多个能力、性格迥异的 Agent 挤在同一个 Workspace 里，必须要定下乾坤。Topology Engine 负责维护一张“物理空间-逻辑角色-任务依赖”的复合拓扑图。

### 2.1 Team Leader 竞选机制 (Leader Election)

当 Workspace 启动，或原 Leader 进程退出（Pane 关闭）时，触发自治竞选：

- **静态指定**：人类用户在工作区配置文件 `.ridge/workspace.json` 中明确指定了某个 Pane 的主 Agent 为 Leader。

- **动态自组织（权重算法）**：若无指定，Topology Engine 会向所有活跃 Teammate 发起全网广播，并根据其画像计算权重得分 $W$：

$$W = (\text{Language\_Skills}_{\text{avg}} \times 0.4) + (\text{Context\_Window\_Score} \times 0.4) + (\text{Thoroughness} \times 0.2)$$

> 💡 **设计潜台词**：上下文窗口大、综合编程能力强、且做事稳重的 Agent（比如 Claude 3.7 Sonnet 级别的 Coder）更容易在算法中胜出，被加冕为 **Team Leader**；而速度快、上下文小的轻量级模型则更倾向于被判定为执行特定子任务的 **Worker**。

### 2.2 拓扑图模型 (Workspace Topology Graph)

Ridge 内部维护一个全局状态单例 `TopologyGraph`，用于描述谁在对谁下命令：

Rust

```
use petgraph::graph::{DiGraph, NodeIndex}; // 使用 petgraph 库维护有向图

pub struct TaskEdge {
    pub instruction_id: String,
    pub description: String,
}

pub struct TopologyGraph {
    // 节点是 Teammate，边代表正在进行的协同控制/协作流
    pub graph: DiGraph<Teammate, TaskEdge>,
    pub leader_node: Option<NodeIndex>,
}
```

## B3. Teammate API 核心逻辑逻辑层路由

Teammate API 是逻辑层的“外交部”。它不走底层的字符流强灌，而是走结构化的 Tauri IPC Command 或本地高速 Local Loopback WebSockets。

### 3.1 核心 API 语义设计

| **API 方法名**                   | **调用者**     | **逻辑行为描述**                                                              |
| ----------------------------- | ----------- | ----------------------------------------------------------------------- |
| `teammate.get_team_profile()` | 任何 Agent    | 获取当前 Workspace 全体成员的“能力/性格/Pane ID”花名册。通常由 Leader 启动时调用，用以了解手下兵马。       |
| `teammate.delegate_task()`    | Team Leader | 向指定 Worker 派发任务。Topology Engine 会在图中连线，并**自动激活/聚焦**对应的物理 Pane。          |
| `teammate.broadcast()`        | 任何 Agent    | 全网求助。例如 Worker 遇到了编译死循环，向所有人大喊：“谁懂这个 C++ 模板链接错误？”，由其他懂该领域的 Worker 举手接单。 |
| `teammate.report_progress()`  | Worker      | 向 Leader 汇报战果或提交 Code Review 请求，触发状态机向下一阶段流转。                           |

### 3.2 跨智能体协同生命周期流转 (Sequence View)

```
[ 人类用户 ] -> 键入指令 "修复现有的内存泄漏并发布 Git Commit" 给 Leader Pane
      |
      v
[ Team Leader (Pane 1) ] 
      |---> 调用 `teammate.get_team_profile()` 发现 Pane 2 擅长 "Valgrind/C-Fix"
      |
      |---> 调用 `teammate.delegate_task(target: "Pane 2", task: "分析内存泄漏日志")`
      v
[ Ridge Topology Engine ]
      |---> 变更为物理联动：高亮闪烁 Pane 2，利用 Domain A 净化管道准备接收
      |---> 将指令通过 TML 或 API 优雅送达
      v
[ Expert Worker (Pane 2) ] -> 物理运行 Valgrind -> 得到结果
      |
      |---> 调用 `teammate.report_progress(to: "Pane 1", data: "发现第42行未 free")`
      v
[ Team Leader (Pane 1) ] -> 自行修改代码 -> 跑通测试 -> 呼叫物理 Git 插件自动提交
```

## B4. Rust Core 核心实现伪代码

为了在 Tauri 后端高效、线程安全地管理这套自治逻辑，我们需要利用 Tokio 的异步通道（Channels）与读写锁。

Rust

```
use tauri::{State, AppHandle};
use std::sync::Arc;
use tokio::sync::RwLock;
use dashmap::DashMap; // 高并发哈希表

pub struct WorkspaceState {
    pub teammates: DashMap<String, Teammate>,
    pub active_leader: Arc<RwLock<Option<String>>>,
}

// Tauri Command: 供前端界面渲染或者 Agent 通过 HTTP/WS 网关调用
#[tauri::command]
pub async fn handle_agent_delegate(
    from_pane: u32,
    to_pane: u32,
    task_payload: String,
    state: State<'_, WorkspaceState>
) -> Result<String, String> {
    // 1. 权限安全校验：调用者必须是 Leader，或者具备高权限
    let leader_id = state.active_leader.read().await;
    // (此处省略严格的身份比对逻辑...)

    // 2. 拓扑状态变更
    if let Some(mut worker) = state.teammates.get_mut(&format!("pane_{}", to_pane)) {
        worker.status = "Working".to_string();
    }

    // 3. 物理层击穿：将任务负载转换为标准输入，递交给 Domain A 的物理注入通道
    // 让 Worker 的 PTY 窗口里自动打印出提示词，促使其苏醒并开工
    crate::physical::inject_to_pane(to_pane, &task_payload).await?;

    Ok(format!("Task successfully routed to Pane {}", to_pane))
}
```

## 💡 本层设计的硬核闪光点

1. **性格介入分派（Personality-Driven Dispatch）**：
   
   Leader 拆解完任务后，如果发现当前要改动的是高风险的底层核心库，它在读取 Profile 后，会放弃派发给 `risk_tolerance: 0.9`（极度自信且野路子）的开源小模型，转而将任务指派给 `thoroughness: 0.95`（极其严谨）的 Claude Code，从而在逻辑层实现了**智能体工程学的因材施教**。

2. **人类的“上帝视角”看戏体验**：
   
   因为 Topology Engine 掌握了完整的有向任务图，Ridge 的 Svelte 5 前端完全可以渲染出一个**酷炫的动态连线图（Git Graph 旁的 Agent Graph）**。用户能亲眼看到 Pane 1 射出一道“委派光束”连接到 Pane 2，Pane 2 闪烁开工，交付后光束变绿回弹给 Pane 1。多智能体大乱斗从此变得井然有序。

接下来，你想先细化 **B1 中智能体之间具体的标准握手 JSON 协议格式**，还是直接去写 **Tauri 这一侧用来承载 Teammate 核心命令的通信网关**？
