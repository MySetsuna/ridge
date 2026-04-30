# Ridge Site Content Brief

作者：`ui-designer`（团队 `ridge-site-revamp` 任务 #1）
范围：`site/index.html` · `site/docs.html` · `site/releases.html` · `site/404.html` · `README.md` · `CHANGELOG.md`

本文件提供：
1. 情感化文案 → 中性替代（对照表）
2. 技术内幕措辞 → 用户可读措辞（对照表）
3. `releases.html` 全文重写
4. `docs.html` 新版信息架构（含每节实际正文，可直接粘贴）
5. `CHANGELOG.md` v0.1.0 全文重写
6. `README.md` Highlights 段落重写

---

## 总体语调原则

- **声明式、平静、专业**。不卖弄、不抒情、不押韵。
- 不再使用「田埂 / 田块 / 耕耘 / 开荒 / 第一犁 / 翻开 / 一片地 / 一块田 / 一垄」这一组隐喻作为承重文案。
- 「Ridge」作为品牌名保留。允许一处轻量的命名解释——放在 `docs.html` 末尾的 "About / Under the hood" 段落中（一句话），其余地方一律用直白措辞。
- "Built with" 仅在主页页脚出现一次，仅列框架名：`Tauri · Svelte · Rust · TypeScript`。其它库名、内部协议名、IPC 事件名、字节阈值、文件监听细节、Cargo 闸门、内部 commit / round 编号，**不在面向用户的页面出现**。
- 不写 emoji。

---

## 1. Tone reset（去情感化）

> 表内 "位置" 用文件 + 行号定位。"Proposed" 一栏给出可直接使用的替换文本；如改动结构，会在备注栏说明。

### 1.1 `site/index.html`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 6 `<title>` | `Ridge — 在终端里耕耘` | `Ridge — 分屏终端工作台` | |
| line 7 `<meta description>` | `Ridge：分屏终端 + 嵌入式编辑器 + Git 可视化 + 智能体协作。像田埂一样划分你的工作面，让每一块地都被认真耕耘。` | `Ridge 是一个桌面终端工作台：递归分屏终端、内嵌代码编辑器、Git 提交图，可与 Claude Code 智能体协作。Windows / macOS / Linux。` | |
| line 10 `og:title` | `Ridge — 在终端里耕耘` | `Ridge — 分屏终端工作台` | 与 `<title>` 对齐 |
| line 11 `og:description` | `像田埂一样划分你的工作面：分屏终端、嵌入式编辑器、Git 可视化、Claude Code 智能体团队。` | `分屏终端、内嵌编辑器、Git 提交图、Claude Code 智能体协作，集中在一个桌面应用中。` | |
| line 47–49 hero `<h1>` | `像田埂一样<br/><span class="accent">划分</span>你的<span class="soil">工作面</span>。` | `把终端、编辑器和 Git<br/>放进同一个<span class="accent">工作台</span>。` | 保留两行 + 重点色 span 的视觉结构 |
| line 50–54 hero `<p>` | `Ridge 是一个原生分屏终端 + 嵌入式编辑器 + Git 可视化的桌面工作台。屏幕上的每一道分割线都是一道田埂，把你的注意力切成可以独立耕耘的田块——终端、编辑器、Agent，各就各位。` | `Ridge 是一个本地桌面应用，提供递归分屏终端、内嵌代码编辑器和 Git 提交图。每个分屏都是独立的会话，可以单独运行命令、编辑文件，或交给 Claude Code 协作完成。` | 移除 "田埂 / 各就各位" |
| line 76 `.field-titlebar` | `~/code/ridge · 4 plots` | `~/code/ridge · 4 panes` | mockup 里的 `plots` 改 `panes` |
| line 80 `.label` | `PLOT · TERM` | `PANE · TERMINAL` | |
| line 86 `.label` | `PLOT · GIT` | `PANE · GIT` | |
| line 93 `.label` | `PLOT · EDITOR` | `PANE · EDITOR` | |
| line 99 `.label` | `PLOT · AGENT` | `PANE · AGENT` | |
| line 94 mock code | `fn cultivate() {` | `fn render() {` | 中性示例函数名 |
| line 95 mock code | `&nbsp;&nbsp;let plot = ...;` | `&nbsp;&nbsp;let pane = ...;` | |
| line 110 `.ridge-divider` | `田埂　Ridge　Ridge　田埂` | 整个 `<div class="ridge-divider">` 节点删除 | divider 视觉留白即可，不需要文字 |
| line 116 `<h2>` | `每一块地，都被认真耕耘。` | `为日常开发流程而设计。` | |
| line 117 `<p>` | `从最小的 PTY 到最复杂的多 Agent 协作流，Ridge 把所有需要的工具铺到同一片田里。` | `Ridge 把命令行、代码编辑、版本控制和智能体协作整合到同一个窗口，减少在多个独立工具之间切换的成本。` | |
| line 126 (递归分屏 `<p>`) | `水平、垂直、嵌套切分都不限层数。每一块地都是独立的会话——独立 PTY、独立 cwd、独立滚动历史。` | `水平、垂直、嵌套切分都不限层数。每个分屏都是独立的终端会话，拥有自己的工作目录与命令历史。` | |
| line 140 (嵌入式编辑器 `<p>`) | `Monaco 编辑器作为另一种 Pane 模式：与终端共享同一棵分屏树，文件、命令、AI 协作在同一片田里来回。` | `代码编辑器与终端共享同一套分屏布局：可以把任意分屏切换为编辑器模式，文件、命令与智能体回应保持在同一视野内。` | |
| line 170 `<h2>` | `看一眼就明白。` | `演示。` | |
| line 171 `<p>` | `下面的位置都是录制位——内容会在你录好后自动出现。` | `下面四组截图位用于呈现核心场景，正式录屏到位后会自动替换占位图。` | |
| line 176 eyebrow | `Plot 1 · Split` | `Scene 1 · Split` | |
| line 177 `<h2>` | `切一刀，又是一片地。` | `按需分屏。` | |
| line 178 `<p>` | `Ctrl+\ 横切，Ctrl+- 竖切，递归层数不限。每条分割线都是一道田埂。` | `Ctrl + \ 水平切分，Ctrl + - 垂直切分，可以无限嵌套。` | |
| line 183 `<li>` | `Pane 标题栏显示 cwd + Git 状态` | `分屏标题栏显示当前目录与 Git 状态` | |
| line 201 eyebrow | `Plot 2 · Editor` | `Scene 2 · Editor` | |
| line 202 `<h2>` | `编辑器，是另一种 Pane。` | `编辑器是另一种分屏模式。` | |
| line 203 `<p>` | `资源管理器树 + Monaco 编辑器，与终端共用同一棵分屏树。打开文件就像在田里多种了一垄。` | `内置文件树与代码编辑器，与终端共享同一套分屏布局。打开文件不会跳出当前窗口。` | |
| line 207 `<li>` | `Search 标签：跨工作区并行 grep + glob 过滤` | `搜索面板：跨工作区并行查找，支持 glob 过滤与替换` | |
| line 226 eyebrow | `Plot 3 · Git` | `Scene 3 · Git` | |
| line 227 `<h2>` | `历史的纹路，看得见。` | `直接查看分支历史。` | |
| line 228 `<p>` | `Canvas 绘制的提交图，分支拓扑一目了然。每个 Pane 标题栏的 Git Pill 即时显示 ahead/behind。` | `提交图直接渲染分支拓扑。每个分屏标题栏会显示当前分支与 ahead / behind 数量。` | |
| line 230 `<li>` | `Git 状态由 .git/ 文件监听驱动（无轮询）` | `仓库变更后状态自动刷新` | |
| line 232 `<li>` | `linked worktree 自动识别` | `自动识别 git worktree` | |
| line 251 eyebrow | `Plot 4 · Agents` | `Scene 4 · Agents` | |
| line 252 `<h2>` | `把 Agent 也安一块田。` | `与 Claude Code 协作。` | |
| line 253 `<p>` | `内置的 tmux shim 让 Claude Code 直接把 Ridge 当作多终端会话。多个 Agent 并行耕作，互不打扰。` | `Ridge 兼容 Claude Code 的多分屏会话协议。可以让多个智能体并行在不同分屏上工作，输出互不干扰。` | |
| line 255 `<li>` | `tmux shim 暴露 list-panes / display-message / kill-pane` | `智能体可以列出、命名、关闭分屏` | |
| line 256 `<li>` | `<code>RIDGE_TEAMMATE_URL/TOKEN</code> 注入到 PTY` | `从分屏内启动智能体，环境变量自动就绪` | |
| line 257 `<li>` | `list-panes 返回 cwd，Agent 自然知道在哪片田工作` | `智能体可以查询每个分屏当前的工作目录` | |
| line 279 `<h2>` | `从零到第一块田，2 分钟。` | `两分钟跑起来。` | |
| line 280 `<p>` | `需要 Node 18+, pnpm, 以及 Rust 工具链 (1.77+).` | `源码构建需要 Node 18+、pnpm 9+、Rust 1.77+。` | |
| line 329 `<h2>` | `第一犁地。` | `首个公开版本。` | |
| line 330 `<p>` | `开荒之版本——所有特性的起点。` | `Ridge 0.1.0 是首个对外发布的版本，奠定了分屏、终端、编辑器、Git 与智能体协作的基础体验。` | |
| line 335 v0.1.0 标签后缀 | `<span ...>"开荒"</span>` | 整个 span 删除 | 副标题不要 |
| line 361 footer | `在终端里耕耘 · MIT License` | `分屏终端工作台 · MIT License` | |

