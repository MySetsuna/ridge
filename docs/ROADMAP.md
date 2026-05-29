# Ridge — 产品路线图

> **产品定位**：AI Development Control Plane / Multi-Agent Development Workspace
> **核心价值**：让 AI coding 变得"可控、可见、可接管"

---

## 产品核心架构

```text
Workspace
 ├ Agents           ← 我们在这里
 ├ File Context
 ├ Timeline
 ├ Human Control Layer
 ├ Tasks
 └ Runtime State
```

> "Pane" 以后只是 UI 容器，"Agent Runtime" 才是产品核心。

---

## 五阶段总览

| Phase | 名称 | 版本 | 状态 |
|-------|------|------|------|
| 1 | Terminal Orchestration | v0.0.1–v0.0.2 | ✅ 已完成 |
| 2 | Agent Awareness Layer | v0.0.3–v0.0.5 | 🚧 进行中 |
| 3 | Human Control Layer | v0.1.0–v0.2.0 | 📋 规划中 |
| 4 | Workspace Intelligence | v0.3.0–v0.4.0 | 🔭 远期 |
| 5 | Multi-Agent Coordination | v0.5.0+ | 🌌 探索 |

---

## Phase 1: Terminal Orchestration（已完成）

### 当前状态

已经具备：
- 多 pane 分屏（递归嵌套，无深度限制）
- 终端嵌入（xterm.js + wasm ridge-term）
- Claude Code / OpenCode 集成（tmux shim + HTTP teammate server）
- 工作区切换与保持（keep-alive 多 workspace）
- 文件编辑与 Monaco 集成
- Git SCM 面板 + 图谱
- 搜索面板（跨工作区并发搜索）
- 全局状态/插件系统
- Claude Code teammate 路由（split-window / register-agent / release-pane）
- 文件系统 watcher + Git watcher
- WebGPU 渲染后端（实验性）

---

## Phase 2: Agent Awareness Layer — v0.0.5（当前目标）

### 核心目标

让系统知道：**agent 是谁、在做什么、改了哪些文件、当前状态是什么**

### 5 个实施任务

| # | 任务 | 说明 | 预估文件数 |
|---|------|------|-----------|
| 1 | Agent Metadata 系统 | PaneState 升级为完整结构化元数据 | ~8 |
| 2 | Modified Files 追踪 | 实时追踪 agent 修改的文件 | ~5 |
| 3 | Activity Timeline | 记录 agent 行为历史时间线 | ~3 |
| 4 | Human Control v1 | Pause / Resume agent | ~4 |
| 5 | Agent 全景观测面板 | 全局侧边栏展示所有 agent | ~2 |

### 执行顺序

```
Task 1 ──→ Task 2 ──→ Task 3
                     │
              Task 5 ←┘
                     
Task 4 (依赖 Task 1 状态定义，独立执行)
```

### 详细设计

详见 [`docs/v0.0.5-AGENT-AWARENESS.md`](./v0.0.5-AGENT-AWARENESS.md)

---

## Phase 3: Human Control Layer（规划中）

> 预计版本：v0.1.0–v0.2.0

### 核心思想

AI 不是全自动——而是 **Human-supervised Autonomy**（人类监督下的自主）

### 核心功能

#### 1. Human Takeover（人类接管）

当以下情况发生时，系统要求人类审查：
- AI 出错或陷入循环
- diff 过大（超过阈值）
- 修改涉及敏感文件（配置文件、密钥、数据库 schema）

**交互流程**：
1. 系统暂停 agent
2. 自动打开受影响文件
3. 自动定位 diff
4. 显示审查请求面板（含 Approve / Reject / Edit）

#### 2. Approval Gate（审批门禁）

AI 在以下操作前需获得人类批准：
- 删除文件
- 重构目录结构
- 执行危险命令（rm -rf、git push --force 等）
- 修改超过阈值行数的代码

这是 AI Governance Layer 的雏形——让 AI coding 不再是"黑盒提交"。

#### 3. Rollback System（回滚系统）

支持：
- `revert agent changes` — 回滚指定 agent 的全部改动
- `revert specific step` — 回滚到某个时间点
- 基于 git 的自动 checkpoint

#### 4. Agent Pause / Resume（生产级）

- pause 时 SIGSTOP 进程
- resume 时 SIGCONT + 上下文提示
- pause 期间用户可手动修改，resume 时告知 agent 变更

### 成功标准

1. 用户可以在 agent 出错时一键暂停并接管
2. 危险操作需要人类确认才能执行
3. 可以回滚到任意历史状态
4. 多 agent 环境下用户能控制单个 agent

---

## Phase 4: Workspace Intelligence（远期）

> 预计版本：v0.3.0–v0.4.0

### 核心问题

当前 AI coding 最大问题之一：**上下文持续丢失**

### 解决方案

#### Workspace-level Memory（工作区级别记忆）

不再依赖 chat-level 记忆。Workspace 应该记住：

```text
Current Goal        当前目标
Architecture        架构决策
Constraints         约束条件
Tech Stack          技术栈
Active Tasks        活跃任务
Recent Decisions    近期决策
```

#### 能力

- Agent 重启后能恢复工作上下文
- 切换模型（Claude ↔ GPT ↔ 本地模型）时上下文不丢失
- 多 agent 协作时共享项目知识

#### 技术实现

- 持久化知识库（每 workspace）
- 语义检索（RAG over workspace context）
- Agent 自动写入/读取工作区记忆

### 成功标准

1. 关闭工作区再打开，agent 知道刚才在做什么
2. 切换 AI 模型后无需从头解释上下文
3. 多个 agent 共享同一组项目约束

---

## Phase 5: Multi-Agent Coordination（探索期）

> 预计版本：v0.5.0+

### 核心能力

#### 1. Task Graph（任务图）

```text
Auth System
 ├ UI Agent          ← 前端实现
 ├ Backend Agent     ← API 实现
 └ Testing Agent     ← 测试编写
```

#### 2. Agent Dependencies（agent 依赖）

```text
Wait Backend API Done → 通知 UI Agent 开始集成
Wait UI Agent Done    → 通知 Testing Agent 开始 E2E
```

#### 3. Shared Context（共享上下文）

多个 agent 共享：
- Architecture decisions
- API contracts
- Project goals & constraints
- Current state of the workspace

### 探索方向

- **编排器模式**：一个 orchestrator agent 协调多个 worker agent
- **市场模式**：空闲 agent 自动领取任务队列中的工作
- **管道模式**：任务按流水线在不同 agent 间传递

### 成功标准

1. 能自动将大型任务分解给多个 agent 并行执行
2. Task Graph 可视化（谁在做什么、依赖什么）
3. Agent 间上下文无断裂传递

---

## 绝对不做的事

| ❌ 不做 | 原因 |
|---------|------|
| 万能 AI IDE | 太大，容易失焦 |
| 重新训练模型 | 资源密集，与核心价值无关 |
| 疯狂加聊天 | 聊天窗口是 commodity |
| UI 打磨地狱 | 够用即可，核心在 agent runtime |

---

## 真正的产品护城河

1. **Agent Runtime Visibility** — 知道 agent 在做什么
2. **Human Override UX** — 能随时接管控制权
3. **Workspace Persistence** — 上下文不丢失
4. **AI Workflow Management** — 编排多个 AI 开发者

这些是 model API 和 terminal 无法替代的。
