# Ridge

<p align="center">
  <i>如田埂分畦，各耕其获 —— 像田埂一样分割你的工作，每一片田都有各自的产出。</i>
</p>

<p align="center">「畦」为田界，「埂」为田埂。四块方田，十字一横，各自耕作，各自丰收。</p>

---

## 架构

Ridge 是基于 Tauri v2 构建的**原生终端工作台**（Rust 后端 + Svelte 5 前端）。每个窗格承载一个独立的 PTY 会话；布局引擎支持无限递归的水平/垂直分割。WebWorker 托管的终端渲染器（Rust → WASM）驱动网格，WebGPU 为主渲染后端，Canvas2D 为通用回退方案。

```
┌─ Tauri v2 (Rust) ─────────────────────────────────────────┐
│  ┌─ commands/ ──┐  ┌─ engine/ ──────────┐  ┌─ remote/ ─┐ │
│  │ git · pane    │  │ pane_tree · pty    │  │ auth.rs   │ │
│  │ terminal      │  │ parser · cwd       │  │ mDNS      │ │
│  │ workspace     │  │ title · delta      │  │ WebSocket │ │
│  │ project       │  └────────────────────┘  └───────────┘ │
│  └───────────────┘                                         │
│  ┌─ teammate/ ──┐  ┌─ fs/ ──────┐  ┌─ db/ ──────────────┐ │
│  │ tmux shim    │  │ search     │  │ projects.db (SQLite)│ │
│  │ HTTP API     │  │ tree walk  │  └────────────────────┘ │
│  └──────────────┘  └────────────┘                         │
├───────────────────────────────────────────────────────────┤
│  ┌─ SvelteKit SPA (TypeScript) ───────────────────────┐   │
│  │  SplitContainer → @ridge/split（自定义布局组件）    │   │
│  │  RidgePane（终端 shell + Monaco 编辑器）            │   │
│  │  侧栏：资源管理器 · 搜索 · 源代码管理 · 扩展        │   │
│  │  多工作区 + .ridge 文件持久化                       │   │
│  │  TerminalManager → WebWorker → ridge-term (WASM)    │   │
│  └─────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────┘
```

### 包（pnpm monorepo）

| 包名 | 说明 |
|---------|-------------|
| `packages/ridge-term` | Rust 终端内核：VT 解析器、网格、回滚缓冲区、选区、搜索、Canvas2D/WebGPU 渲染后端。编译为 WASM。 |
| `packages/rg-split` | 纯渲染的分割窗格布局组件。零内部状态；store 是唯一真相源。 |

---

## 核心功能

### 终端
- 无限递归的**水平/垂直分割**，支持拖拽停靠
- 每个窗格独立 PTY：各自拥有独立的 shell 进程、工作目录和历史记录
- **增量帧协议**（P3）：Rust 端解析器通过 Tauri Channel 发送 postcard 压缩的 `GridDelta` 帧 —— 典型键盘回显约 10 字节，对比 JSON 约 3 KB
- **自适应 PTY 输出合并**，亚毫秒级回显路径：合并窗口从 0ms（<256 B）到 8ms（>=4 KB）动态调整
- 回滚缓冲区支持搜索、"加载更早"分页和用户滚动锁定检测
- Shell 集成 prompt 标记（`OSC 133;A` / `OSC 633;A`），用于即时 git diff 刷新
- 每窗格 **Shell 切换器**（pwsh、cmd、bash、git-bash、WSL）
- 内联链接识别：可点击的超链接、文件路径、git SHA

### 工作区
- 多个**命名工作区**，各自独立的窗格树，切换时所有进程保持运行
- 单一**全局宿主画布**（A.9）：工作区切换是纯 DOM 显示切换 —— 无需画布重配置、交换链不清空、无黑屏闪烁
- 通过 `~/ridge-workspaces/` 下的 `.ridge` 文件保存/恢复工作区
- 启动恢复：从菜单启动时重新打开上次会话；CLI `ridge` 启动则使用当前目录

### 编辑器
- Monaco Editor 作为**替代窗格模式**，融入分割布局
- 差异对比模态框，支持行内文件比较
- Markdown 预览，支持 Mermaid 图表渲染

