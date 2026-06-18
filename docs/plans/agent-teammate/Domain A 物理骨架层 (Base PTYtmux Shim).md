# 🛠️ Ridge 技术规范说明书：Domain A —— 物理骨架层 (Base PTY & Shim Layer)

本规范定义了 Ridge 在**裸终端环境**（即不依赖 MCP 协议的传统 CLI 环境）下，通过物理流量拦截、虚拟输入注入与终端流净化，实现多智能体协同的基本盘物理骨架层。

---

## A1. `tmux-shim` 流量拦截与原语扩展规范

### 1.1 物理注入与环境伪装机制

当工作区（Workspace）启动时，Ridge Core 会为当前工作区环境注入一个定制的 `PATH` 优先级，并导出关键环境变量。

* **二进制伪装**：编译生成一个高度轻量化的 Rust 独立可执行二进制文件 `ridge-tmux-shim`，在注入环境的 `PATH` 中重命名为 `tmux`。

* **环境变量导出**：
  
  ```bash
  export PATH="/path/to/ridge/shims:$PATH"
  export TMUX="/tmp/ridge_tmux_${WORKSPACE_ID}.sock,${PID},0"
  export RIDGE_WORKSPACE_ID="ws_2026_x91"
  export RIDGE_CURRENT_PANE_ID="pane_01"
  ```

```
### 1.2 通信管道设计 (IPC Emulation)

`tmux-shim` 与 `Ridge Core` (Tauri/Rust) 之间采用本地 Unix Domain Socket (UDS) 建立非阻塞的 JSON-RPC 2.0 管道。

* **Socket 路径**：`/tmp/ridge_ipc_{WORKSPACE_ID}.sock`
* **协议封装**：

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct ShimIpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ShimIpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<IpcError>,
}
```

### 1.3 关键命令拦截与仿真状态映射表

当 Agent（如 Claude Code）在物理终端中执行 `tmux <command>` 时，`tmux-shim` 必须完美仿真标准 tmux 的 stdout/stderr 输出，并向 Ridge Core 同步逻辑状态。

| 拦截的 tmux 命令       | Shim 内部转换方法                 | 映射的 Ridge 核心语义 | 给 Agent 的仿真返回 (Stdout) |
| ----------------- | --------------------------- | -------------- | ---------------------- |
| `split-window -h` | `Method: "pane.split"` <br> |                |                        |

<br>`Params: { direction: "Horizontal" }` | 物理屏幕右侧切分出新 PTY | `ws_2026_x91:0.%1` (符合 tmux 窗格编号命名规约) |
| `split-window -v` | `Method: "pane.split"` <br>

<br>`Params: { direction: "Vertical" }` | 物理屏幕下方切分出新 PTY | `ws_2026_x91:0.%2` |
| `list-panes` | `Method: "pane.list"` | 动态返回当前工作区所有活跃 Pane 状态 | `%1: [80x24] [layout ...] [active]\n%2: [80x24] [layout ...]` |
| `send-keys -t %2 "ls -la" C-m` | `Method: "pane.inject_input"` <br>

<br>`Params: { target: "pane_02", keys: "ls -la\n" }` | 跨窗格输入流强灌。若触发 TML 则升级为协同流 | *无输出* (Exit code: 0) |

---

## A2. PTY 文本标记语言 (Teammate Markup Language - TML) 规范

当 Agent 之间无 MCP 协议可用时，它们通过向彼此的 PTY 写入结构化文本进行纯文本社交。Ridge 物理层必须能够完美捕获、解析并路由这些“在野”协同流。

### 2.1 TML 语法格式 BNF 定义

```text
<tml-block>      ::= <tml-start> <tml-header> <tml-body> <tml-end>
<tml-start>      ::= "@@RIDGE_TML_START@@\n"
<tml-end>        ::= "@@RIDGE_TML_END@@\n"
<tml-header>     ::= <json-metadata> "\n"
<json-metadata>  ::= "{" <kv-pairs> "}"
<tml-body>       ::= <text-content>
```

### 2.2 TML 报文头部结构与内联动作设计

TML 头部是一个严苛的 JSON 结构，用于描述路由和动作语义。

