# ridge-cli 使用文档

> 语言约定：散文简体中文，所有命令/标志/字段/代码块保持英文原文（与代码库一致）。
> 本文件基于 `packages/ridge-cli/` 源码、`packages/ridge-core/src/` 及
> `docs/plans/s1-migration-ledger.md` 实证撰写——仅记录代码中实际存在的行为，
> 不文档化任何规划中功能。

---

## 概述

`rdg` 是 Ridge IDE 的**无头远控 host**，设计用于无图形界面的 Linux / VPS 环境。
它的核心价值是：**无需安装 Tauri / WebView / GPU 驱动**，即可把一台服务器变成可被
Ridge 控制端（浏览器或桌面 app）远程控制的终端主机。

### 与统一远控架构的关系

`rdg` 是统一远控架构（`docs/plans/unified-remote-architecture-handoff-final.md`）
的 headless host 实现（子项 **S5**）。架构决策 **D4** 要求 headless host 与桌面 app host
共用同一个**运行时无关** Rust crate（`packages/ridge-core`），而 `rdg` 正是这条
决策的第一个实现载体。

```
controller 浏览器
    └── WebRTC + E2EE DataChannel
           └── ridge-cli (headless host)
                  └── ridge_core::dispatch(method, args, ctx)
                         └── 与桌面端完全相同的 fs/search 引擎
```

**重要**：`rdg` 不依赖 Tauri（`cargo tree -p ridge-cli` 无 `tauri`）。
当前实现（S5 切片）只暴露 PTY 桥 + 文件只读命令（搜索 / 目录树），
完整 IDE 命令面（git / 写文件 / 工作区 / 分屏）尚未迁移——详见"已迁移 vs 未迁移"一节。

---

## 构建与安装

### 构建

```bash
# 从仓库根，仅构建 ridge-cli（发布模式）
cargo build --release -p ridge-cli

# 产物路径
# Linux / macOS: target/release/rdg
# Windows:       target\release\rdg.exe
```

**特性标志**：

| 特性 | 默认 | 说明 |
|------|------|------|
| `rtc` | 开启 | 真实 WebRTC host peer（`webrtc` crate）。关闭后 RTC 层为 stub，其余功能（设备码流 / E2EE / PTY / 攒批 / 信令）仍真实编译。 |

```bash
# 受限 CI 环境（webrtc 依赖树不可用）
cargo build --release -p ridge-cli --no-default-features
```

### 安装到系统路径（Linux）

```bash
sudo cp target/release/rdg /usr/local/bin/rdg
```

---

## 命令参考

`rdg` 使用 **clap 4** 解析参数。顶层子命令：`remote`（WebRTC 远程控制）与
`tmux`（在本机托管无头 tmux 会话引擎）。

### 顶层用法

```
USAGE:
    rdg <SUBCOMMAND>

SUBCOMMANDS:
    remote    远程控制：配对（--enable）或后台守护（--daemon）
    tmux      在本机托管无头 tmux 会话引擎（teammate 协议子集），供 `tmux` shim 连接

OPTIONS:
    -h, --help       Print help
    -V, --version    Print version
```

---

### `rdg remote`

```
USAGE:
    rdg remote [OPTIONS]

OPTIONS:
    --enable           启动设备码配对流程，绑定后把 device JWT 写入
                       ~/.config/ridge/auth.json
    --daemon           以守护进程运行：连接信令、等待 controller、桥接本地 shell
    --shell <SHELL>    指定要拉起的 shell（默认按平台探测：$SHELL → bash → sh）
    --cwd <CWD>        会话 shell 的工作目录（默认 $HOME / 当前目录）
    --root <ROOT>      fs 服务根沙箱：限定 controller 可读的目录子树
                       [env: RIDGE_REMOTE_ROOT]
    -h, --help         Print help
```

**选项说明**：