### 1.2 `site/docs.html`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 71 `<p>` | `Ridge 是一个本地终端工作台。它把分屏终端、嵌入式编辑器、Git 可视化、智能体协作四样东西放进同一个原生应用里——四块田地，由一道道田埂隔开。` | `Ridge 是一个本地桌面应用，把分屏终端、代码编辑器、Git 工具与智能体协作整合到一个窗口中。` | |
| line 104 `<h2>` | `Pane / 田块` | `分屏（Pane）` | |
| line 95 `<p>` | `启动后会看到一块默认的「田」——一个全屏的终端 Pane。试试这些动作：` | `启动后会看到一个默认的全屏终端分屏。可以试试以下操作：` | |
| line 235 footer | `在终端里耕耘 · MIT License` | `分屏终端工作台 · MIT License` | |

> 注：`docs.html` 的章节结构整体替换（见第 4 节信息架构）。表格中只列保留沿用的部分需要替换的文案。

### 1.3 `site/releases.html`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 36 `<h1>` | `开荒 → 耕作 → 收成` | `发布记录` | |
| line 37–39 `<p>` | `每一次 release 都是一次播种。最早的版本是开荒——把田埂铺到位；之后每一次只在熟地上添新作物。` | `所有正式发布记录，按版本倒序排列。GitHub Releases 上同时提供安装包与源码归档。` | |
| line 54 v 标签后缀 | `"开荒 · Breaking Ground"` | 整个 span 删除 | |
| line 63 `<p>` | `Ridge 的第一犁。把田埂铺到位、把 PTY 跑起来、把 Agent 接上、把零警告闸门拉上。` | `Ridge 的首个公开版本。在这一版里，分屏终端、代码编辑器、Git 提交图与智能体协作首次同时可用。` | |
| line 126 `.ridge-divider` | `下一犁　Next furrow` | `下一版本　Next release` | |
| line 135 footer | `在终端里耕耘 · MIT License` | `分屏终端工作台 · MIT License` | |

> 整页的 release notes 主体（第 65–116 行的全部 `<h4>` 与 `<ul>`）按本简报第 3 节重写。