```rust
#[derive(Serialize, Deserialize, Debug)]
pub struct TmlHeader {
    pub version: String,          // 固定为 "1.0"
    pub msg_id: String,           // UUIDv4 区分会话
    pub from_pane: String,        // 发起方 Pane ID
    pub to_pane: String,          // 目标方 Pane ID
    pub action: TmlAction,        // 内联控制动作
    pub task_id: Option<String>,  // 级联任务链 ID
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type", content = "payload")]
pub enum TmlAction {
    PeerTalk,                    // 纯文本多模态对话/闲聊
    AssignTask {                  // 派发一个具体物理子任务
        objective: String, 
        max_steps: u32 
    },
    YieldControl {                // 权限转交（例如：老大执行挂起，小弟获取输入焦点）
        reason: String 
    },
    ReportStatus {                // 子智能体向主智能体反馈执行阶段性结果
        status: String, 
        exit_code: i32 
    },
}
```

### 2.3 物理 PTY 拦截流状态机 (Parser State Machine)

Ridge Core 为每个 Pane 的 PTY 输出流挂载一个轻量级字节状态机，用于非阻塞解析 TML。

```
       +------------------ [ 匹配到 @@RIDGE_TML_START@@ ] ------------------+
       |                                                                  |
       v                                                                  |
  [ STATE: IDLE ] <---+                                                   |
       |              |                                                   |
(将字节向下透传       |                                                   |
 给 UI xterm.js)      +--- [ 解析失败/超时 ] ---+                         |
       ^                                        |                         v
       |                                  [ STATE: PARSING_HEADER ] -> [ 提取 JSON 边界 ]
 [ 完成路由/净化 ]                               |                         |
       ^                                        |                         v
       |                                        +--- [ 解析成功 ] --> [ STATE: READING_BODY ]
 [ STATE: EMITTING ] <---- [ 匹配到 @@RIDGE_TML_END@@ ] -------------------------+
```

---

## A3. 伪终端物理流输出净化过滤器 (PTY Stream Cleaner)

多智能体自主协同的最大痛点在于：Agent 在物理 Pane 频繁互灌数据时，会产生大量的打字回显噪音、控制字符串流、以及高频 TML 原始文本。这会导致前端渲染（xterm.js）极度卡顿，且严重毁坏人类用户的看戏视角（噪点拉满）。

### 3.1 零拷贝多段环形缓冲区 (RingBuffer Pipeline)

Stream Cleaner 直接插入在 `PTY Output Read` 与 `Tauri Front-end Event Emit` 之间。

* **架构设计**：采用基于 Rust `tokio::sync::mpsc` 的管道。
* **数据流向**：`PTY stdout` -> `StreamCleaner` (字节过滤) -> `UI Render Event`。

### 3.2 过滤净化策略与规则引擎

物理层实现三档过滤策略（可通过前端大盘动态配置）：

1. **`MUTATION_HIDE` (协同隐藏级)**：
   一旦状态机切入 `STATE: PARSING_HEADER` 到 `STATE: READING_BODY` 阶段，该区间内的所有原始字节**禁止**投递给前端 UI。整个智能体社交通信过程在前端“静默发生”，仅通过 UI 拓扑连线闪烁表达。
2. **`ANSI_REDUCE` (控制符瘦身级)**：
   高频物理操作（如快速 `cat` 探测大文件、`grep` 高亮）会产生大量光标移动转义序列（如 `\x1b[K`, `\x1b[H`）。Cleaner 会合并连续的重绘指令，降低 70% 的前端微操作重绘开销。
3. **`ECHO_SUPPRESSION` (回显抑制器)**：
   当前 Pane 被标记为 `Agent Autonomous`（自治状态）时，物理输入产生的字符回显（Stdout 对 Stdin 的 Echo）会被 Cleaner 动态对齐。通过维护滑窗哈希，若 Stdout 产出字节与最近一次 Stdin 强灌的字节高概率重合，且属于“打字机式”逐字吐墨，则予以丢弃，直到该命令触发真正的 `\r\n` 输出。

### 3.3 核心核心清洗核心：`StreamCleaner` 核心 Rust 实现 Specs

