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

## 站点录制 / 截图

GitHub Pages 站点 (`site/`) 里的所有 demo 都是占位符。
要替换成真正的录屏，看 [`site/RECORDING.md`](./site/RECORDING.md)——
里面写了用什么录、录什么、放哪里。

## 协议

[MIT](./LICENSE)（如未存在 LICENSE 文件，以 `package.json` 中声明的 MIT 为准）。

---

<div align="center">
<sub>Built with Tauri 2 · Svelte 5 · Rust · TypeScript</sub>
</div>