| 选项 | 类型 | 说明 |
|------|------|------|
| `--enable` | flag | 执行设备码配对流程（契约 §4.4）。调用 `POST /api/v1/device/code` 取配对码，打印 ANSI 彩色提示，异步轮询直到配对成功，将 device JWT 写入 `~/.config/ridge/auth.json`（Linux 权限 0600）。 |
| `--daemon` | flag | 加载已保存凭据 → 拉取 ICE servers → 连信令 WS（role=host）→ 等 controller 接入 → 建立 WebRTC DataChannel + E2EE → 桥接本地 shell PTY。信令断开后按指数退避自动重连。 |
| `--shell` | 可选字符串 | 覆盖默认 shell，例如 `--shell /bin/zsh`。 |
| `--cwd` | 可选字符串 | 覆盖 PTY 会话的起始工作目录，例如 `--cwd /srv/app`。 |
| `--root` | 可选字符串 | **fs 服务根沙箱**（D-GM-9）：把 controller 经 `search` / 列目录可触达的路径限定在该子树内，避免公网 host 暴露 `~/.ssh`、`/etc/passwd` 等。也可由 `RIDGE_REMOTE_ROOT` 环境变量提供（空/全空白视为未设）。 |

**`--enable` 与 `--daemon` 可组合使用**：配对成功后立即进入守护模式。

**fs 服务根沙箱（`--root` / `RIDGE_REMOTE_ROOT`）**：headless host 跑在公网 VPS 上时，
controller 的 `search` / 列目录命令默认会被关进一个服务根，**优先级 `--root` > `--cwd` >
进程当前目录**——即便不显式配置，裸 `rdg remote --daemon` 也只暴露其启动目录而非整机
文件系统（secure-by-default）。落在服务根之外的路径由 `ridge_core` 的 `sandbox_guard` 在
`dispatch` 入口统一拒绝。需要放开为整机时显式设 `--root /`（不推荐）；当服务根最终为空
（连当前目录都不可读）时日志会打 `WARN` 提示 fs 不受限。该沙箱仅约束 `remote` 子命令的
**只读 fs 命令**，与 PTY shell 自身能访问的路径无关。

#### 示例：配对（首次）

```bash
rdg remote --enable
```

终端输出（ANSI 彩色，实际颜色随终端而定）：

```
  ╔══════════════════════════════════════════════╗
  ║          RIDGE · DEVICE PAIRING               ║
  ╚══════════════════════════════════════════════╝

  1. 在已登录的浏览器打开:  https://remo2ridge.duckdns.org/activate
  2. 输入下面的配对码 (≈10 分钟内有效):

        ▎ XA4B-97RE ▎

  等待绑定中… (Ctrl-C 取消)

  ✓ 设备已绑定
    device   : my-vps
    username : alice
    公网入口 : https://my-vps-alice.remo2ridge.duckdns.org
    凭据已写入: ~/.config/ridge/auth.json
```

配对完成后：

```bash
配对完成。运行 `rdg remote --daemon` 开始守护。
```

#### 示例：守护运行

```bash
rdg remote --daemon
# 指定 shell 和工作目录
rdg remote --daemon --shell /bin/zsh --cwd /srv/app
# 把 controller 可读的 fs 限定在某子树（公网 VPS 推荐）
rdg remote --daemon --root /srv/app
# 经环境变量注入服务根（适配 systemd EnvironmentFile）
RIDGE_REMOTE_ROOT=/srv/app rdg remote --daemon
# 一步配对+启动
rdg remote --enable --daemon
```

#### 示例：无选项时的错误输出

```bash
rdg remote
# stderr:
# 请指定 --enable（配对）或 --daemon（守护）。详见 `rdg remote --help`。
```

---

### `rdg tmux`

在本机起一个 **teammate 协议子集** 的 HTTP 端点，背后是与桌面端**逐字节同源**的
`ridge-tmux` 无头会话引擎（`/api/v1/tmux/*` 路由由 `ridge_tmux::http` 提供，桌面端
teammate server 与本子命令挂载的是**同一个 router**）。用途：让运行在本 host 上的
agent（Claude Code 等）通过 PATH 上的 `tmux` shim，在这台无图形界面的机器上创建 /
send-keys / capture / kill 无头 tmux 会话——无需桌面 app、无需 controller。

