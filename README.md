<div align="center">

<img src="./site/assets/ridge-mark.svg" width="64" height="64" alt="Ridge mark" />

# Ridge

**在终端里耕耘** · A modern terminal workbench with split panes, an embedded editor, Git visualisation, and Claude Code agent teams.

[Site](https://mysetsuna.github.io/ridge/) · [Docs](https://mysetsuna.github.io/ridge/docs.html) · [Releases](https://github.com/MySetsuna/ridge/releases) · [Issues](https://github.com/MySetsuna/ridge/issues)

</div>

---

## 田埂的隐喻

屏幕上的每一道分屏线都是一道**田埂**——把你的注意力切成可以独立耕耘的田块：
终端、编辑器、Git 图、Agent，各就各位，互不干扰，又共用同一片土地。

## 特性 · Highlights

- **递归分屏** — 水平 / 垂直 / 嵌套，无层数限制。每个 pane 独立 PTY、独立 cwd、独立滚回。
- **真·终端** — portable-pty + xterm.js (WebGL)。Unicode 11、超链接、64 KiB 块状滚回（4 MiB 上限）、OSC 7 cwd 同步。
- **嵌入式编辑器** — Monaco 作为另一种 pane 模式，与终端共享同一棵分屏树。
- **Git 可视化** — Canvas 提交图 + 每 pane 的 Git pill；SCM 状态由 `.git/` 文件监听驱动（无轮询）。
- **tmux 兼容 shim** — 内置自研 `tmux.exe`，让 Claude Code / 其它 tmux-aware 工具自然地把 Ridge 当成多终端会话。
- **多工作区** — 独立 PTY 命名空间，可同时打开多个项目。
- **零警告闸门** — `cargo build --lib` 0 warnings；`cargo clippy -- -D warnings` 在 CI 安全可跑。

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

[v0.1.0 · 开荒](https://github.com/MySetsuna/ridge/releases/tag/v0.1.0) — 2026-04-30 · 第一犁。
更新日志详见 [CHANGELOG.md](./CHANGELOG.md)。

## 站点录制 / 截图

GitHub Pages 站点 (`site/`) 里的所有 demo 都是占位符。
要替换成真正的录屏，看 [`site/RECORDING.md`](./site/RECORDING.md)——
里面写了用什么录、录什么、放哪里。

## 协议

[MIT](./LICENSE)（如未存在 LICENSE 文件，以 `package.json` 中声明的 MIT 为准）。

---

<div align="center">
<sub>built with Tauri v2 · Svelte 5 · Tailwind v4 · Rust · TypeScript</sub>
</div>