### 1.4 `site/404.html`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 16 `<h1>` | `这块田还没翻开。` | `页面没找到。` | |
| line 17 `<p>` | `你找的页面不在这块地里。回到主路或者去看看别的田。` | `这个地址在 Ridge 站点上不存在。请回主页或前往文档。` | |

### 1.5 `README.md`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 7 | `**在终端里耕耘** · A modern terminal workbench with split panes, an embedded editor, Git visualisation, and Claude Code agent teams.` | `**分屏终端工作台。** A modern terminal workbench with split panes, an embedded editor, Git visualisation, and Claude Code agent teams.` | |
| line 15–18 「田埂的隐喻」整段 | `## 田埂的隐喻\n\n屏幕上的每一道分屏线都是一道**田埂**——把你的注意力切成可以独立耕耘的田块：终端、编辑器、Git 图、Agent，各就各位，互不干扰，又共用同一片土地。` | 整段删除（保留 `---` 分隔线即可） | 名称解释挪到下文「为什么叫 Ridge?」一行（可选） |
| line 57 | `[v0.1.0 · 开荒](https://github.com/MySetsuna/ridge/releases/tag/v0.1.0) — 2026-04-30 · 第一犁。` | `[v0.1.0](https://github.com/MySetsuna/ridge/releases/tag/v0.1.0) — 2026-04-30 · 首个公开版本。` | |

> Highlights 段落重写见第 6 节。

### 1.6 `CHANGELOG.md`

| Location | Current | Proposed | 备注 |
|---|---|---|---|
| line 10 `## [0.1.0]` 标题 | `## [0.1.0] — 2026-04-30 · 「开荒 / Breaking Ground」` | `## [0.1.0] — 2026-04-30` | |
| lines 12–13 引言 | `The first public release of Ridge. The plot lines are laid; from here on every furrow is cultivation.` | `The first public release of Ridge.` | |

> 主体重写见第 5 节。

### 1.7 唯一保留的「品牌轻触」

允许在 `docs.html` 新增的 "About / Under the hood" 末尾，加一行（仅一行，不展开）：

> 名字 "Ridge" 取自田埂——分隔田地的窄堤。每一道分屏线都是一道 ridge。

不再有第二处。

---

## 2. Tech detail removal（去内部实现）

### 通则

- **保留**：操作系统、安装包后缀、入门所需的工具链版本（Node / pnpm / Rust）、用户可见的快捷键、用户可见的 UI 名称（"分屏 / 编辑器 / 文件树 / 提交图 / SCM 面板"）。
- **隐藏**：所有库名（`portable-pty`, `xterm.js`, `WebGL`, `parking_lot`, `Monaco`, `Tauri 2`, `tokio`, `walkdir`, `overlayscrollbars`…）、所有字节阈值、IPC 事件名、文件监听机制名、内部命令名、`OSC 7`、`linked worktree` 这种 git 内部术语、Cargo 闸门、`round N` 编号、`tmux.exe shim` 这一实现机制。
- **"Built with" 例外**：主页页脚单独加一行 `Built with Tauri · Svelte · Rust · TypeScript`，仅四个名字，不带版本号、不带库名。

### 2.1 `site/index.html`

| Location | Current | Proposed |
|---|---|---|
| line 45 eyebrow | `v0.1.0 · Tauri + Svelte 5 · MIT` | `v0.1.0 · MIT License` |
| line 67 `.meta-row` | `<i class="dot"></i> Windows / macOS / Linux` | 保留 |
| line 68 `.meta-row .mono` | `portable-pty · xterm.js · Monaco` | 整个 `<span class="mono">` 删除（"Built with" 单独放页脚） |
| line 132 (真·终端模拟 `<h3>`) | `真·终端模拟` | `稳定的本地终端` |
| line 133 (`<p>`) | `portable-pty + xterm.js + WebGL 渲染。Unicode 11、超链接、64 KiB 块状滚回 (4 MiB 上限)、OSC 7 cwd 同步全部到位。` | `支持 Unicode、可点击超链接、可滚动数 MB 的命令历史。无论是 PowerShell、bash、zsh 还是 cmd，体验与原生终端一致。` |
| line 153 (tmux 兼容 Shim `<h3>`) | `tmux 兼容 Shim` | `Claude Code 智能体协作` |
| line 154 (`<p>`) | `内置自研 <code>tmux.exe</code>，让 Claude Code 等工具误以为自己在 tmux 里。Agent 团队、跨 Pane RPC 自然接入。` | `从 Ridge 内的分屏启动 Claude Code，可以直接以多分屏模式协作：智能体能列出、命名、新建、关闭分屏，并查询每个分屏的工作目录。` |
| line 161 (`<p>`) | `每个工作区独立 PTY 进程、独立 Pane ID 命名空间，可同时打开多个项目；侧栏 Search 平行扫描所有 cwd。` | `每个工作区有独立的进程与命令历史，可同时打开多个项目；侧栏搜索会并行扫描所有打开中的目录。` |
| line 230 `<li>` | `Git 状态由 .git/ 文件监听驱动（无轮询）` | `仓库变更后即时刷新，无需手动重载` |
| line 232 `<li>` | `linked worktree 自动识别` | `自动识别 git worktree` |
| line 255 `<li>` | `tmux shim 暴露 list-panes / display-message / kill-pane` | `智能体可以查询、命名、新建、关闭分屏` |
| line 256 `<li>` | `<code>RIDGE_TEAMMATE_URL/TOKEN</code> 注入到 PTY` | `从分屏启动的智能体自动取得连接凭证` |
| line 257 `<li>` | `list-panes 返回 cwd，Agent 自然知道在哪片田工作` | `每个分屏的工作目录可被智能体读取` |
| line 313–321 安装包 tab `<div class="code-block">` | 包含 `# tmux shim 会随安装包附带，PATH 自动注册` 与 `tmux -V` / `ridge-tmux-shim/0.1.0` 验证步骤 | 删除 tmux shim 相关注释与验证行；只保留下载 + 双击安装两步。如下文：<br>`# 从 Releases 页面下载`<br>`$ open https://github.com/MySetsuna/ridge/releases/tag/v0.1.0`<br>`# Windows: 双击 .msi 或 .exe 安装即可`<br>`# macOS / Linux: v0.1.0 暂未提供官方二进制，请参考"开发模式"自行构建` |
| line 345 `<li>` | `递归分屏（水平 / 垂直 / 嵌套）+ 多工作区` | `递归分屏与多工作区` |
| line 346 `<li>` | `portable-pty + xterm.js (WebGL) 终端，4 MiB 块状滚回` | `稳定的终端体验，可滚动数 MB 的命令历史` |
| line 347 `<li>` | `Monaco 编辑器作为另一种 Pane 模式` | `内置代码编辑器，与终端共享分屏布局` |
| line 348 `<li>` | `Canvas Git Graph + 实时 SCM 状态（基于 .git/ 文件监听）` | `Git 提交图与实时仓库状态` |
| line 349 `<li>` | `内置 tmux shim，Claude Code Agent 团队即插即用` | `开箱即用的 Claude Code 智能体协作` |
| line 350 `<li>` | `三套主题 + 字体下拉框；Cargo 零警告闸门` | `多套主题、可切换编辑器字体` |
| footer 新增一行 | — | 在 `.foot-inner` 内新增一个 `<div class="muted" style="font-size:12px;font-family:var(--font-mono)">Built with Tauri · Svelte · Rust · TypeScript</div>` |