```
USAGE:
    rdg tmux [OPTIONS]

OPTIONS:
    --port <PORT>    监听端口（默认 0 = 由系统分配，启动后打印实际端口）[env: RIDGE_TMUX_PORT]
    --bind <ADDR>    监听地址（默认 127.0.0.1，仅本机回环）             [env: RIDGE_TMUX_BIND]
    --token <TOKEN>  鉴权 token（默认随机生成并打印）                  [env: RIDGE_TMUX_TOKEN]
    -h, --help       Print help
```

**环境变量 fallback**：三个选项均可由 `RIDGE_TMUX_*` 环境变量提供，优先级为
**命令行参数 > 环境变量 > 内置默认**（clap `env` 特性）。这让 systemd `EnvironmentFile`
能注入配置——尤其是 token：放进 0600 的 env 文件比写在命令行更安全（命令行上的 token 会被
`ps` 看到）。空/全空白的 `RIDGE_TMUX_TOKEN`（如占位行 `RIDGE_TMUX_TOKEN=`）视为未提供，退回
随机生成。配套的 `packages/ridge-cli/ridge-tmux.service` + `ridge-tmux.env.example` 即基于此；
端到端部署见 [`ridge-vps-agent.md`](./ridge-vps-agent.md)。

启动后向 **stderr** 打印供 agent 注入的环境变量（`tmux` shim 据此连接）：

```bash
rdg tmux --port 47615
# stderr:
# rdg tmux engine listening on http://127.0.0.1:47615
#
#   export RIDGE_TEAMMATE_URL=http://127.0.0.1:47615
#   export RIDGE_TEAMMATE_TOKEN=<random-hex>
#
# 将 `tmux` shim 放入 PATH 后，本 host 上的 agent 即可创建无头 tmux 会话。
```

典型用法：

```bash
# 1) 起引擎（前台或交给 systemd / nohup 守护）
rdg tmux --port 47615 --token "$(openssl rand -hex 16)"

# 2) 在 agent 进程环境里导出上面两个变量，并把 Ridge 的 `tmux` shim 放进 PATH
export RIDGE_TEAMMATE_URL=http://127.0.0.1:47615
export RIDGE_TEAMMATE_TOKEN=...
# 3) agent 照常发 `tmux new-session -L work -d ...` / `tmux capture-pane -t work -p`
```

**鉴权**：每个请求需带 `x-ridge-token: <TOKEN>` 或 `Authorization: Bearer <TOKEN>`，
否则返回 401。

**与桌面端的差异**：headless host 没有可见工作区，故 GUI 专属的 `summon`（把会话领养
进可见分屏）不在本子命令暴露；其余 native 路由（new-session / has-session / resolve /
list-sessions / list-panes / capture-pane / list-windows / display-message /
split-window / send-keys / select / kill / list-all-sessions）行为与桌面端一致。

**安全边界**：默认仅监听 `127.0.0.1`。引擎对会话内子进程**无路径沙箱**——拉起的 shell
拥有运行 `rdg` 的用户的全部权限。跨机暴露（`--bind 0.0.0.0`）须自行用 firewall /
反向代理 / systemd 加固，并务必设置强 `--token`。

---

## 协议 / 输出格式

### 总体架构

守护运行时，`rdg` 在 **E2EE WebRTC DataChannel** 之上实现了一套二进制帧协议：

```
DataChannel 帧（E2EE 密文）
    └── 解密后明文：
           host→controller：[通道字节] [payload]
           controller→host：JSON 控制消息（UTF-8）
```

### 通道字节（channel prefix）

`packages/ridge-cli/src/protocol.rs` 中定义：

| 常量 | 值 | 含义 |
|------|----|------|
| `channel::PTY_OUTPUT` | `0x10` | 后续字节为 PTY 原始输出（二进制）。控制端 `kernel.feed()` 直接消费。 |
| `channel::JSON` | `0x11` | 后续字节为 UTF-8 JSON（带外响应，如搜索结果、目录列表、错误）。 |

