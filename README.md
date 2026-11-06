<p align="center">
  <img src="./static/ridge-mark.svg" width="64" height="64" alt="Ridge mark" />
</p>

# Ridge

<p align="center">
  <i>Like the ridges between rice paddies, split your work into plots — each field bearing its own harvest.</i>
</p>

<p align="center">「如田埂分畦，各耕其获」</p>

<p align="center">
  <a href="docs/README_CN.md">中文文档</a>
</p>

---

## 架构

| | |
|---|---|
| **架构入口** | [docs/architecture/README.md](docs/architecture/README.md) |
| **系统总览** | [docs/architecture/00-overview.md](docs/architecture/00-overview.md) |
| **ridge-kernel** | [docs/architecture/10-kernel.md](docs/architecture/10-kernel.md) |
| **Terminal protocol** | [docs/architecture/20-rtp1-wire.md](docs/architecture/20-rtp1-wire.md) |
| **Remote lifecycle** | [docs/architecture/30-remote-lifecycle.md](docs/architecture/30-remote-lifecycle.md) |
| **ridge CLI** | [docs/architecture/40-cli.md](docs/architecture/40-cli.md) |
| **Desktop / Tauri** | [docs/architecture/50-desktop-tauri.md](docs/architecture/50-desktop-tauri.md) |
| **Data flow** | [docs/architecture/60-data-flow.md](docs/architecture/60-data-flow.md) |
| **Runtime epoch** | [docs/architecture/70-runtime-epoch.md](docs/architecture/70-runtime-epoch.md) |
| **Security** | [docs/architecture/80-security.md](docs/architecture/80-security.md) |
| **Performance** | [docs/architecture/90-performance.md](docs/architecture/90-performance.md) |
| **Foundation 终报告** | [docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md](docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md) |

权威契约见根目录 [specs](specs/)。docs 解释当前实现。Foundation 报告、旧规范副本与阶段验收均属历史材料，边界见[架构入口](docs/architecture/README.md)。

---

<p align="center">
  <a href="https://github.com/Pooowu">
    <img src="https://github.com/Pooowu.png" width="72" height="72" alt="@Pooowu" />
  </a>
</p>

<p align="center">
  <b>💛 Special Thanks · 特别鸣谢</b><br/>
  Heartfelt thanks to <a href="https://github.com/Pooowu"><b>@Pooowu</b></a> for sponsoring Ridge.<br/>
  <sub>衷心感谢赞助者 <a href="https://github.com/Pooowu">@Pooowu</a> 对 Ridge 的支持 🙏</sub>
</p>

---

## Architecture

Ridge runs business services and PTYs in an independent Kernel process. Desktop WebView, CLI/TUI, LAN and Cloud clients share KernelHost and business dispatch. Tauri provides OS integration and authenticated local bootstrap. Clients consume canonical terminal semantics and keep presentation state. Closing a client detaches its connection while Runtime and live PTYs continue.

| Component | Responsibility |
| --- | --- |
| [ridge-kernel](packages/ridge-kernel/src/lib.rs) | Runtime lifecycle, Host, domain services, PTY and semantic output |
| [ridge-core](packages/ridge-core/src/lib.rs) | Tauri-free domain logic and capability checks |
| [ridge-remote](packages/ridge-remote/src/lib.rs) | LAN/Cloud transport and authentication adapters |
| [ridge-term](packages/ridge-term/src/lib.rs) | Terminal semantics and rendering support |
| [ridge-cli](packages/ridge-cli/src/main.rs) | TUI, dashboard and headless entry points |
| [src-tauri](src-tauri/src/lib.rs) | Desktop OS shell and Runtime bootstrap |
| [shared frontend](packages/remote/src/shared/) | RPC, terminal consumption and transport adapters |

See [topology](docs/architecture/00-overview.md), [flows](docs/architecture/60-data-flow.md) and [authoritative Runtime contract](specs/l2-unified-client-runtime.md).

---

## Key Features

### Terminal
- Unlimited recursive **horizontal/vertical splits** with drag-to-dock
- Independent PTY per pane: each pane has its own shell process, CWD, and history
- Canonical terminal snapshots, deltas and history produced by Kernel and consumed through shared Host transports
- Scrollback with search, "load older" paging, and user-scroll-lock detection
- Shell-integration prompt markers (`OSC 133;A` / `OSC 633;A`) for instant git-diff refresh
- Per-pane **shell switcher** (pwsh, cmd, bash, git-bash, WSL)
- Inline link resolution: clickable hyperlinks, file paths, git SHAs

### Workspaces
- Multiple **named workspaces** with independent pane trees, all processes kept alive across switches
- Single **global host canvas** (A.9): workspace switching is a pure DOM display toggle — no canvas reconfigure, no swap-chain clear, no black flash
- Save/restore via `.ridge` files in `~/ridge-workspaces/`
- Startup restore: re-open last session when launched from menu; override with CLI `ridge`

### Editor
- Monaco Editor as an **alternative pane mode** within the split layout
- Diff editor modal for inline file comparison
- Markdown preview with Mermaid diagram support

### Git (Source Control)
- Commit graph rendered from libgit2 history (not `git log` subprocess)
- Per-pane **Git status pill**: branch name, ahead/behind count, changed-file count
- **SCM sidebar**: stage/unstage/discard individual files, commit with message, branch switch, create tag, cherry-pick, revert
- Git operations: fetch, pull, push, sync against the SCM-selected repository
- Auto-refresh on filesystem change (notify crate) — no polling