### 2.2 `site/docs.html`

整页的内容主体（架构、IPC、滚回、SCM 监听、tmux shim 协议）按第 4 节信息架构重写。下表只保留涉及现存少量保留章节的清理：

| Location | Current | Proposed |
|---|---|---|
| line 76 `.callout` | `这里只是入门。完整内部文档（PTY 实现、滚回设计、Agent 协议）见仓库 <a><code>/docs</code></a>。` | `这是面向用户的使用文档。开发者文档与实现细节在 <a>仓库</a> 内。` |

新版整页的全部正文见第 4 节，按节给出可粘贴文本。**原文 line 117–199（Pane / 分屏树 / 工作区 / Architecture / IPC / Scrollback / Search / SCM / Agent 团队）整段替换。**

### 2.3 `site/releases.html`

整段 release notes 重写见第 3 节。技术细节如 `OSC 7`, `parking_lot`, `64 KiB`, `4 MiB`, `seq byte counter`, `linked worktree`, `round 64`, `cargo build --lib`, `WixNSIS`, `RIDGE_TEAMMATE_URL/TOKEN` 等全部从用户面前消失。

### 2.4 `README.md`

Highlights 段落（第 22–28 行）重写见第 6 节。技术细节如 `portable-pty + xterm.js (WebGL)`, `64 KiB 块状滚回（4 MiB 上限）`, `OSC 7 cwd 同步`, `.git/ 文件监听`, `tmux.exe`, `Cargo 零警告闸门`, `cargo clippy -- -D warnings` 等全部移除。最末的 `<sub>built with Tauri v2 · Svelte 5 · Tailwind v4 · Rust · TypeScript</sub>` 简化为 `<sub>Built with Tauri · Svelte · Rust · TypeScript</sub>`。

---

## 3. `releases.html` 全文重写

将 `<div class="release-list">` 内 v0.1.0 整块替换为以下内容（保留外层 `<div class="release">` 容器与 CSS class）：

```html
<div class="release">
  <div class="release-head">
    <div class="release-tag"><span class="v">v</span>0.1.0</div>
    <div class="release-meta">
      <span class="pill first">FIRST RELEASE</span>
      <span>2026-04-30</span>
      <a href="https://github.com/MySetsuna/ridge/releases/tag/v0.1.0" target="_blank" rel="noopener">在 GitHub 上查看 →</a>
    </div>
  </div>

  <p>Ridge 的首个公开版本。在这一版里，分屏终端、代码编辑器、Git 提交图与智能体协作首次同时可用。</p>

  <h4>新增 · New</h4>
  <ul>
    <li>递归分屏：水平、垂直、可嵌套，无层级限制；分屏可关闭、可拖拽改变大小。</li>
    <li>多工作区：可同时打开多个项目，每个工作区独立保留命令历史，切换不会中断后台进程。</li>
    <li>稳定的终端体验：支持 PowerShell / bash / zsh / cmd；Unicode、可点击超链接、长达数 MB 的可滚动命令历史。</li>
    <li>内嵌代码编辑器：可以把任意分屏切换为编辑器模式，文件、命令与 AI 输出在同一窗口内。</li>
    <li>文件浏览器：左侧栏文件树支持新建、重命名、删除、拖拽、键盘导航；右键菜单可在系统资源管理器中定位文件。</li>
    <li>跨工作区搜索：左侧栏搜索面板并行扫描所有打开的目录，支持大小写敏感、整词匹配、正则、glob 过滤、批量替换。</li>
    <li>Git 提交图：直接渲染分支拓扑，可在分支选择器中切换或新建分支。</li>
    <li>分屏 Git 状态徽章：每个分屏标题栏显示当前分支、ahead / behind 数量与未提交改动数。</li>
    <li>SCM 面板：浏览改动、暂存、提交、查看 diff。</li>
    <li>Claude Code 智能体协作：从 Ridge 内的分屏启动智能体即可以多分屏模式工作；智能体可以列出、命名、新建、关闭分屏，并查询每个分屏当前的工作目录。</li>
    <li>主题与字体：内置三套配色与可选编辑器字体下拉。</li>
    <li>历史回放窗口：在每个分屏的标题栏可以打开历史窗口，浏览、搜索此前滚出屏幕的命令输出。</li>
  </ul>

  <h4>改进 · Improved</h4>
  <ul>
    <li>仓库状态变化时自动刷新，无需手动重载或周期轮询。</li>
    <li>自动识别 git worktree 链接，子工作树的状态与主仓库一致。</li>
    <li>所有确认 / 输入对话框使用与 Ridge 自身风格一致的窗口，避免操作系统弹窗打断专注。</li>
    <li>Windows 路径在前后端统一为正斜杠，资源管理器的目录列不会出现重复行。</li>
  </ul>

  <h4>已知限制 · Known</h4>
  <ul>
    <li>v0.1.0 只在 Windows 提供官方安装包；macOS / Linux 用户需要从源码构建。</li>
    <li>智能体协作目前以 Claude Code 为目标客户端，其它兼容多分屏会话协议的工具未做完整测试。</li>
    <li>站点上的演示截图与录屏正在补录中，部分仍是占位图。</li>
  </ul>

  <p style="margin-top:22px;color:var(--mist-soft);font-family:var(--font-mono);font-size:13px">
    完整 commit 记录见 <a href="https://github.com/MySetsuna/ridge/commits/v0.1.0" target="_blank" rel="noopener">git log</a>
  </p>
</div>
```