### controller → host：ControlMsg（JSON tagged 枚举）

序列化格式：`{"t": "<kebab-case variant>", ...fields}`

| variant（`t` 字段） | 字段 | 说明 |
|---------------------|------|------|
| `"input"` | `data: String` | 键盘 / 粘贴输入，写入 PTY stdin。 |
| `"resize"` | `cols: u16, rows: u16` | 终端尺寸变化，调用 `pty.resize(cols, rows)`。 |
| `"search"` | `root: String, query: String, use_regex: bool (default false), case_sensitive: bool (default false)` | ripgrep 级文本搜索，经 `ridge_core::dispatch("search", …)` 执行，结果以 `HostMsg::SearchResult` 回传。 |
| `"tree"` | `path: String` | 列出一层目录子项，经 `ridge_core::dispatch("get_directory_children", …)` 执行，结果以 `HostMsg::Tree` 回传。 |

**示例（controller 发送的 JSON 明文，在 E2EE 之内）**：

```json
{"t":"resize","cols":200,"rows":50}
{"t":"input","data":"ls -la\n"}
{"t":"search","root":"/srv/app","query":"TODO","use_regex":false,"case_sensitive":false}
{"t":"tree","path":"/srv/app"}
```

### host → controller：HostMsg（JSON tagged 枚举）

带外响应，用 `0x11` 通道前缀包裹。格式：`{"t": "<kebab-case variant>", ...fields}`

| variant（`t` 字段） | 字段 | 说明 |
|---------------------|------|------|
| `"search-result"` | `results: Vec<SearchResult>` | 搜索命中列表。 |
| `"tree"` | `entries: Vec<FileNode>` | 目录子项列表。 |
| `"error"` | `message: String` | 人类可读错误（不泄露内部路径细节）。 |

**SearchResult DTO**（线形 schema，保持与现网 controller 字节级兼容）：

```json
{
  "file": "/srv/app/src/main.rs",
  "line": 42,
  "column": 5,
  "content": "    // TODO: handle error"
}
```

**FileNode DTO**：

```json
{
  "name": "src",
  "path": "/srv/app/src",
  "is_dir": true
}
```

### ridge_core::dispatch 接口

`search` 和 `tree` 两个控制命令在内部经由 `ridge_core::dispatch` 路由：

```rust
// 签名（packages/ridge-core/src/dispatch.rs）
pub fn dispatch(method: &str, args: serde_json::Value, ctx: &Ctx)
    -> Result<serde_json::Value, CoreError>
```

- dispatch 边界**字符串类型化**（stringly-typed，GM 决策 D-S1-1）：`method` 为方法名字符串，`args` 为 `serde_json::Value`，符合线协议的 JSON 形式。
- `ControlMsg::Search` 对应 dispatch 方法名 `"search"`（`REMOTE_ALLOWLIST` 中的别名；与桌面侧 `"text_search"` 同处理器）。
- `ControlMsg::Tree` 对应 dispatch 方法名 `"get_directory_children"`。
- headless 侧使用 `headless_ctx()`（`packages/ridge-cli/src/core_host.rs`）构造每请求 `Ctx`，携带 `CapabilitySet::remote_default()` 能力集和 no-op 事件 sink。

### 握手流程（E2EE，契约 §7.1）

DataChannel 打开后：
1. 双方各发一条二进制消息 `0x01 || X25519公钥(32字节)`。
2. 双方用 X25519 DH + HKDF-SHA256 派生会话密钥，方向分离（host→controller / controller→host 各一对密钥）。
3. Nonce 严格递增（counter），防止重放攻击。
4. 握手超时：15 秒。

### PTY 输出攒批

为减少 DataChannel 帧开销，host 侧采用 **16ms 攒批**缓冲（`packages/ridge-cli/src/batching.rs`）：
- 16ms 窗口内多次 PTY 输出合并为一帧发送。
- 超过硬上限（缓冲区满）立即 flush，不等窗口到期。

