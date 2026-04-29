**Ridge 架构文档 v2.0**  
**项目名称**：Ridge —— 现代 Warp-like + tmux-like 终端（Rust + Tauri）  
**版本**：v2.0（基于当前全部讨论总结）  
**目标**：打造一款**以终端为核心**、支持任意层级 split、轻量内置编辑器、Git Graph 可视化的高稳定终端工具。**不直接集成 AI**，保持纯粹终端特性。

---

### 1. 设计原则（核心约束）

- **终端为主、编辑器为次、Git 为再次**  
- **极致不易崩溃**：Rust 内存安全 + 进程/任务隔离 + 严格错误处理 + 状态持久化  
- **tmux-like**：支持无限层级 H/V Split、resize、zoom、快捷键  
- **Warp-like**：现代 UI、命令块感知、Command Palette、玻璃态效果  
- **性能优先**：Git Graph 使用 Canvas 渲染  
- **前后端严格分离**：前端崩溃不影响后端 PTY 进程

---

### 2. 高层架构图（文字版）

```
┌─────────────────────────────────────────────────────┐
│                Frontend (Svelte 5 + TS)             │
│  • SplitContainer (递归嵌套)                        │
│  • Pane (xterm.js + Monaco Editor 双模式)           │
│  • GitGraph (Canvas 渲染)                           │
│  • Activity Bar / Tab Bar / Command Palette         │
└───────────────────────▲─────────────────────────────┘
                        │ Tauri IPC (invoke / emit)
┌───────────────────────▼─────────────────────────────┐
│               Rust Backend (Tauri v2 + Tokio)       │
│                                                     │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────┐ │
│  │ TerminalEngine│   │ PaneTree     │   │ GitEngine│ │
│  │ (portable-pty)│   │ (递归树)     │   │ (git2)   │ │
│  └──────────────┘   └──────────────┘   └──────────┘ │
│                                                     │
│  ┌──────────────┐   ┌──────────────┐                 │
│  │ EditorEngine │   │ AppState     │                 │
│  │ (Monaco + FS)│   │ (RwLock全局) │                 │
│  └──────────────┘   └──────────────┘                 │
│                                                     │
│  Event Bus (mpsc → Tauri emit) + Watchdog           │
└─────────────────────────────────────────────────────┘
```

---

### 3. 技术栈 v2.0

| 层级           | 技术选型                                                     | 原因             |
| ------------ | -------------------------------------------------------- | -------------- |
| Frontend     | Svelte 5 (Runes) + TypeScript + Tailwind + shadcn-svelte | 轻量、高响应式        |
| Terminal     | xterm.js v5 + FitAddon                                   | 行业标准           |
| Editor       | Monaco Editor                                            | VSCode 同款，轻量嵌入 |
| Split Layout | svelte-splitpanes + interact.js                          | 支持嵌套拖拽         |
| Git Graph    | HTML5 Canvas (自定义渲染)                                     | 性能最好           |
| Backend      | Tauri v2 + Tokio + portable-pty                          | 当前最优 PTY 方案    |
| Git          | git2-rs                                                  | 高性能            |
| 持久化          | redb                                                     | 快速本地 KV        |
| 状态管理         | parking_lot::RwLock + Arc                                | 高并发低开销         |

---

### 4. 项目目录结构（推荐）

```bash
ridge/
├── src/                          # Svelte 前端
│   ├── lib/
│   │   ├── components/
│   │   │   ├── SplitContainer.svelte
│   │   │   ├── Pane.svelte
│   │   │   ├── GitGraph.svelte
│   │   │   └── CommandPalette.svelte
│   │   ├── stores/               # Svelte writable / runes stores
│   │   └── utils/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── lib.rs
│   │   ├── state.rs              # AppState 核心
│   │   ├── types.rs              # 共享结构体
│   │   ├── commands/
│   │   │   ├── terminal.rs       # 已完整提供
│   │   │   ├── pane.rs
│   │   │   ├── git.rs
│   │   │   └── editor.rs
│   │   ├── engine/
│   │   │   ├── pty.rs
│   │   │   ├── pane_tree.rs      # 递归 PaneTree
│   │   │   ├── git.rs
│   │   │   └── editor.rs
│   │   └── utils/
│   ├── Cargo.toml
│   └── tauri.conf.json
```

---

### 5. 核心数据结构（伪代码）

```rust
// types.rs
#[derive(Clone, Serialize, Deserialize)]
pub enum PaneMode {
    Terminal,
    Editor { file_path: Option<PathBuf>, language: String },
}

#[derive(Clone)]
pub struct Pane {
    id: Uuid,
    mode: PaneMode,
    pty_handle: Option<PtyHandle>,
    editor_state: Option<EditorState>,
}

pub enum PaneNode {
    Leaf(PaneId),
    HSplit { children: Vec<PaneNode>, ratios: Vec<f32> },
    VSplit { children: Vec<PaneNode>, ratios: Vec<f32> },
}

#[derive(Clone)]
pub struct AppState {
    pub terminals: Arc<RwLock<HashMap<Uuid, PtyHandle>>>,
    pub pane_tree: Arc<RwLock<PaneTree>>,
    pub git_repos: Arc<RwLock<HashMap<PathBuf, Repository>>>,
    pub event_tx: mpsc::Sender<GlobalEvent>,
}
```