### Git（源代码管理）
- 基于 libgit2 历史渲染的提交图谱（非 `git log` 子进程）
- 每窗格 **Git 状态标记**：分支名、超前/落后计数、已变更文件数
- **SCM 侧栏**：暂存/取消暂存/放弃单个文件、带消息提交、分支切换、创建标签、cherry-pick、revert
- Git 操作：fetch、pull、push、sync，针对 SCM 选定的仓库
- 文件系统变更自动刷新（notify crate）—— 无需轮询

### 搜索
- **跨工作区搜索**，支持正则、大小写敏感、全词匹配、glob 过滤开关
- 跨匹配结果批量替换文件内容
- 文本搜索诊断侧栏

### 协作
- **Teammate 服务器**（本地 HTTP API）：外部代理（Claude Code 等）可列出/创建/关闭窗格、读取窗格 CWD、管理工作区布局
- **tmux 兼容层**：名为 `tmux` 的二进制文件，将标准 tmux CLI 命令翻译为 Ridge teammate HTTP 调用 —— 可直接嵌入 Claude Code 的 `teammateMode: tmux`
- 设置面板中的代理统计仪表板

### 远程控制
- **LAN-First** 移动端 Web 应用，由 Tauri 后端提供服务（也提供独立服务器）
- 基于 TOTP 的身份验证（RFC 6238，无外部依赖）
- mDNS 服务发现（`_ridge._tcp.local.`），端口 5353
- 每设备黑名单、会话管理
- 远程侧栏复用与桌面端相同的共享 TypeScript 组件（`src/shared/sidebar/`）
- WebSocket 二进制终端数据流，每客户端独立移动端解析器实例

### 主题
- 三套内置主题，使用 CSS 自定义属性
- 主题系统在启动画面阶段注入（早于 SvelteKit 水合），首帧即匹配用户保存的主题
- Monaco 编辑器主题自动从 Ridge 主题数据同步

### 插件
- 轻量级侧栏插件系统，支持全局和 workspace 作用域
- 内置插件：全局状态面板

---

## 技术栈