---

## 能力与安全边界

### D8 能力白名单

`rdg` 使用 `CapabilitySet::remote_default()`（`packages/ridge-core/src/capability.rs`），
这是与桌面 LAN host 共享的**同一份**方法名白名单常量 `REMOTE_ALLOWLIST`。

白名单涵盖以下类别（完整列表见 `capability.rs` 中的 `REMOTE_ALLOWLIST` 常量）：

| 类别 | 代表方法 |
|------|----------|
| 文件系统（读写） | `get_file_tree`, `get_directory_children`, `read_file`, `write_file`, `create_file`, `delete_path`, … |
| 搜索 | `text_search`, `search` (别名), `filename_search`, `replace_in_files` |
| 分屏 / 终端 | `get_pane_layout`, `split_pane`, `write_to_pty`, `resize_pane`, … |
| 工作区（实时 + 持久化） | `switch_workspace`, `create_workspace`, `save_workspace`, … |
| 主题 / 设置 | `get_theme_data`, `set_active_theme`, `set_user_default_cwd` |
| Git（读 + 写） | `get_scm_status`, `git_stage`, `git_commit`, `git_push`, … |

**刻意排除的 host 特权命令**（不在白名单中，dispatch 会返回 `CapabilityDenied`）：

- `get_remote_info`
- `set_remote_enabled`
- `disconnect_session`
- `enter_deep_root_mode`
- `set_cloud_remote_active`
- 及全部黑名单管理命令

### dispatch 入口的四层检查（按顺序）

1. **能力准入（D8）**：方法名必须在 `CapabilitySet` 中。
2. **路径穿越守卫**：`path`/`from`/`to`/`root`/`cwd`/`repoRoot`/`paths[]` 中任何包含 `..` 段的参数立即拒绝（`CoreError::PathTraversal`）。
3. **沙箱 / root-scoping 守卫（D8/§5.6，R10）**：若 host 通过 `CapabilitySet::with_roots([…])` 注入了工作区根目录，所有路径参数必须解析在这些根目录之内，否则拒绝（`CoreError::OutsideSandbox`）。**当前 headless `headless_ctx()` 未注入 roots，即沙箱为空（不受限）**——这是已知的安全缺口，见下文。
4. **方法表**：路由到对应的迁移处理器。

### fs 沙箱现状（重要限制）

`headless_ctx()` 当前构造的 `CapabilitySet` 未传入 workspace roots，
因此 **fs 路径沙箱实际上没有启用**（`RootScope::unrestricted()`）。
这意味着 controller 可以通过 `search` / `tree` 命令访问 headless 机器的任意文件路径，
包括 `~/.ssh`、`/etc` 等敏感目录。

沙箱机制已就绪（`CapabilitySet::with_roots([…])`），但 S5 切片中尚未在 daemon 启动时
从配置或命令行参数注入根目录。这是 **S8（安全与可观测，R10）**的待办项。

---

## 已迁移 vs 未迁移命令

以下分类来自 `docs/plans/s1-migration-ledger.md`，描述哪些方法在 `ridge_core::dispatch`
中真实可用，哪些虽在白名单中但 dispatch 会返回 `MethodNotFound`（需回退 host 侧桥接）。

### 已迁移（dispatch 可直接路由）

当前 `rdg` 通过 `ridge_core::dispatch` 真实可用的方法：

| 方法 | 来源模块 | 说明 |
|------|----------|------|
| `get_theme_data` | `ridge_core::commands::theme` | 读取主题目录，返回主题列表 + 激活 ID |
| `set_active_theme` | `ridge_core::commands::theme` | 切换激活主题（写 `active-theme.txt`）|
| `set_user_default_cwd` | `ridge_core::commands::settings` | 设置用户默认工作目录 |
| `get_file_tree` | `ridge_core::fs::commands` | 返回以指定路径为根的文件树（递归，含深度限制）|
| `get_directory_children` | `ridge_core::fs::commands` | 返回一层目录子项（分页：offset / limit）|
| `read_file` | `ridge_core::fs::commands` | 读取文件内容为 UTF-8 字符串 |
| `path_exists` | `ridge_core::fs::commands` | 检查路径是否存在，返回 `bool` |
| `read_file_for_editor` | `ridge_core::fs::commands` | 读取文件供编辑器使用（含二进制探测、大文件截断等）|
| `text_search` / `search` | `ridge_core::fs::commands` | ripgrep 级文本搜索（两个方法名路由到同一处理器）|