```rust
pub struct StreamCleaner {
    state: ParserState,
    header_buffer: Vec<u8>,
    body_buffer: Vec<u8>,
    workspace_id: String,
    pane_id: String,
}

impl StreamCleaner {
    pub fn new(workspace_id: &str, pane_id: &str) -> Self {
        Self {
            state: ParserState::Idle,
            header_buffer: Vec::with_capacity(1024),
            body_buffer: Vec::with_capacity(8192),
            workspace_id: workspace_id.to_string(),
            pane_id: pane_id.to_string(),
        }
    }

    /// 核心入口：流入原始 PTY 字节，流出“可透传给前端 UI 展示”的物理干净字节
    pub fn clean_stream(&mut self, raw_bytes: &[u8]) -> Vec<u8> {
        let mut ui_visible_bytes = Vec::with_capacity(raw_bytes.len());

        // 采用零拷贝或单次扫描指针
        let mut cursor = 0;
        while cursor < raw_bytes.len() {
            match self.state {
                ParserState::Idle => {
                    // 检查是否命中 TML 开始标记
                    if let Some(offset) = self.find_subsequence(&raw_bytes[cursor..], b"@@RIDGE_TML_START@@\n") {
                        // 将标记之前的字节全部保留送给 UI
                        ui_visible_bytes.extend_from_slice(&raw_bytes[cursor..cursor + offset]);
                        cursor += offset + b"@@RIDGE_TML_START@@\n".len();
                        self.state = ParserState::ParsingHeader;
                    } else {
                        // 未命中，当前剩余全量透传给 UI
                        ui_visible_bytes.extend_from_slice(&raw_bytes[cursor..]);
                        break;
                    }
                }
                ParserState::ParsingHeader => {
                    // 寻找换行符，边界锁定 JSON
                    if let Some(nl_idx) = raw_bytes[cursor..].iter().position(|&b| b == b'\n') {
                        self.header_buffer.extend_from_slice(&raw_bytes[cursor..cursor + nl_idx]);
                        cursor += nl_idx + 1;

                        // 尝试解析 TML 头部
                        if let Ok(header) = serde_json::from_slice::<TmlHeader>(&self.header_buffer) {
                            self.state = ParserState::ReadingBody;
                        } else {
                            // 解析失败，退化降级：认为此段数据是垃圾杂质，回吐原始数据，防止挂死
                            ui_visible_bytes.extend_from_slice(b"@@RIDGE_TML_START@@\n");
                            ui_visible_bytes.extend_from_slice(&self.header_buffer);
                            ui_visible_bytes.push(b'\n');
                            self.header_buffer.clear();
                            self.state = ParserState::Idle;
                        }
                    } else {
                        // 这一批字节里还没出现换行符，暂存进 Header 缓存
                        self.header_buffer.extend_from_slice(&raw_bytes[cursor..]);
                        break;
                    }
                }
                ParserState::ReadingBody => {
                    // 寻找 TML 结束标记
                    if let Some(offset) = self.find_subsequence(&raw_bytes[cursor..], b"@@RIDGE_TML_END@@\n") {
                        self.body_buffer.extend_from_slice(&raw_bytes[cursor..cursor + offset]);
                        cursor += offset + b"@@RIDGE_TML_END@@\n".len();

                        // 【物理骨架层向上核心跃迁】
                        // 数据体读取完毕，将其打包并分流发布至 Domain B 团队逻辑自治引擎总线
                        self.dispatch_to_topology_bus();

                        // 彻底清空临时缓冲，重置状态机
                        self.header_buffer.clear();
                        self.body_buffer.clear();
                        self.state = ParserState::Idle;
                    } else {
                        // 暂未读到结束标记，将当前全量吞入 Body 缓存（对前端 UI 保持绝对屏蔽隐形）
                        self.body_buffer.extend_from_slice(&raw_bytes[cursor..]);
                        break;
                    }
                }
            }
        }
        ui_visible_bytes
    }

    fn find_subsequence(&self, haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    fn dispatch_to_topology_bus(&self) {
        // 通过 Tauri 核心线程内部多生产者单消费者通道 (mpsc) 投递给拓扑网路
        // 触发 Domain B1/B2 判定逻辑。该实现在后续规范中详述。
    }
}

enum ParserState {
    Idle,
    ParsingHeader,
    ReadingBody,
}
```

---

本 Domain A Specs 奠定了系统在不具备现代通信框架下的高内聚物理连接。