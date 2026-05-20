<div align="center">

<img src="./site/assets/ridge-mark.svg" width="64" height="64" alt="Ridge mark" />

# Ridge

**分屏终端工作台。** A modern terminal workbench with split panes, an embedded editor, Git visualisation, and Claude Code agent teams.

[Site](https://mysetsuna.github.io/ridge/) · [Docs](https://mysetsuna.github.io/ridge/docs.html) · [Releases](https://github.com/MySetsuna/ridge/releases) · [Issues](https://github.com/MySetsuna/ridge/issues)

</div>

---

## 特性 · Highlights

- **递归分屏** — 水平 / 垂直 / 嵌套，无层数限制。每个分屏都是独立的终端会话，有自己的工作目录与命令历史。
- **稳定的本地终端** — 支持 PowerShell / bash / zsh / cmd，Unicode、超链接、可滚动数 MB 的命令历史。
- **内嵌代码编辑器** — 与终端共享同一套分屏布局，可同时打开多个编辑器。
- **Git 可视化** — 提交图、分支选择器、分屏内的状态徽章，仓库变更后自动刷新。
- **跨分屏搜索** — 一次扫描所有打开的目录，支持正则、glob、批量替换。
- **Claude Code 智能体协作** — 从 Ridge 启动的智能体即获得多分屏协作能力。
- **多工作区** — 同时打开多个项目，互不干扰。

## 快速开始

需要 Node 18+, pnpm 9+, Rust 1.77+。Windows 还需要 MSVC + WebView2。

```bash
git clone git@github.com:MySetsuna/ridge.git
cd ridge
pnpm install
pnpm tauri dev
```

生产构建：

```bash
pnpm tauri build       # 安装包（Windows: NSIS / MSI）
```

## 文档

- 在线主页：<https://mysetsuna.github.io/ridge/>
- 在线文档：<https://mysetsuna.github.io/ridge/docs.html>
- 仓库内深度文档：[`docs/`](./docs/) — `TERMINAL_SCROLLBACK.md`、`AGENT_TEAMS_TEAMMATES.md`、`PANE_GIT_PILL_VERIFY.md`
- 项目根开发约定：[`CLAUDE.md`](./CLAUDE.md)
- 技术架构：[`technical_architecture.md`](./technical_architecture.md)

## Releases

[v0.1.0](https://github.com/MySetsuna/ridge/releases/tag/v0.1.0) — 2026-04-30 · 首个公开版本。
更新日志详见 [CHANGELOG.md](./CHANGELOG.md)。

## 路线图 · Roadmap (v0.0.3)

下一个版本（**v0.0.3**）聚焦两条主线：**远程控制能力**与**次世代渲染管线**。两者均以"零服务器依赖、纯客户端、开箱即用"为设计原则。

### ⚙️ 共同基础：Rust 侧 GridDelta 解析管线（P3 ladder）

为两条主线奠基的 11 步重构。已完成 ✅：

- **p3.1** 把 `ridge-term` 作为 native dep 接入 src-tauri
- **p3.2** GridDelta wire format 数据类型 (`packages/ridge-term/src/term/delta.rs`)
- **p3.3** `engine::parser::PaneParser` producer
- **p3.4** `Terminal::apply_delta` / `apply_frame` consumer + round-trip
- **p3.5** postcard codec (`encode_frame` / `decode_frame`)
- **p3.6** wasm-bindgen `applyDeltaFrame(bytes)` entry
- **p3.7** `Settings.parserBackend = 'wasm' | 'rust'`（默认 `'rust'`）
- **p3.8** main loop 消费侧接 PaneParser + emit `pty-delta-*`
- **p3.9** `set_pane_delta_mode` 命令 + manager switch + 200ms fade mask
- **p3.9.r** rust 模式下 fitPane 走单向 resize
- **p3.10** `GridDelta::Reset` producer + apply（RIS）
- **p3.11** `GridDelta::ScrollbackAppend` producer + apply
- **p3.12** `GridDelta::ModeChange` producer + apply
- **p3.13** col-range diff（per-row payload 收缩 5-20×）
- **p3.14** 真桌面 e2e (`tauri-driver` + WebdriverIO) + perf-bench backend 比较

P3 让 VT 解析从 wasm 主线程搬到 Rust tokio 任务，主线程 CPU 占用显著下降；同时为远程控制（主线一）准备好"只接收 delta 不跑解析器"的轻量客户端协议。

### 🛰️ 主线一：远程控制 · Remote Control

为 Ridge 增加跨设备远程控制能力，定位为纯 P2P 开源工具——不引入中心化信令、不依赖公网中继。

<details open>
<summary><b>🟩 Phase 1 · 真实局域网极速直连（LAN-First）</b></summary>

> 聚焦零公网依赖的本地连接，利用 mDNS 实现局域网"开箱即用"。

- **[Backend] mDNS 服务广播**
  - [ ] 引入 Rust `mdns-sd`，轻量 Daemon 启动时广播 `_ridge._tcp.local.`
  - [ ] 动态检测本地物理网卡，自动绑定 `192.168.x.x` 及监听端口
- **[Web-Client] 移动端轻量 Web 控制台**
  - [ ] 基于 Svelte 的移动端适配页面，验证手机/平板浏览器（Safari/Chrome）下的 WebGPU/WebGL 渲染
  - [ ] 桌面端生成含本地 IP 的二维码，同 WiFi 下扫码直连 `ws://` 高带宽 WebSocket
- **[Security] 基础安全校验层（2FA）**
  - [ ] 引入 TOTP 一次性密码机制
  - [ ] 首次连接强制输入 6 位动态验证码，即便在 LAN 内也保留底线安全

</details>

<details open>
<summary><b>🟨 Phase 2 · 极客公网免服务器直连（Tailscale + 动态安全层）</b></summary>

> 借助现有虚拟局域网（VLAN）基础设施，实现零服务器、不限速、跨网络的公网控制。

- **[Backend] Tailscale 网络环境感知**
  - [ ] Daemon 启动时检测 `tailscale0` 虚拟网卡，抓取分配的 IP 与 MagicDNS 域名（`*.ts.net`）
  - [ ] Web 服务严格仅监听 `tailscale0` 或物理 LAN 接口，隔离公网常规扫描
- **[UI/UX] 动态时效配对二维码**
  - [ ] 桌面端生成含 MagicDNS 域名、端口及高时效 Session Token（≈60s）的快捷连接二维码
- **[Security] 二次鉴权与长效续期**
  - [ ] 动态校验码兜底：Token 超时或二次访问时自动弹出 2FA 输入框
  - [ ] 校验成功后由 Rust 后端下发长效 `AuthToken`，浏览器 `localStorage` 持久化，实现后续无感连接
- **[Architecture] 纯点对点通信架构定型**
  - [ ] 删除所有中心化信令 / 中继中转规划
  - [ ] 确立"用户自建网络环境（LAN/VLAN）+ 客户端安全验证"为核心的纯净 P2P 开源工具属性

</details>

---

### 🎨 主线二：次世代渲染优化 · Rendering & Emoji Refinement

继续打造基于 WebGPU 的高性能、像素级精准且支持复杂 Emoji 的终端模拟器；目标对标 Warp 与现代浏览器级别的字符画／变宽字形渲染效果。

<details open>
<summary><b>📅 Milestone 1 · 几何精度与制表符完美衔接（消除缝隙）</b></summary>

> 解决连续字符画、Claude Code 启动界面等场景的微小间隙，实现严丝合缝的像素级对齐。

- **物理像素网格强制对齐（Pixel Snapping）**
  - [ ] 审视 `frame.viewport` 对 `window.devicePixelRatio` (DPR) 的缩放，CPU 端将单元格尺寸转为整数物理像素
  - [ ] 修改 `vs_main.wgsl`，在像素→NDC 转换前显式 `floor(pixel_pos + 0.5)`，消除浮点光栅化裂缝
- **制表符／块字符过程化渲染（Procedural Box Drawing）**
  - [ ] Rust 端（`glyph_rasterizer.rs`）拦截 `U+2500`~`U+257F` 与 `█` 等块字符，不生成字形纹理
  - [ ] 扩展 `InstanceIn` 增加 `is_full_block` 标志位或特殊编码
  - [ ] `fs_main.wgsl` 命中标志时跳过纹理采样，直接 `return in.fg`；非全满线条对边缘做 0.5 像素微幅重叠（Overdraw）确保严丝合缝

</details>

<details open>
<summary><b>📅 Milestone 2 · 逻辑与视觉解耦的流式文本布局（复杂 Emoji）</b></summary>

> 打破等宽网格束缚，完美支持 ZWJ 复合 Emoji，同时保证光标与选区不跑位。

- **文本塑形层（Text Shaping Pass）**
  - [ ] 引入 `rustybuzz` 或 `cosmic-text`
  - [ ] 非纯 ASCII 行激活 Shaping，将复合 Code Points 聚合为单一 Glyph Cluster，取得精确视觉物理像素宽度
- **动态宽度 Quad 注入（Cluster-based Instance）**
  - [ ] 允许 `glyph_rasterizer.rs` 栅格化大于标准 Cell 槽位的变宽 Emoji，独立分配动态纹理尺寸
  - [ ] `InstanceIn` 写入实际像素宽度到 `@location(1) cell_size.x`，允许视觉形态自由溢出
- **"两本账"解耦策略（逻辑固化 / 视觉溢出）**
  - [ ] PTY 状态机仍按 `unicode-width` 视复杂 Emoji 为 2 宽，保证 Backspace / 光标行进逻辑稳定
  - [ ] 渲染端遇到变宽 Emoji 自动"吞噬"右侧紧邻空 Cell，视觉起算点顺延实际物理宽度
  - [ ] 动态光标（Smart Cursor）：光标宽度动态读取所在 Glyph Cluster 的测量宽度

</details>

<details open>
<summary><b>📅 Milestone 3 · 架构健壮性与性能捍卫（规避重构风险）</b></summary>

> 解决变宽带来的遮挡问题，并防止纹理图集内存暴涨。

- **双通道渲染重构（Two-Pass Rendering）**
  - [ ] 重构 `gpu_context.rs` 的绘制提交：Pass 1 先铺背景色（`bg_rgba`）
  - [ ] Pass 2 纯粹绘制前景笔画与 Emoji 纹理（`glyph_contribution`），利用已有的 Premultiplied Alpha 让溢出笔画自然重叠在邻格背景之上，杜绝"文字被相邻背景吃掉"
- **动态多级纹理图集（Multi-Size Texture Atlas）**
  - [ ] 维持 Texture Array 渲染普通等宽字符（如 16×32 槽位）
  - [ ] 为变宽 Emoji / 复杂连字建立专用"大尺寸动态扩展纹理槽"，避免小字符占据大空间，杜绝 GPU 内存阶跃式暴涨
- **快速分支扫描（Fast-Path / Slow-Path）**
  - [ ] 进入 Rust Shaping 前对整行做纯 ASCII 预检
  - [ ] 普通代码 / 日志走原本 Fast-Path 单 Cell 渲染；仅当包含变宽 / 彩色字符时触发 Slow-Path Shaping，守住 Ridge 的极致性能下限

</details>

## 站点录制 / 截图

GitHub Pages 站点 (`site/`) 里的所有 demo 都是占位符。
要替换成真正的录屏，看 [`site/RECORDING.md`](./site/RECORDING.md)——
里面写了用什么录、录什么、放哪里。

## 协议 · License

本项目以 **MIT License** 开源。完整条款见 [`LICENSE`](./LICENSE)。

Released under the [MIT License](./LICENSE).

---

<div align="center">
<sub>Built with Tauri 2 · Svelte 5 · Rust · TypeScript</sub>
</div>