这些方法均为只读 fs 操作，在 S5 切片中作为"read-only fs slice"已迁移。
`search` 是 `ControlMsg::Search` 使用的别名，与桌面侧 `text_search` 共享实现。

### 未迁移（白名单中存在，但 dispatch 返回 `MethodNotFound`）

以下大类虽在 `REMOTE_ALLOWLIST` 中，但对应 handler 尚未迁移进 `ridge_core::dispatch`：

| 大类 | 代表方法 | 迁移计划 |
|------|----------|---------|
| **fs 写操作** | `write_file`, `apply_file_edits`, `rename_path`, `delete_path`, `create_file`, `create_directory`, `copy_path`, `move_path` | S1 余量（需先落 read-only gate 下沉）|
| **Git（全部 32 个）** | `get_scm_status`, `git_stage`, `git_commit`, `git_push`, `git_pull`, 等 | S1 余量（`git.rs` 最易迁，零 Tauri 状态）|
| **分屏 / 终端** | `get_pane_layout`, `split_pane`, `write_to_pty`, `resize_pane`, `activate_pane_pty`, 等 | S5（需 D11 领域模型 + PtyHost 端口，最重）|
| **工作区（全部）** | `switch_workspace`, `create_workspace`, `save_workspace`, 等 | S5（D11 共享实体图谱）|
| **filename_search / 诊断 / replace** | `filename_search`, `text_search_diagnostics`, `replace_in_files` | S1 余量（需 read-only gate 确认）|
| **fs 监控** | `start_watching_paths`, `start_watching_repos` | S1 余量（需后台任务 + 背压）|

**对 `rdg` 的实际影响**：当前版本通过 `ControlMsg`（`protocol.rs`）只暴露
`Input` / `Resize` / `Search` / `Tree` 四种控制消息，分别路由到 PTY 写入 / PTY resize /
`ridge_core::dispatch("search")` / `ridge_core::dispatch("get_directory_children")`。
未迁移的白名单方法不可通过当前 `ControlMsg` 协议访问。

---

## 示例工作流

### 工作流一：VPS 首次配对并启动守护

```bash
# 步骤 1：构建（从仓库根）
cargo build --release -p ridge-cli
sudo cp target/release/rdg /usr/local/bin/rdg

# 步骤 2：首次配对（需要网络 + 浏览器）
rdg remote --enable
# 按提示在浏览器 https://remo2ridge.duckdns.org/activate 输入配对码

# 步骤 3：启动守护
rdg remote --daemon
# 此时守护已连接信令服务器，等待 controller 接入
```

从控制端浏览器连接后，Ridge 会建立 WebRTC DataChannel，发送 `ControlMsg::Resize` 设定
终端尺寸，随后即可收到 PTY 输出。

### 工作流二：systemd 系统级自启动

```bash
# 创建专用系统账户
sudo useradd --system --create-home --home-dir /var/lib/ridge \
    --shell /usr/sbin/nologin ridge

# 以该账户完成配对（一次性）
sudo -u ridge -H /usr/local/bin/rdg remote --enable

# 安装 systemd 服务
sudo cp packages/ridge-cli/ridge-cli.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ridge-cli

# 查看日志
journalctl -u ridge-cli -f
```

systemd 单元已配置：低 CPU 优先级（`Nice=10`，`CPUWeight=20`）、内存上限 256M、
`NoNewPrivileges=true`、`ProtectSystem=strict` 等沙箱加固，失败后自动退避重启。

