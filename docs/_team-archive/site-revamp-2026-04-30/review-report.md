# Review report — Ridge site revamp

**Reviewer:** code-reviewer (team `ridge-site-revamp`, task #4)
**Date:** 2026-04-30
**Scope:** the 6 user-facing files rewritten in tasks #2 and #3 — `site/index.html`, `site/docs.html`, `site/releases.html`, `site/404.html`, `README.md`, `CHANGELOG.md`. Cross-referenced against `docs/_team-archive/site-revamp-2026-04-30/content-brief.md` and `decisions.md`.

---

## Verdict

**PASS WITH MINOR FIXES** — the 6 listed user-facing files are clean. The fix list below is two ≤1-line text substitutions to placeholder SVGs that render visible "Monaco" / "tmux shim" strings on the homepage. Fixes are surgical (string replace inside an SVG `<text>` element). Team-lead can apply directly without re-spawning the editor. Without these, the edits as committed are still publishable — the SVGs are placeholder slots scheduled to be replaced by real recordings — but they do contradict Goal B as written.

---

## Goal A — zero sentimentality on user-facing files

**Result:** PASS.

Grep for `田埂|田块|耕耘|开荒|第一犁|一片地|一垄|翻开|播种|熟地|下一犁|看一眼就明白|切一刀|各就各位|铺到位` against the 6 files:

| File | Hits |
|---|---|
| `site/index.html` | 0 |
| `site/docs.html` | **1** at line 234 — `<p>Ridge 取自田埂——把屏幕分割成可以独立工作的几块。</p>` |
| `site/releases.html` | 0 |
| `site/404.html` | 0 |
| `README.md` | 0 |
| `CHANGELOG.md` | 0 |

The single hit at `site/docs.html:234` is the **canonical brand-touch sentence** specified verbatim in `decisions.md`'s "Brand-touch one-liner" section. Wording matches exactly. Appears only once and only in `docs.html` (under `#about`). ✓

Unrelated matches outside the review scope (flagged in Out-of-scope below): `site/RECORDING.md` and `site/styles/main.css` still contain `田埂` — both are not user-facing pages but `RECORDING.md` is shipped to GitHub Pages.

---

## Goal B — zero internal-implementation leak on user-facing files

**Result for the 6 listed files:** PASS.

Grep for `portable-pty|parking_lot|RwLock|OSC 7|64 KiB|4 MiB|tmux\.exe|tmux shim|RIDGE_TEAMMATE_URL|RIDGE_TEAMMATE_TOKEN|pty-output-|Cargo 零警告|cargo build --lib|cargo clippy|round 64|get_pane_scrollback|text_search|paneCwdStore|VecDeque|walkdir|overlayscrollbars|tokio|WixNSIS|scm-repo-changed|teammate-layout-changed` against the 6 files: **0 hits**.

Grep for `xterm\.js|WebGL|Monaco` against the 6 files: **0 hits**.

The footers ("Built with Tauri 2 · Svelte 5 · Rust · TypeScript") match the explicit allowance in decisions.md #6.

The README's mention of `pnpm tauri build`, `cargo`, `MSVC`, `WebView2`, `Rust 1.77+`, `Node 18+`, `pnpm 9+` are install-requirement disclosures — these are explicitly allowed by Goal E ("install requirements, not internal implementation"). ✓

**Caveat — homepage placeholder SVGs render banned strings:**
The 6 listed files don't contain `Monaco` or `tmux shim`, but two homepage placeholder SVGs that ARE rendered visually on `site/index.html` do:

| Asset | Rendered on | Visible text |
|---|---|---|
| `site/assets/placeholders/editor.svg:9` | `index.html:212` (showcase row "02 · Editor") | `资源管理器 + Monaco 编辑器 截图` |
| `site/assets/placeholders/agent-team.svg:14` | `index.html:262` (showcase row "04 · Agents") | `tmux shim + Claude Code 多终端协作` |

These SVGs are referenced directly with `<img src="…">` and render at runtime; the text inside the SVGs is visible to the user. `decisions.md` says "Do not remove or rename the file paths under `site/assets/`" — but it does not prohibit editing the rendered text inside an asset to remove a banned phrase. Fix is in the Fix list.

---

## Goal C — docs.html answers user questions

**Result:** PASS.

Sidebar IA matches the brief exactly:

```
Getting started: Install · First run
Working with terminals: Splitting & navigating · Scrollback & history
Working with files: Explorer · Editor · Search across panes
Working with Git: Commit graph · Branch & status · Stage & commit
Agents: Claude Code 协作
Reference: Keyboard shortcuts · Troubleshooting
About: Under the hood
```

Section-by-section verdict (each opens with a user-task lead, not architecture):

| Section | First-paragraph framing | Verdict |
|---|---|---|
| `#install` 安装 | "Ridge 是一个桌面应用，提供 Windows 安装包；macOS 与 Linux 用户当前需要从源码构建。" | task-first ✓ |
| `#first-run` 第一次运行 | "启动后会看到一个全屏的终端分屏。" + bulleted "建议先做这几件事" | task-first ✓ |
| `#terminals-split` 分屏与切换 | "Ridge 的核心工作模式是任意嵌套的分屏。" + concrete shortcuts | task-first ✓ |
| `#terminals-scrollback` 滚动历史 | "每个分屏会保留最近的命令输出，可以用滚轮…" | task-first ✓ |
| `#files-explorer` 文件浏览器 | "左侧栏第一个图标打开文件浏览器。" | task-first ✓ |
| `#files-editor` 代码编辑器 | "Ridge 内置的代码编辑器和终端共享同一套分屏布局" | task-first ✓ |
| `#files-search` 跨分屏搜索 | "左侧栏第三个图标（也可按 Ctrl + Shift + F）打开搜索面板。" | task-first ✓ |
| `#git-graph` 提交图 | "左侧栏第二个图标打开源代码管理面板。" | task-first ✓ |
| `#git-status` 分支与状态徽章 | "每个分屏标题栏右侧有一个徽章…" | task-first ✓ |
| `#git-commit` 暂存与提交 | "源代码管理面板列出当前仓库的全部改动…" | task-first ✓ |
| `#agents` 与 Claude Code 协作 | "Ridge 兼容 Claude Code 的多分屏会话协议。" + concrete steps | task-first ✓ |
| `#shortcuts` 快捷键 | Pure reference table | reference ✓ |
| `#trouble` 常见问题 | Symptom-led sub-headings | troubleshoot ✓ |
| `#about` Under the hood | One short paragraph + brand-touch | About-style ✓ |

No section drifts back into architecture-explaining mode.

---

## Goal D — visual flow + structure preserved

**Result:** PASS.

| Check | Status |
|---|---|
| Hero field-frame mockup with 4 plots labelled `PANE · TERMINAL/GIT/EDITOR/AGENT` | ✓ `index.html:76–102` |
| 4 showcase rows with eyebrows `01 · Split` / `02 · Editor` / `03 · Git` / `04 · Agents` | ✓ `index.html:171, 196, 221, 246` |
| Quick-start 3 tabs (dev / build / install) + code block | ✓ `index.html:278–313` |
| Latest-release peek h2 `最新版本 v0.1.0` | ✓ `index.html:320` |
| Footer `Ridge · MIT License` + `Built with Tauri 2 · Svelte 5 · Rust · TypeScript` | ✓ all three pages match |
| Nav anchors `#features`, `#showcase`, `#start` resolve in `index.html` | ✓ `id="features"` line 108, `id="showcase"` line 162, `id="start"` line 271 |
| All 14 `docs.html` sidebar `href="#…"` anchors resolve to matching `id="…"` in same page | ✓ confirmed each one (install, first-run, terminals-split, terminals-scrollback, files-explorer, files-editor, files-search, git-graph, git-status, git-commit, agents, shortcuts, trouble, about) |
| HTML balance — no obvious unclosed tags in diff | ✓ |
| Existing CSS classes preserved (no class renames) | ✓ |

The hero `<h1>` two-line + accent-span structure is preserved (`把终端、编辑器和 Git<br/>放进同一个<span class="accent">工作台</span>`). The `field-frame` / `field-titlebar` / `field-plots` / `plot` / `label` class hierarchy is intact, just relabelled `PLOT →  PANE` and `cultivate / plot` → `render / pane` in the mock code. ✓

---

## Goal E — README + CHANGELOG appropriate for their audience

**Result:** PASS.

**README.md** — developer-facing landing page:
- Highlights bullets reframed at user level; no `portable-pty / xterm.js / WebGL / Monaco / tmux.exe / OSC 7 / 64 KiB / 4 MiB / cargo build --lib / paneCwdStore / .git/ 文件监听` anywhere.
- Install requirements (Node 18+, pnpm 9+, Rust 1.77+, MSVC, WebView2) retained — explicitly allowed by Goal E.
- 田埂的隐喻 section deleted ✓.
- Footer simplified to `Built with Tauri 2 · Svelte 5 · Rust · TypeScript` ✓.
- Brand-touch is correctly NOT duplicated here (decisions.md #1: only docs.html) ✓.
- Internal-doc links `TERMINAL_SCROLLBACK.md` / `AGENT_TEAMS_TEAMMATES.md` / `PANE_GIT_PILL_VERIFY.md` / `CLAUDE.md` are kept under "## 文档" — appropriate for a dev README, not a user-facing leak.

**CHANGELOG.md** — reads as proper release notes:
- Subtitle `「开荒 / Breaking Ground」` removed ✓
- Sectioning is `Added` / `Improved` / `Known limitations` ✓ (Keep-a-Changelog conformant)
- All `portable-pty + xterm.js (WebGL) / 64 KiB blocks / 4 MiB / OSC 7 / paneCwdStore / round-64 / cargo build --lib / NSIS / WiX / RIDGE_TEAMMATE_URL / overlayscrollbars / round 19` from the previous version are gone.
- Tone is user-facing ("scrollback that holds several megabytes of output", "Auto-detects git worktree links") rather than commit-log internals. ✓

---

## Fix list

Both fixes are <1-line surgical text substitutions inside placeholder SVG assets. Apply directly with `Edit` — no need to re-spawn the editor.

### Fix 1 — `site/assets/placeholders/editor.svg` line 9

Replace the visible label so "Monaco" no longer leaks onto the homepage.

**Old:**
```xml
    <text x="760" y="412" font-size="13">资源管理器 + Monaco 编辑器 截图</text>
```

**New (paste-ready):**
```xml
    <text x="760" y="412" font-size="13">文件浏览器 + 代码编辑器 截图</text>
```

### Fix 2 — `site/assets/placeholders/agent-team.svg` line 14

Replace the visible label so "tmux shim" no longer leaks onto the homepage.

**Old:**
```xml
    <text x="640" y="412" font-size="13">tmux shim + Claude Code 多终端协作</text>
```

**New (paste-ready):**
```xml
    <text x="640" y="412" font-size="13">Claude Code 多分屏协作 截图</text>
```

---

## Out-of-scope observations (not blocking)

These are flagged for follow-up. None of them are review-goal violations on the 6 listed files; do not block on them.

1. **`site/RECORDING.md`** is shipped to the GitHub Pages site root and still contains `田埂` (line 51) and `tmux shim` / `list-panes / rename-window` (line 54). It's a contributors' "how to record demos" guide, not a user-navigated page (no nav links point to it), but a curious user could browse to `/RECORDING.md` directly. Probably worth a separate cleanup pass; not part of this review's bar.

2. **`site/styles/main.css:3`** has a comment `Theme: 田埂 / field ridges. Dark earth + ridge-green accent.` — it's a CSS comment, not user-visible. Cosmetic only; mention only because the brief technically scoped "anywhere else" outside docs.html.

3. **Minor wording inconsistency between index.html peek and releases.html main entry:**
   - `index.html:332` — "Ridge 的第一个公开版本"
   - `releases.html:61` — "Ridge 的首个公开版本"
   Both fine, both compliant; cosmetic harmonisation if you want to align them.

4. **`index.html:332`** says "Ridge 的第一个公开版本。在这一版里，分屏终端、代码编辑器、Git 提交图与智能体协作首次同时可用。" — duplicates verbatim the lead paragraph above it (`index.html:321`'s sub-heading already says the same thing). Slight redundancy in the latest-release peek; not a bug.

5. **CHANGELOG.md** — no inbound link from the homepage. README references it. Maybe worth an `<a href="../CHANGELOG.md">` from `releases.html` next to "git log", but this is enhancement-territory.

6. **`site/RECORDING.md`** still describes the placeholder SVGs as carrying their old labels — if Fix 1 / Fix 2 are applied, the descriptions in RECORDING.md become slightly stale (pointing at strings that no longer exist). Not user-visible, but worth knowing if you want them in sync.

7. **`site/index.html:332`** — first sentence reads "Ridge 的第一个公开版本。" but the brief's reference text was "Ridge 的首个公开版本。" — a minor stylistic preference, not a violation.

---

End of report.