---

## 4. `docs.html` 信息架构（IA）

### 4.1 新版侧栏

```
Getting started
  · Install
  · First run
Working with terminals
  · Splitting & navigating
  · Scrollback & history
Working with files
  · Explorer
  · Editor
  · Search across panes
Working with Git
  · Commit graph
  · Branch & status
  · Stage & commit
Using Claude Code agents
Keyboard shortcuts
Troubleshooting
About / Under the hood
```

对应 `<aside class="docs-side">` HTML（替换原侧栏整块）：

```html
<aside class="docs-side">
  <h4>Getting started</h4>
  <ul>
    <li><a href="#install">Install</a></li>
    <li><a href="#first-run">First run</a></li>
  </ul>
  <h4>Working with terminals</h4>
  <ul>
    <li><a href="#terminals-split">Splitting &amp; navigating</a></li>
    <li><a href="#terminals-scrollback">Scrollback &amp; history</a></li>
  </ul>
  <h4>Working with files</h4>
  <ul>
    <li><a href="#files-explorer">Explorer</a></li>
    <li><a href="#files-editor">Editor</a></li>
    <li><a href="#files-search">Search across panes</a></li>
  </ul>
  <h4>Working with Git</h4>
  <ul>
    <li><a href="#git-graph">Commit graph</a></li>
    <li><a href="#git-status">Branch &amp; status</a></li>
    <li><a href="#git-commit">Stage &amp; commit</a></li>
  </ul>
  <h4>Agents</h4>
  <ul>
    <li><a href="#agents">Claude Code 协作</a></li>
  </ul>
  <h4>Reference</h4>
  <ul>
    <li><a href="#shortcuts">Keyboard shortcuts</a></li>
    <li><a href="#trouble">Troubleshooting</a></li>
  </ul>
  <h4>About</h4>
  <ul>
    <li><a href="#about">Under the hood</a></li>
  </ul>
</aside>
```

### 4.2 各节正文（可直接粘贴到 `<article class="docs-content">`）

> 全部为正式发布稿。每节 1–2 段，引述事实均与 `CLAUDE.md` 对照过，但措辞改为用户视角。

#### `#install` Install

```html
<h1 id="install">安装</h1>
<p>Ridge 是一个桌面应用，提供 Windows 安装包；macOS 与 Linux 用户当前需要从源码构建。所有平台均可使用同一份代码。</p>

<h3>从安装包安装（Windows）</h3>
<p>访问 <a href="./releases.html">Releases 页面</a>，下载与系统匹配的安装包：</p>
<ul>
  <li><code>ridge_0.1.0_x64-setup.exe</code> — NSIS 安装程序</li>
  <li><code>ridge_0.1.0_x64_en-US.msi</code> — MSI 安装程序</li>
</ul>
<p>双击运行，按提示完成即可。安装结束后会在「开始」菜单看到 Ridge。首次启动可能需要授予网络权限（Ridge 会在本地启动一个供智能体使用的 HTTP 端口）。</p>

<h3>从源码构建（所有平台）</h3>
<p>需要：</p>
<ul>
  <li>Node.js 18 或更新版本</li>
  <li>pnpm 9 或更新版本</li>
  <li>Rust 1.77 或更新版本</li>
  <li>Windows 还需要：MSVC Build Tools 与 WebView2 运行时</li>
  <li>Linux 还需要：webkit2gtk、libssl-dev 等系统依赖（参考 Tauri 官方安装清单）</li>
</ul>
<pre class="code-block"><span class="prompt">$</span> <span class="kw">git</span> clone <span class="str">https://github.com/MySetsuna/ridge.git</span>
<span class="prompt">$</span> <span class="kw">cd</span> ridge
<span class="prompt">$</span> <span class="kw">pnpm</span> install
<span class="prompt">$</span> <span class="kw">pnpm</span> tauri build   <span class="cmt"># 输出可执行文件 + 安装包到 src-tauri/target/release/bundle/</span></pre>
```

#### `#first-run` First run

```html
<h2 id="first-run">第一次运行</h2>
<p>启动后会看到一个全屏的终端分屏。这是 Ridge 的最小工作单元：每个分屏都是一个独立的终端会话，有自己的工作目录、命令历史与 Git 状态。</p>
<p>建议先做这几件事：</p>
<ul>
  <li>按 <code>Ctrl + \</code> 把当前分屏水平切分成两个，按 <code>Ctrl + -</code> 垂直切分。</li>
  <li>在左上角点击工作区下拉，选择「打开文件夹…」加载一个项目。Ridge 会以这个目录作为新工作区。</li>
  <li>左侧栏顶部三个图标分别是：文件浏览器、源代码管理（Git）、搜索。</li>
  <li>每个分屏的标题栏右侧有几个按钮：当前分支徽章（点击切换 / 新建分支）、Bot（启动 Claude Code 智能体）、History（查看历史输出）、× 关闭。</li>
</ul>
<p>所有界面元素的位置都是固定的，不需要先做配置就能开始用。</p>
```