---

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `RUST_LOG` | `info` | 日志级别。格式：`info`、`debug`、`ridge_cli=debug,info`。日志写入 **stderr**（systemd 收进 journald）。 |
| `RIDGE_BASE_DOMAIN` | `remo2ridge.duckdns.org` | 覆盖默认服务域名。用于自托管 / 本地测试。影响所有 URL（信令 WS、API、激活页）。 |

---

## 已知限制

1. **fs 沙箱未启用（安全缺口）**：`headless_ctx()` 未注入 workspace roots，controller 可通过 `search` / `tree` 访问 headless 机器的任意可读路径。生产环境应在 S8 完成沙箱注入前，通过 firewall / systemd `ReadOnlyPaths` / `ProtectHome` 等系统级措施补位。

2. **只有四种控制消息**：当前 `ControlMsg` 只支持 `Input` / `Resize` / `Search` / `Tree`。git / 工作区 / 分屏 / 文件写入等功能尚未在 headless 协议层暴露，需等待 S5 后续切片。

3. **无工作区 / 分屏领域模型**：D11（共享实体图谱 + 每连接视图）未实现，headless 侧只有单一 PTY 会话，无工作区切换 / 多 pane 支持。

4. **git 凭据缺口（headless 环境）**：无 GUI 机器上 git 远程操作（push / pull / fetch）需要凭据来源（SSH agent / credential helper），当前未定义。计划先以"本地-only git 能力档"分期交付。

5. **PTY 环境依赖系统配置**：shell 默认探测顺序 `$SHELL → bash → sh`，不继承桌面会话的 login env。生产部署时建议用 `--shell` / `--cwd` 显式指定。

6. **单会话**：daemon 一次只服务一个 controller 会话。controller 断开后等待下一个接入，不支持并发多 controller。

7. **`--no-default-features` 构建缺少 WebRTC**：去掉 `rtc` 特性后 `WebRtcHost` 为 stub，`remote --daemon` 会在会话建立时失败（无法创建真实 peer connection）。该构建仅用于 CI 验证其他逻辑。

8. **凭据文件仅支持单设备配对**：`auth.json` 存储单份 device JWT；多设备绑定不在当前范围内。

---

## 模块速查

| 模块 | 文件 | 职责 |
|------|------|------|
| `main` | `src/main.rs` | CLI 入口（clap）：`Command::Remote` → `run_remote()`，`Command::Tmux` → `run_tmux()`（挂载 `ridge_tmux::http` 共享 router）|
| `daemon` | `src/daemon.rs` | 守护主循环 + 指数退避重连（2s–30s） |
| `session` | `src/session.rs` | 信令→WebRTC→E2EE→PTY 完整会话编排 |
| `protocol` | `src/protocol.rs` | `ControlMsg` / `HostMsg` 定义 + 通道前缀常量 |
| `core_host` | `src/core_host.rs` | `headless_ctx()` 工厂（携带 `CapabilitySet::remote_default()`）|
| `fs_reuse` | `src/fs_reuse.rs` | 线形 DTO（`SearchResult` / `FileNode`）+ 到 ridge_core dispatch 的映射 |
| `device_flow` | `src/device_flow.rs` | 设备码流（契约 §4.4）+ ANSI 配对码 banner |
| `config` | `src/config.rs` | `AuthFile` 凭据持久化 + URL 拼接 + `RIDGE_BASE_DOMAIN` 覆盖 |
| `e2ee` | `src/e2ee.rs` | X25519 + HKDF-SHA256 + ChaCha20-Poly1305（方向分离 nonce）|
| `batching` | `src/batching.rs` | 16ms 攒批缓冲，含硬上限 flush |
| `signaling` | `src/signaling.rs` | 信令 WS 客户端（role=host，§5）|
| `rtc` | `src/rtc.rs` | host answerer（`webrtc` crate；`--no-default-features` 时为 stub）|
| `pty` | `src/pty.rs` | `portable-pty` 拉起本地 shell（解耦 Tauri AppState）|
| `ice` | `src/ice.rs` | ICE servers 拉取（契约 §5.2）|