### Search
- **Cross-workspace search** with regex, case-sensitive, whole-word, glob-filter toggles
- Replace-in-files across matches
- Text-search diagnostics sidebar

### Collaboration (agents & tmux)

Commune members, groups, protocol connections, history and task receipts belong to the Host Runtime. Desktop and Remote present shared state with exact workspace/pane/session bindings. See [Commune contract](specs/l3-commune-protocol-membership.md). The standalone ridge-tmux crate provides the headless tmux engine.

### Remote Control

LAN WSS and Cloud E2EE WebRTC adapt to the same KernelHost business services and terminal semantics. Runtime owns listener, signaling and reconnection lifecycle. Remote opens the narrow console by default, with the desktop browser surface available through `?ui=desktop`. CLI initial creation covers TUI, dashboard, LAN-only and explicit Cloud shell/CWD requests. See [CLI flows and retry rules](docs/architecture/40-cli.md).

### Theming
- Three built-in themes with CSS custom properties
- Theme system injected at splash-screen time (before SvelteKit hydration) so the first frame already matches the user's saved theme
- Monaco editor theme auto-synced from Ridge theme data

### Plugins
- Lightweight sidebar plugin system with global and workspace scopes
- Built-in plugins: **global status** panel; **Native sessions** panel (desktop-only — lists headless `ridge-tmux` sessions and summons them into the workspace; gated out of the web-remote build, where its host-only commands aren't available)

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop framework | [Tauri v2](https://v2.tauri.app/) |
| Frontend | Svelte 5 + SvelteKit (SPA mode) + Tailwind CSS v4 |
| Terminal kernel | Custom Rust crate (`ridge-term`) → WASM via wasm-bindgen |
| GPU rendering | wgpu (WebGPU-only) |
| Rendering host | Main-thread terminal host (`TerminalManager` + WebGPU surface) |
| Code editor | Monaco Editor 0.55 |
| Git | libgit2 (`git2` crate v0.19) |
| Database | SQLite via rusqlite (bundled) |
| Remote server | axum 0.7 (WebSocket + static file serving) |
| Auth | TOTP (SHA-256 HMAC, self-implemented RFC 6238) |
| IPC | Authenticated local WS, LAN WSS and Cloud WebRTC through shared Host services. Tauri handles OS bootstrap |
| Serialization | Generated canonical terminal protocol and JSON business RPC |
| PTY | portable-pty 0.8 |
| Font icons | lucide-svelte |
| Diagrams | Mermaid 11 |
| Markdown | marked 15 |
| File watching | notify v6 + notify-debouncer-mini |
| Process discovery | sysinfo |
| Testing | Vitest (unit), Playwright (e2e), WebdriverIO (e2e-shell + perf) |

---

## Development

### Prerequisites
- **Rust** toolchain (latest stable)
- **Node.js** ≥ 18
- **pnpm** ≥ 8
- **Tauri v2** system dependencies ([setup guide](https://v2.tauri.app/start/prerequisites/))
- Windows: WebView2 runtime (bundled with Windows 10 21H2+)

### Commands

```bash
# Install dependencies
pnpm install

# Development (Vite HMR + Tauri hot-reload)
pnpm tauri dev

# Build release binary
pnpm tauri:build

# Run tests
pnpm test          # Unit tests (vitest)
pnpm e2e           # E2E tests (Playwright)
pnpm e2e:shell     # Shell-level integration tests (WebdriverIO)
pnpm e2e:perf      # Perf benchmarks (frame attribution, stress)

# Build WASM terminal kernel
node packages/ridge-term/build.mjs

# Build tmux compatibility shim
pnpm build:teammate-shim

# Build remote server (standalone binary)
pnpm build:remote-server

# Remote dev
pnpm dev:remote         # Run remote app dev server
pnpm build:remote       # Build remote app for production

# Type-check
pnpm check
```

### Project Structure

- [packages/ridge-kernel](packages/ridge-kernel/) contains independent Runtime services.
- [packages](packages/) contains reusable Rust crates and shared frontend packages.
- [src](src/) contains desktop and Remote presentation entry points.
- [src-tauri](src-tauri/) contains the desktop OS shell.
- [specs](specs/) and [changes](changes/) contain SpecTree-controlled contracts and changes.
- [docs/architecture](docs/architecture/) explains current structure and links historical evidence.

---

## Release & Distribution

- **Windows**: NSIS installer + MSI, both with `PATH` environment registration
- Binary includes embedded `tmux` shim for Claude Code agent integration
- Bundled static assets: `ridge.theme` file + remote web app for LAN mobile access
- CI: GitHub Actions deploys the marketing site (`site/`) to GitHub Pages on push to `main`

---

## License

MIT License. Copyright (c) 2026 Jack Jiang and Ridge contributors.

---

<details>
<summary><b>☕ Buy Me a Coffee</b></summary>

<br>

<p align="center">
  <i>If Ridge has made your workflow smoother, consider buying me a coffee.</i>
</p>

<div align="center">

| WeChat 赞赏 | PayPal |
|:---:|:---:|
| WeChat Reward | PayPal Donate |
| 微信扫码赞赏 | PayPal Donate |

</div>

<br>

</details>
