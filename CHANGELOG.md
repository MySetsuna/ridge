# Changelog

All notable changes to **Ridge** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.0.8] — 2026-06-19

公网远控稳定性与体验专项（切后台不掉线、TOTP 少重输、scrollback 完整、切 Pane 不丢/不断）。

### Added
- **TOTP 受信控制端授权**：通过验证的设备（绑定云账号 + 该浏览器/设备的持久 Ed25519 身份）**24 小时内重连免再输** TOTP。端到端在 host 验证（relay/后端零参与），grant 经 DPAPI/0600 加密落盘。登出 / 换设备 / 防爆破触发 / 种子轮换 / 过期仍**强制重验**（契约 §7.4）。
- **连接前 fail-fast 校验**：访问 `{device}-{user}` 远控域名时先校验登录态与账号/设备归属，并即时映射 WS 错误码（账号不符 / 设备不属 / 已停用 / 非会员）为可读提示，不再长时间干等「连接中」。

### Changed
- **登录态滑动 3 天续期**：刷新凭证 3 天内有活动即长期在线，配合控制端「可见即主动续期」熬过切后台（需配套已部署的云后端）。
- 终端 scrollback 容量上调：host 存储 4→8 MiB，云回放上限 64→256 KiB。

### Fixed
- **切后台/锁屏回前台断连且重连失败**：回前台先 `await` 刷新 access token 再重连，修「后台 token 过期 → WS 升级 403 → 无限退避」。
- **终端 scrollback 不完整**：修复 E2EE/TOTP verified 之前回放被丢弃的竞态，连接/重连后历史完整回放。
- **手机端切 Pane 丢失 scrollback**：订阅即触发不节流的历史回放，空闲 pane 也能立即渲染历史。
- **快速切 Pane 易中断连接**：订阅 150ms 防抖 + DataChannel 背压保护，避免大回放灌爆缓冲导致断连。

---

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

[0.1.0]: https://github.com/MySetsuna/ridge/releases/tag/v0.1.0