#### `#terminals-split` Splitting & navigating

```html
<h1 id="terminals-split">使用终端</h1>
<h2>分屏与切换</h2>
<p>Ridge 的核心工作模式是任意嵌套的分屏。每次切分都把当前分屏一分为二，可以是水平或垂直方向，没有层级上限。</p>
<ul>
  <li><code>Ctrl + \</code> 水平切分（左右排列）</li>
  <li><code>Ctrl + -</code> 垂直切分（上下排列）</li>
  <li><code>Ctrl + W</code> 关闭当前分屏</li>
  <li>用鼠标拖拽分屏之间的分隔线可以调整大小；双击分隔线复位为均分。</li>
</ul>
<p>每个分屏完全独立——切换 shell、运行长任务、按 <code>Ctrl + C</code> 中断，都不会影响其它分屏。关闭某个分屏时，对应的进程会被一并结束。</p>
<p>分屏标题栏会显示当前的工作目录与前台进程名。当 shell 切换目录（例如执行 <code>cd</code>）时，标题栏会自动同步。</p>
```

#### `#terminals-scrollback` Scrollback & history

```html
<h2 id="terminals-scrollback">滚动历史与历史回放</h2>
<p>每个分屏会保留最近的命令输出，可以用滚轮、<code>Shift + PageUp / PageDown</code> 或者 <code>Shift + 上 / 下</code> 翻看。容量按字节计算，足以容纳数 MB 的输出，老内容会按队列丢弃最早的部分。</p>
<p>如果需要查看更久之前的输出（例如已经被新输出推走的内容），点击分屏标题栏的 <strong>History</strong> 按钮，会打开一个独立的历史回放窗口。这个窗口默认载入最新的 256 KiB 文本，可以按「加载更早」继续往前翻；附带搜索框（支持区分大小写、上一项 / 下一项跳转），并自动剥离 ANSI 转义码方便复制。</p>
```

#### `#files-explorer` Explorer

```html
<h1 id="files-explorer">使用文件</h1>
<h2>文件浏览器</h2>
<p>左侧栏第一个图标打开文件浏览器。每个工作区下会按当前活跃过的目录形成一列；点击文件夹展开，点击文件直接在编辑器中打开。</p>
<p>文件浏览器支持：</p>
<ul>
  <li>键盘导航：上下方向键移动焦点，Enter 打开，左 / 右收起或展开目录，Home / End 跳到首尾。</li>
  <li>右键菜单：新建文件、新建文件夹、重命名、删除、复制路径、在系统资源管理器中显示。</li>
  <li>行内重命名 / 新建：按 F2 或在右键菜单选择「重命名」会把名称变成可编辑输入框，Enter 提交，Esc 取消。新建的输入框会出现在所选目录顶端。</li>
  <li>展开状态记忆：每列的展开 / 选中状态会在重启后恢复。</li>
</ul>
```

#### `#files-editor` Editor

```html
<h2 id="files-editor">代码编辑器</h2>
<p>Ridge 内置的代码编辑器和终端共享同一套分屏布局：可以把任意分屏切换为编辑器模式，也可以同时存在多个编辑器分屏。</p>
<p>编辑器顶部是文件标签栏，支持横向滚动；标签可以拖拽排序、按 <code>Ctrl + W</code> 关闭。语言识别、语法高亮、括号匹配、查找替换等基础能力均开箱可用。可以在设置里切换主题与编辑器字体。</p>
```

#### `#files-search` Search across panes

```html
<h2 id="files-search">跨分屏搜索</h2>
<p>左侧栏第三个图标（也可按 <code>Ctrl + Shift + F</code>）打开搜索面板。它会同时扫描当前打开的所有工作区与分屏所在的目录——你不需要先选择「在哪里搜」。</p>
<p>支持的开关与过滤：</p>
<ul>
  <li>大小写敏感、全词匹配、正则三个独立开关。</li>
  <li>包含 / 排除路径用 glob 写法（例如 <code>**/*.ts</code>、<code>!node_modules/**</code>）。</li>
  <li>展开「替换」行后，可以执行批量替换；替换会按仓库根分组提交，避免跨项目误改。</li>
  <li>搜索为输入防抖触发；按 Enter 立刻执行并取消防抖。点击结果会跳到对应行列。</li>
</ul>
```

#### `#git-graph` Commit graph

```html
<h1 id="git-graph">使用 Git</h1>
<h2>提交图</h2>
<p>左侧栏第二个图标打开源代码管理面板。顶部是当前仓库的提交图，按时间倒序绘制分支拓扑：每个圆点是一个 commit，颜色区分分支，连线表示父子关系。</p>
<p>提交图会随仓库变化自动刷新——执行 <code>git commit</code>、<code>git fetch</code>、切换分支、暂存改动等操作后，无需手动按刷新键。</p>
```

#### `#git-status` Branch & status

```html
<h2 id="git-status">分支与状态徽章</h2>
<p>每个分屏标题栏右侧有一个徽章显示当前分支、未提交改动数、与远端的 ahead / behind 差。</p>
<ul>
  <li>点击徽章打开分支选择器，可以切换到任一现有分支。</li>
  <li>选择器底部有「+ 创建新分支…」，输入分支名按 Enter 即可（基于当前 HEAD）。</li>
  <li>按住 Ctrl 点击徽章会跳到源代码管理面板。</li>
</ul>
<p>同一仓库的多个分屏会共享一份缓存的状态，避免重复查询。</p>
```

#### `#git-commit` Stage & commit

