# Project Guidelines

## Communication
- Think in English, respond in Simplified Chinese. Code comments follow the existing language style of each repo.

## Workflow
- Use the Superpowers skills for non-trivial work: `brainstorming` → `writing-plans` → `executing-plans` (or `subagent-driven-development`). Process skills (brainstorming/debugging) come before implementation skills.
- Write design specs and plans under `docs/superpowers/specs/` (filename `YYYY-MM-DD-<topic>-design.md`); commit the design doc before implementing.
- **If there is even a 1% chance a skill applies to the current task, invoke it.** Don't skip skills because the task seems simple.

## Collaboration Rules
- Follow the user's instructions precisely, and within that scope act autonomously: gather the necessary context and complete the requested work end-to-end in this run, asking questions only when essential information is missing or the instructions are critically ambiguous.
- For hard-to-reverse or outward-facing actions (deletes, pushes, publishing), confirm the scope first unless explicitly authorized.
- One feature/concern per commit; keep commits focused.

## Releases（硬规矩）

发 **versioned GitHub Release**（`vX.Y.Z`）时，**必须**带上与该版本号一致的安装包/CLI 产物，形态对齐历史 Release（如 `v0.0.16` / `v0.0.17`），**禁止**只建空 tag + 文字说明、无 asset。

### 期望产物（与 `.github/workflows/release.yml` 矩阵一致）

| 平台 | 资产名示例（版本号随 release） |
| --- | --- |
| Windows | `ridge_X.Y.Z_x64-setup.exe`、`ridge_X.Y.Z_x64_en-US.msi`、`rdg-X.Y.Z-x86_64-windows.exe` |
| Linux | `ridge_X.Y.Z_amd64.deb`、`ridge_X.Y.Z_amd64.AppImage`、`rdg-X.Y.Z-x86_64-linux` |
| macOS | `ridge_X.Y.Z_aarch64.dmg`、`ridge_X.Y.Z_x64.dmg`（及 app tar 若 CI 产出）、`rdg-X.Y.Z-aarch64-macos` |

验收：`gh release view vX.Y.Z` 列出上述类资产；缺任一类（整平台失败除外且须在 notes 写明）不得声称「Release 完成」。

### 标准流程

1. 同步 `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 版本号。
2. 提交后打 **annotated tag** `vX.Y.Z` 并 `git push origin vX.Y.Z`（`on.push.tags: v*` 触发 `release.yml`）。
3. **等** workflow 全部 matrix job 结束；用 `gh run list --workflow release.yml` / `gh run watch` 跟进。
4. 确认 Release 资产齐全后，再改正式说明（`gh release edit`）；CI 默认可能出 **draft**，需人工/命令发布。
5. 若 tag 已推但 workflow 未跑/失败：`gh workflow run release.yml -f tag=vX.Y.Z` 重跑，或本机 `RIDGE_BUNDLES=nsis,msi pnpm tauri:build` 后 `gh release upload vX.Y.Z <files> --clobber`（仅补当前平台；全平台仍以 CI 为准）。
6. **Remote 云 artifact**（`pnpm publish:remote-cloud`）是另一条发布线，与桌面安装包并列；有 token 的机器上传，缺 token 写进交接文档，**不能**用「只发了 GitHub 空 Release」代替安装包。

### 禁止

- 仅 `gh release create` 无构建、无 upload、无 CI 产物即收工。
- 资产版本号与 tag / `package.json` 不一致（例如 tag `v0.0.18` 却挂 `0.0.17` 安装包）。

参考：`.github/workflows/release.yml`、`docs/release-signing.md`、历史 Release 资产列表。

## Code Intelligence
- See `.Codex/AGENTS.md` for the CodeGraph MCP usage guide — prefer it for structural code questions (callers/callees/impact/definitions) over grep.