---

### 6. 关键模块详解（带伪代码）

#### 6.1 Terminal 实现（portable-pty）

```rust
// commands/terminal.rs
#[tauri::command]
async fn create_pane(state, pane_id, shell, cwd) {
    let pair = pty_system.openpty(size)?;
    let child = pair.slave.spawn_command(cmd)?;
    let writer = pair.master.take_writer()?;
    let reader = pair.master.take_reader()?;

    state.terminals.write().insert(pane_id, PtyHandle { writer, _child: child });

    spawn_pty_reader(state.clone(), pane_id, reader);   // tokio::spawn 异步循环
}

async fn spawn_pty_reader(state, pane_id, reader) {
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        state.event_tx.send(PtyOutput { pane_id, data: String::from_utf8_lossy(...) }).await;
    }
}
```

**前端通信**：

- `term.onData(data => invoke('write_to_pty', {pane_id, data}))`
- `listen(`pty-output-${pane_id}`, e => term.write(e.payload.data))`

#### 6.2 PaneTree 状态机（伪代码）

```rust
pub struct PaneTree {
    root: PaneNode,
}

impl PaneTree {
    pub fn split(&mut self, pane_id: Uuid, direction: SplitDirection) -> Result<Uuid> {
        // 找到目标 Leaf → 替换为 HSplit/VSplit，新增两个 Leaf
    }

    pub fn resize(&mut self, pane_id: Uuid, ratio: f32) { ... }
    pub fn close(&mut self, pane_id: Uuid) { ... }
    pub fn get_pane_mut(&mut self, id: Uuid) -> Option<&mut Pane> { ... }
}
```

#### 6.3 Git Graph（Canvas 渲染）

```rust
// Rust
#[tauri::command]
fn get_git_graph(repo_path) -> Vec<CommitNode> {
    let repo = git2::Repository::open(repo_path)?;
    // 拓扑排序 + lane 分配 → 返回扁平 CommitNode 列表
}

// Svelte Canvas
function drawGraph(commits) {
    ctx.clearRect();
    commits.forEach(c => {
        drawDot(c.x, c.y);
        c.parents.forEach(p => drawBezierLine(c, commits[p]));
        drawText(c.message);
    });
}
```

#### 6.4 Editor 双模式切换

同一 `Pane` 组件内：

- `currentMode: 'terminal' | 'editor'`
- 切换时清理旧实例 → 创建新实例（PTY 后台持续运行）

---

### 7. UI 布局 v2.0

- **左侧**：Activity Bar (Terminal / Git / Explorer)
- **侧边栏**：Git Graph (Canvas) + 文件树
- **主区域**：
  - Tab Bar（Session 层）
  - SplitContainer（递归渲染 Pane）
- **每个 Pane**：右键菜单（Split H/V、Toggle Editor、Close、Zoom）
- **顶部**：Command Palette (Cmd+K)
- **底部**：Status Bar

---

### 8. 防崩溃 & 可靠性策略（v2.0 强化）

1. 每个 Pane 独立 PTY + 独立 tokio task
2. 所有操作返回 `Result` + `AppError`，禁止 `unwrap()`
3. redb 持久化 PaneTree 与打开文件（启动秒恢复）
4. Watchdog task 监控 PTY 健康状态
5. 前端崩溃后自动重建 xterm/Monaco 实例，后端 PTY 继续运行
6. 资源限制（scrollback、buffer size）

---

### 9. 当前实现状态 & 下一步（v2.0）

**已完成**：

- Cargo.toml + tauri.conf.json（修正版）
- AppState 骨架
- commands/terminal.rs 完整实现（create/write/resize/kill）
- Pane.svelte 双模式（xterm + Monaco）
- SplitContainer 递归结构
- Git Graph Canvas PoC

**待完成优先级**：

1. `engine/pane_tree.rs` 完整实现
2. PaneTree 与 PTY Handle 的关联管理
3. 右键菜单 + 快捷键系统
4. redb 持久化
5. Git Graph 交互（点击跳转）

---

**文档结束**

这份 v2.0 架构文档已完整整合我们所有讨论内容，可作为后续开发的**蓝图和参考**。

**下一步行动**：  
你希望我现在开始写哪一部分的**完整可运行代码**？  

- `engine/pane_tree.rs` + 关联逻辑  
- 完整的 `Pane.svelte`（含右键菜单）  
- redb 持久化模块  
- 或者其他模块？

请告诉我，我们继续迭代到 v2.1！🚀