| 层 | 技术 |
|-------|-----------|
| 桌面框架 | [Tauri v2](https://v2.tauri.app/) |
| 前端 | Svelte 5 + SvelteKit（SPA 模式）+ Tailwind CSS v4 |
| 终端内核 | 自研 Rust crate（`ridge-term`）→ 通过 wasm-bindgen 编译为 WASM |
| GPU 渲染 | wgpu（WebGPU），Canvas2D 回退（通过 OffscreenCanvas） |
| 渲染宿主 | Web Worker（`type: module`），主线程外渲染 |
| 代码编辑器 | Monaco Editor 0.55 |
| Git | libgit2（`git2` crate v0.19） |
| 数据库 | SQLite（通过 rusqlite，bundled） |
| 远程服务器 | axum 0.7（WebSocket + 静态文件服务） |
| 认证 | TOTP（SHA-256 HMAC，自实现 RFC 6238） |
| IPC | Tauri `invoke()` 用于 RPC，Tauri Channel 用于增量帧，Tauri 事件用于 PTY 输出 |
| 序列化 | postcard（二进制）用于网格增量帧，serde_json 用于 RPC |
| PTY | portable-pty 0.8 |
| 字体图标 | lucide-svelte |
| 图表 | Mermaid 11 |
| Markdown | marked 15 |
| 文件监控 | notify v6 + notify-debouncer-mini |
| 进程发现 | sysinfo |
| 测试 | Vitest（单元测试）、Playwright（端到端测试）、WebdriverIO（Shell 集成测试 + 性能测试） |

---

## 开发

### 环境要求
- **Rust** 工具链（最新 stable）
- **Node.js** ≥ 18
- **pnpm** ≥ 8
- **Tauri v2** 系统依赖（[安装指南](https://v2.tauri.app/start/prerequisites/)）
- Windows：WebView2 运行时（Windows 10 21H2+ 已内置）

### 常用命令

```bash
# 安装依赖
pnpm install

# 开发模式（Vite HMR + Tauri 热重载）
pnpm tauri dev

# 构建发布版二进制文件
pnpm tauri:build

# 运行测试
pnpm test          # 单元测试（vitest）
pnpm e2e           # 端到端测试（Playwright）
pnpm e2e:shell     # Shell 级别集成测试（WebdriverIO）
pnpm e2e:perf      # 性能基准测试（帧归因、压力测试）

# 构建 WASM 终端内核
node packages/ridge-term/build.mjs

# 构建 tmux 兼容层
pnpm build:teammate-shim

# 构建远程服务器（独立二进制文件）
pnpm build:remote-server

# 远程开发
pnpm dev:remote         # 运行远程应用开发服务器
pnpm build:remote       # 构建远程应用

# 类型检查
pnpm check
```

### 项目结构

```
src/
├── routes/                     # SvelteKit 页面（SPA 入口）
│   ├── +page.svelte            # 主应用外壳（布局、侧栏、键盘、主题、启动）
│   └── +layout.svelte          # 根布局（加载画面）
├── lib/
│   ├── components/             # Svelte UI 组件
│   │   ├── SplitContainer.svelte    # 递归窗格分割布局
│   │   ├── RidgePane.svelte         # 终端/编辑器窗格（含标题栏）
│   │   ├── FileTree.svelte          # 文件资源管理器树
│   │   ├── Explorer.svelte          # 资源管理器面板（支持拖拽）
│   │   ├── SourceControl.svelte     # SCM 面板（暂存/提交/差异）
│   │   ├── GitGraph.svelte          # 提交图谱可视化
│   │   ├── SearchSidebar.svelte     # 搜索与替换面板
│   │   ├── SettingsPanel.svelte     # 主题/字体/Shell 设置
│   │   ├── WorkspaceTabs.svelte     # 工作区标签栏（支持拖拽排序）
│   │   ├── QuickOpen.svelte         # Ctrl+P 文件快速打开
│   │   ├── MarkdownPreview.svelte   # Markdown/Mermaid 预览窗格
│   │   ├── DiffEditorModal.svelte   # 并排差异查看器
│   │   └── ...
│   ├── terminal/               # 终端生命周期与渲染
│   │   ├── manager.ts          # TerminalManager 单例（attach、render RAF 循环）
│   │   ├── ptyBridge.ts        # PTY 输出 → kernel.feed() 桥接
│   │   ├── workerHostedRenderer.ts   # WebWorker 代理，主线程外渲染
│   │   ├── renderWorker.ts     # Worker 端渲染编排
│   │   ├── workerRendererBridge.ts   # 主线程 ↔ Worker 协议定义
│   │   ├── linkSpans.ts        # 可点击链接检测器
│   │   └── ...
│   ├── stores/                 # Svelte writable stores
│   │   ├── paneTree.ts         # 全局窗格树状态（分割/关闭/停靠/比例）
│   │   ├── fileExplorer.ts     # 文件树状态
│   │   ├── scmCache.ts         # Git 状态缓存
│   │   ├── paneGitStatus.ts    # 每窗格 git 标记数据
│   │   ├── themes.ts           # 主题状态与持久化
│   │   ├── settings.ts         # 用户设置
│   │   ├── searchState.ts      # 搜索面板状态
│   │   └── ...
│   ├── transport/              # IPC 抽象
│   │   ├── tauri.ts            # Tauri invoke() 传输
│   │   └── ws.ts               # WebSocket 传输（远程）
│   ├── remote/                 # 远程控制 UI
│   │   ├── RemotePanel.svelte  # 二维码 + 会话列表
│   │   └── wsClient.ts         # 远程 WS 客户端
│   ├── plugins/                # 侧栏插件系统
│   └── utils/                  # 链接解析器、Markdown、ANSI、路径工具
├── remote/                     # 独立移动端远程应用（Svelte，不含 SvelteKit）
│   ├── App.svelte
│   ├── MainApp.svelte
│   └── lib/
│       ├── terminalController.ts  # 基于 WebSocket 的终端客户端
│       ├── VirtualKeyboard.svelte # 移动端虚拟键盘
│       └── TerminalCanvas.svelte  # 移动端 Canvas 终端
├── shared/sidebar/             # 传输层无关的侧栏组件
│   ├── SidebarFileTree.svelte
│   ├── SidebarGitPanel.svelte
│   ├── SidebarSearch.svelte
│   └── types.ts                # SidebarProvider 接口
└── app.html                    # HTML 外壳（启动画面加载器、启动全局变量）

src-tauri/
├── src/
│   ├── main.rs                 # 入口 → ridge_lib::run()
│   ├── lib.rs                  # 应用构建器、事件循环、命令注册
│   ├── state.rs                # AppState（工作区、终端、PTY 句柄）
│   ├── types.rs                # GlobalEvent 枚举、PaneMode、PTY 事件类型
│   ├── commands/               # Tauri IPC 命令处理器
│   │   ├── git.rs              # 25+ Git 操作（图谱、差异、暂存、提交、分支等）
│   │   ├── pane.rs             # 分割、关闭、停靠、切换模式
│   │   ├── terminal.rs         # 创建、激活、调整大小、写入、回滚、增量帧
│   │   ├── workspace.rs        # 工作区 CRUD + 保存/恢复历史
│   │   ├── project.rs          # 文件树、搜索、替换、读写、Claude/opencode 历史
│   │   ├── ridge_file.rs       # .ridge 工作区文件 I/O + 恢复集合
│   │   ├── settings.rs         # 用户默认 CWD
│   │   ├── theme.rs            # 主题数据 + 启动画面初始化脚本构建
│   │   ├── watch.rs            # Git 仓库文件系统监控
│   │   ├── fs_watch.rs         # 通用文件系统监控
│   │   ├── remote.rs           # 远程控制开关、会话/黑名单管理
│   │   └── process.rs          # 前台进程 / CWD 检测
│   ├── engine/                 # 核心引擎
│   │   ├── pane_tree.rs        # 递归分割树数据结构与操作
│   │   ├── pty.rs              # PTY 生成（2 阶段激活）
│   │   ├── parser.rs           # 原生 PaneParser（ridge-term 在桌面端）
│   │   ├── cwd.rs              # OSC 7 CWD 追踪
│   │   └── title.rs            # OSC 0/1/2 标题追踪
│   ├── fs/                     # 文件系统操作
│   │   ├── tree.rs             # 目录树构建
│   │   └── search.rs           # 类 ripgrep 文本搜索 + 文件名搜索
│   ├── remote/                 # 远程控制服务器
│   │   ├── server.rs           # Axum HTTP + WebSocket 服务器、PTY 扇出
│   │   ├── auth.rs             # TOTP（RFC 6238）实现
│   │   └── mdns.rs             # mDNS 广播（_ridge._tcp.local.）
│   ├── teammate/               # 代理协作
│   │   └── server.rs           # 面向外部代理的本地 HTTP API
│   ├── db/
│   │   └── projects.rs         # SQLite 项目存储
│   ├── utils/                  # 错误类型、日志、PTY 日志、pane_id 工具
│   └── bin/
│       ├── tmux.rs             # Tmux 兼容层：将 tmux CLI 翻译为 Ridge teammate HTTP
│       └── remote-server.rs    # 独立远程服务器二进制文件
├── Cargo.toml
└── tauri.conf.json
```

---

## 发布与分发

- **Windows**：NSIS 安装包 + MSI，均带 `PATH` 环境变量注册
- 二进制文件内嵌 `tmux` 兼容层，用于 Claude Code 代理集成
- 捆绑静态资源：`ridge.theme` 文件 + 远程 Web 应用（用于 LAN 移动端访问）
- CI：GitHub Actions 在 push 到 `main` 分支时将营销站点（`site/`）部署到 GitHub Pages

---

## 许可证

MIT License. Copyright (c) 2026 Jack Jiang and Ridge contributors.

---

<details>
<summary><b>☕ 请作者喝杯咖啡</b></summary>

<br>

<p align="center">
  <i>如果 Ridge 让你的工作流更顺畅了，不妨请我喝杯咖啡。</i>
</p>

<div align="center">

| 微信赞赏 | PayPal |
|:---:|:---:|
| ![微信赞赏]([Image 1]) | ![PayPal 捐赠]([Image 2]) |
| 微信扫码赞赏 | PayPal Donate |

</div>

<br>

</details>