```html
<h2 id="git-commit">暂存与提交</h2>
<p>源代码管理面板列出当前仓库的全部改动，分为「未暂存」「已暂存」两组。每个文件支持：</p>
<ul>
  <li>查看 diff（点击文件名）。</li>
  <li>暂存 / 取消暂存（点击 + / − 按钮）。</li>
  <li>放弃改动 / 删除新文件（右键菜单）。</li>
</ul>
<p>顶部输入框写提交信息，按 <code>Ctrl + Enter</code> 提交。提交后状态徽章与提交图都会即时更新。如果项目使用 git worktree（同一仓库的多个工作树），Ridge 会识别每个工作树独立的 HEAD 与索引。</p>
```

#### `#agents` Claude Code 协作

```html
<h1 id="agents">与 Claude Code 协作</h1>
<p>Ridge 兼容 Claude Code 的多分屏会话协议。从任一分屏内启动 <code>claude</code>（或在分屏标题栏点 Bot 按钮）后，智能体会把 Ridge 当成多分屏会话的容器——它可以新建分屏、命名分屏、列出所有分屏、读取每个分屏的工作目录，乃至关闭分屏。</p>
<p>这意味着：</p>
<ul>
  <li>可以让多个智能体同时运行在不同分屏上，互不干扰输出。</li>
  <li>智能体在另一个目录下的分屏跑命令时，工作目录会被正确识别。</li>
  <li>从 Ridge 启动的智能体自动获得所需的连接凭证，无需手动配置环境变量。</li>
</ul>
<p>Bot 按钮会弹出启动器，可选预设 prompt 或留空直接进入交互。按住 Shift 或 Alt 点击可跳过启动器、直接打开裸 <code>claude</code>。</p>
<p>建议从 PowerShell、cmd 或 Windows Terminal 风格的环境里启动 Claude Code，避免一些 shell 在转义参数时引入额外问题。</p>
```

#### `#shortcuts` Keyboard shortcuts

```html
<h1 id="shortcuts">快捷键</h1>
<table>
  <thead><tr><th>键位</th><th>动作</th></tr></thead>
  <tbody>
    <tr><td><code>Ctrl + \</code></td><td>水平切分当前分屏</td></tr>
    <tr><td><code>Ctrl + -</code></td><td>垂直切分当前分屏</td></tr>
    <tr><td><code>Ctrl + W</code></td><td>关闭当前分屏</td></tr>
    <tr><td><code>Ctrl + Tab</code></td><td>切换工作区</td></tr>
    <tr><td><code>Ctrl + Shift + F</code></td><td>打开跨分屏搜索</td></tr>
    <tr><td><code>Ctrl + B</code></td><td>显示 / 隐藏侧栏</td></tr>
    <tr><td><code>Shift + PageUp / PageDown</code></td><td>翻看终端历史</td></tr>
    <tr><td><code>Ctrl + C</code> / <code>Ctrl + V</code></td><td>复制 / 粘贴（需先选中文本）</td></tr>
    <tr><td><code>F2</code>（在文件浏览器）</td><td>重命名</td></tr>
    <tr><td><code>Delete</code>（在文件浏览器）</td><td>删除（带确认）</td></tr>
    <tr><td><code>Ctrl + Enter</code>（在 SCM 提交框）</td><td>提交</td></tr>
  </tbody>
</table>
```

#### `#trouble` Troubleshooting

```html
<h1 id="trouble">常见问题</h1>

<h3>启动后显示空白窗口（Windows）</h3>
<p>Ridge 在 Windows 依赖系统的 WebView2 运行时。如果安装的是不带 WebView2 的精简版 Windows，请到 Microsoft 官网安装 <em>Microsoft Edge WebView2 Runtime</em>。安装完毕重启 Ridge 即可。</p>

<h3>从源码构建时 Rust 编译失败</h3>
<ul>
  <li>确认 Rust 工具链版本不低于 1.77（<code>rustc --version</code>）。</li>
  <li>Windows 上需要 MSVC（Visual Studio Build Tools 中的 "Desktop development with C++"）；不能只装 GNU 工具链。</li>
  <li>Linux 上参考 Tauri 官方文档的系统依赖清单（<code>webkit2gtk-4.1</code>、<code>libssl-dev</code> 等）。</li>
</ul>

<h3>从源码构建时前端依赖失败</h3>
<p>使用 pnpm 9+。如果 <code>pnpm install</code> 卡住，先 <code>pnpm store prune</code> 再重试。</p>

<h3>Claude Code 启动后报错找不到分屏</h3>
<p>必须从 Ridge 自己的分屏内启动 <code>claude</code>，让它继承 Ridge 的环境。如果你在外部终端启动 Claude Code，它无法找到 Ridge 的本地协调端口，自然也看不到任何分屏。</p>

<h3>文件浏览器右键菜单没有反应</h3>
<p>菜单依赖于聚焦在文件树上：先用鼠标或 <code>Ctrl + B</code> 切到文件浏览器，再右键点击对应行。</p>

<h3>更多帮助</h3>
<p>仍未解决？欢迎在 <a href="https://github.com/MySetsuna/ridge/issues" target="_blank" rel="noopener">GitHub Issues</a> 提交报告。请附上操作系统版本、Ridge 版本（设置面板可见）以及复现步骤。</p>
```

#### `#about` Under the hood

```html
<h1 id="about">Under the hood</h1>
<p>Ridge 是一个原生桌面应用，使用 Tauri 作为外壳、Rust 处理终端与文件系统、Svelte 与 TypeScript 构建界面。源码以 MIT 协议在 <a href="https://github.com/MySetsuna/ridge" target="_blank" rel="noopener">GitHub</a> 开放。</p>
<p>名字 "Ridge" 取自田埂——分隔田地的窄堤。每一道分屏线都是一道 ridge。</p>
```

> 这是允许保留的唯一一处「品牌轻触」。整个 docs 内不再出现 田埂 / 田 / 耕作 / 开荒 字样。

---

## 5. `CHANGELOG.md` v0.1.0 重写

替换 `CHANGELOG.md` 第 10 行至 80 行（即整个 `## [0.1.0]` 段落，保留页脚 `[0.1.0]: …` 链接行）：

```markdown
## [0.1.0] — 2026-04-30

The first public release of Ridge.

### Added

- Recursive split panes — horizontal, vertical, nested without depth limit.
  Each pane is an independent terminal session with its own working directory
  and command history.
- Multi-workspace support. Each workspace keeps its own panes and processes
  alive when you switch away.
- Stable terminal experience across PowerShell, bash, zsh, and cmd. Unicode,
  clickable hyperlinks, scrollback that holds several megabytes of output.
- Embedded code editor as an alternative pane mode, sharing the same split
  layout as terminals.
- File explorer with create / rename / delete / drag-and-drop / keyboard
  navigation, plus "Reveal in file manager" via context menu.
- Cross-pane search panel — search and replace across every open workspace
  at once, with case / whole-word / regex toggles and glob filters.
- Git commit graph rendered directly from repository history, refreshing
  automatically when the working tree changes.
- Per-pane Git status badge showing branch, ahead / behind counts, and
  uncommitted change count, with an inline branch picker and "create branch"
  input.
- Source-control panel for staging, committing, and viewing diffs. Auto-detects
  git worktree links so the right HEAD is shown for each working tree.
- Claude Code agent collaboration — agents launched from a Ridge pane can
  list, name, create, and close panes, and read the working directory of any
  pane.
- Three built-in themes and a selectable editor font.
- Per-pane scrollback history viewer with search and "load older" paging.

### Improved

- Repository state refreshes from filesystem changes alone — no polling, no
  manual reload required.
- All confirm / input dialogs use Ridge's own window chrome, so prompts no
  longer interrupt the visual flow with native OS popups.
- File paths are normalised consistently across the app on Windows; the
  explorer no longer shows duplicate columns for the same directory.

### Known limitations

- Official installers for v0.1.0 are Windows-only. macOS and Linux users
  build from source.
- Agent collaboration is verified against Claude Code; other clients
  implementing the same multi-pane session protocol are not fully tested.
- Demo screenshots and recordings on the marketing site are still being
  captured; some are placeholders.
```

页脚 `[0.1.0]: https://github.com/MySetsuna/ridge/releases/tag/v0.1.0` 保留。

---

## 6. `README.md` 调整

替换第 13 行下方的 `## 田埂的隐喻` 整段（直至 `## 特性 · Highlights` 之前）——整段删除，不留替代标题，让分隔线 `---` 直接接到 `## 特性 · Highlights`。

替换第 20–28 行的 Highlights 段：

```markdown
## 特性 · Highlights

- **递归分屏** — 水平 / 垂直 / 嵌套，无层数限制。每个分屏都是独立的终端会话，有自己的工作目录与命令历史。
- **稳定的本地终端** — 支持 PowerShell / bash / zsh / cmd，Unicode、超链接、可滚动数 MB 的命令历史。
- **内嵌代码编辑器** — 与终端共享同一套分屏布局，可同时打开多个编辑器。
- **Git 可视化** — 提交图、分支选择器、分屏内的状态徽章，仓库变更后自动刷新。
- **跨分屏搜索** — 一次扫描所有打开的目录，支持正则、glob、批量替换。
- **Claude Code 智能体协作** — 从 Ridge 启动的智能体即获得多分屏协作能力。
- **多工作区** — 同时打开多个项目，互不干扰。
```

可选：在 README 末尾的 `## 协议` 段之上插入一行：

```markdown
> "Ridge" 取自田埂——分隔田地的窄堤。每一道分屏线都是一道 ridge。
```

> 这是 README 内允许保留的唯一品牌解释。

第 73 行 `built with Tauri v2 · Svelte 5 · Tailwind v4 · Rust · TypeScript` 简化为 `Built with Tauri · Svelte · Rust · TypeScript`。

---

## 附录 A：开放问题（请 team-lead 决定）

1. **品牌轻触放在哪里？** 本简报建议放在 `docs.html` 的 About 段尾（一句话）和 `README.md` 协议段前（一句话）。如果只允许全站一处，建议保留 `docs.html` 的版本，删除 README 的版本。

2. **`<title>` 与 `og:title` 的统一文案。** 本简报提议 `Ridge — 分屏终端工作台`。如果你觉得太朴素，可备选：
   - `Ridge — 终端、编辑器、Git 与智能体协作的桌面工作台`（更长，信息密度高）
   - `Ridge — 一个分屏式终端工作台`（口语化）

3. **showcase 区的 eyebrow 是否要改？** 现在用 `Plot 1 / 2 / 3 / 4`，本简报提议改为 `Scene 1 / 2 / 3 / 4`。如果想完全英文化保持一致，也可以改成 `01 · Split` / `02 · Editor` 等更克制的编号。

4. **首页发布段标题。** 当前 `<h2>第一犁地。</h2>` 我提议替换为 `首个公开版本。`。如果你希望与 `releases.html` 联动，也可换成 `Latest release.` 或 `最新版本：v0.1.0`。

5. **footer 行中是否保留 "MIT License" 的字样？** 简报里默认保留，如有更简洁偏好可改成 `Ridge · MIT`。

6. **"Built with" 列表是否要带版本？** 本简报提议不带版本（`Tauri · Svelte · Rust · TypeScript`）。如果你认为带 `v2 / 5` 更专业，可改为 `Tauri 2 · Svelte 5 · Rust · TypeScript`，但其它技术细节仍按本简报隐藏。

7. **404 页是否需要保留品牌色装饰文字？** 现行 `4 0 4` 大字保留无妨；本简报只改下面的标题与说明文字。

---

简报正文到此结束。可以由 content-editor 直接按表 / 按节粘贴。
