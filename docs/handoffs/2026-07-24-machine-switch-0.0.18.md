# 换机交接：Ridge 0.0.18（2026-07-24）

本机（发 GitHub Release 的那台）**无** `RIDGE_ARTIFACT_TOKEN`，Remote 云产物与可选安装包需在有凭据/构建环境的机器上完成。按本文从上到下执行即可接续。

---

## 1. 当前仓库与发布状态

| 项 | 值 |
| --- | --- |
| 分支 | `codex/remote-git-diff-iteration-1`（Level 2 draft，**未** merge 进 `main`） |
| HEAD 参考 | `8404955` — `release: 0.0.18 open-vision close + job/live host plumbing` |
| 版本 | `package.json` / `src-tauri/tauri.conf.json` / `Cargo.toml` → **0.0.18** |
| Git tag | `v0.0.18`（已 push） |
| GitHub Release | https://github.com/MySetsuna/ridge/releases/tag/v0.0.18 |
| Remote 云 artifact | **未上传**（缺 token） |
| 桌面/跨平台安装包 | **由 `release.yml` 在 tag `v0.0.18` 上构建上传**（Windows exe/msi + Linux deb/AppImage + macOS dmg + rdg）。规则见仓库根 `Agents.md`「Releases」。换机后用 `gh release view v0.0.18` 确认资产齐全；CI 失败则 `gh workflow run release.yml -f tag=v0.0.18` 或本机补传。 |

### 换机后第一件事

```bash
cd <wind 仓库根>
git fetch origin
git checkout codex/remote-git-diff-iteration-1
git pull origin codex/remote-git-diff-iteration-1
git status   # 应干净，或仅有本机无关本地文件
git rev-parse --short HEAD   # 期望含 8404955 或其后的交接提交
```

---

## 2. 本轮已完成（不必重做）

### 2.1 规则与 NotebookLM

- Skill `notebooklm-iteration-loop`（`.claude` / `.codex` / `.ridge` / `.agents`）：**Notes 清空 ≡ 愿景全实现**；`[已实现]` 视同清理；用户侧判断可深研后拍板。
- Ridge 笔记本 `66919cb9-1329-4ddf-955c-f426d15a9fe6`：
  - 来源恒 1：`PROJECT-STATE`
  - note：`[已实现] 开放愿景清单`（open=0）
- 本地清单：`docs/iterations/2026-07-24-open-vision-checklist.md`
- 合同/报告：`CONTRACT-iteration-15.md` / `16.md`，`2026-07-24-iteration-15.md` / `16.md`

### 2.2 功能代码（摘要）

- Hosts：TCP 探测、`live_sink` 输入路由、`attach_host_session`
- G1：软暂停 + `os_freeze` + spawn 后 **Job Object** 挂载
- M1 s3：workspace memory goal/constraints/tasks + 命令
- Remote UI 缺失：`REMOTE_UI_MISSING` / `X-Ridge-Error`
- 其它：mobile copy、agent 探测、git rollback、图片预览版本号、paste/resize 回归

### 2.3 明确**未**做 / 不挡愿景

| 项 | 说明 |
| --- | --- |
| V-H1-LIVE 完整 WS 出站 | 下一里程：复用 `ridge-cli` `lan_session` 语义 + foreign 真 PTY 回灌 |
| Job freeze-info 整树冻结 | 现为 job 预建 + pid 冻结；未文档 API 的 job 整树 freeze 可后做 |
| 真机 smoke | `docs/plans/user-verification-checklist.md` |
| merge → main | Level 2 人工审查 |
| Remote 云上传 | **换机任务（见 §3）** |

---

## 3. 换机必办：Remote 云产物发布

涉及 iteration 15 起的 Remote 侧改动（`REMOTE_UI_MISSING`、mobile copy 等），**应上传 artifact**，否则公网 controller 仍吃旧 SPA。

### 3.1 环境变量

```bash
# 生产示例（域名以你环境为准）
export RIDGE_CLOUD_ARTIFACT_URL=https://<生产域名>/api/v1/remote-artifacts
export RIDGE_ARTIFACT_TOKEN=<与云端 RIDGE_ARTIFACT_TOKEN 一致>
```

Windows PowerShell：

```powershell
$env:RIDGE_CLOUD_ARTIFACT_URL = "https://<生产域名>/api/v1/remote-artifacts"
$env:RIDGE_ARTIFACT_TOKEN = "<token>"
```

Token 来源：生产/Dokku 配置或密钥库；**勿写进 git**。

### 3.2 发布命令

在仓库根：

```bash
# 完整：构建 desktop + mobile Remote 并上传
pnpm publish:remote-cloud

# 已有 web-remote-dist / static/remote 时可跳过构建
pnpm publish:remote-cloud -- --no-build

# 只打包不上传（验产物）
pnpm publish:remote-cloud -- --dry-run
```

脚本：`scripts/publish-remote-cloud.mjs`  
期望：HTTP 成功、`current` 指向 **0.0.18**（或脚本打印的 version）。

### 3.3 发布后核验

```bash
# 若有生产只读探测脚本
RIDGE_ARTIFACT_TOKEN=<token> node scripts/check-prod-status.mjs --base-url https://<生产域名>
```

或手动：

- `GET /api/v1/remote-artifacts/status`（需 token）→ 激活版本含 0.0.18
- 浏览器开一次公网 Remote，硬刷新，确认非「Remote UI not built」旧壳

### 3.4 失败回滚

```bash
pnpm publish:remote-cloud -- --rollback
# 或指定版本
pnpm publish:remote-cloud -- --rollback 0.0.17
```

---

## 4. 换机可选：桌面安装包

本机未跑全量 `tauri:build`。若需要 NSIS/MSI：

```bash
# 使用 package.json 版本 0.0.18
pnpm tauri:build
# 或带版本包装（若配置了 build:release）
# node scripts/build-ridge.mjs -r 0.0.18
```

产物常见路径：`target/release/bundle/` 或 `release/`（见 `scripts/post-build-rename.mjs`）。  
可将安装包 **手动附到** 已有 GitHub Release：

```bash
gh release upload v0.0.18 <path-to-setup.exe> <path-to-msi> --clobber
```

---

## 5. 换机可选：门禁复验

```bash
cargo test -p ridge --lib hosts::
cargo test -p ridge --lib job_object
cargo test -p ridge --lib teammate
cargo test -p ridge-remote remote_ui_missing
cargo test -p ridge-term paste_multiline
# 前端
pnpm exec vitest run packages/remote/src/shared/terminal/mobileCopy.test.ts
pnpm exec vitest run src/lib/stores/imagePreviewVersion.test.ts
```

（Windows PowerShell 下 `cargo … 2>&1 | Select-Object` 可能把 warning 显示成红字；以 `test result: ok` 为准。）

---

## 6. NotebookLM（若换机后要继续迭代）

```bash
nlm login --check
nlm note list 66919cb9-1329-4ddf-955c-f426d15a9fe6 -j
nlm source list 66919cb9-1329-4ddf-955c-f426d15a9fe6 -j
```

期望：note 标题含 `[已实现] 开放愿景清单`；source 仅 1 份 `PROJECT-STATE`。  
改状态后：更新 `docs/PROJECT-STATE.md` → `source add --wait` → 删旧源（先加后删，避免空窗）。

Skill 副本若本机未同步：以用户目录下 `.claude/skills/notebooklm-iteration-loop/SKILL.md` 为准，拷到 `.codex` / `.ridge` / `.agents`。

---

## 7. 审查 / 合并（非自动）

- 审查导读：`node scripts/generate-review-pack.mjs` → `docs/review/branch-review-guide.md`
- PR：https://github.com/MySetsuna/ridge/compare/main...codex/remote-git-diff-iteration-1  
  或 `gh pr create`（若尚未开）
- **禁止** auto-merge；人工过安全/协议面后再合 `main`

---

## 8. 建议换机执行顺序（最短路径）

1. `git pull` 到分支最新  
2. 设 `RIDGE_CLOUD_ARTIFACT_URL` + `RIDGE_ARTIFACT_TOKEN`  
3. `pnpm publish:remote-cloud`  
4. 核验 status / 浏览器 Remote  
5. （可选）`pnpm tauri:build` + `gh release upload v0.0.18 …`  
6. （可选）开/审 PR，**不**急于 merge  

---

## 9. 相关路径速查

| 文档/脚本 | 路径 |
| --- | --- |
| 本交接 | `docs/handoffs/2026-07-24-machine-switch-0.0.18.md` |
| Release 说明 | `docs/releases/0.0.18-notes.md` |
| 开放愿景清单 | `docs/iterations/2026-07-24-open-vision-checklist.md` |
| 项目状态 | `docs/PROJECT-STATE.md` |
| 用户核验清单 | `docs/plans/user-verification-checklist.md` |
| Remote 发布 | `scripts/publish-remote-cloud.mjs` / `pnpm publish:remote-cloud` |
| 生产状态探测 | `scripts/check-prod-status.mjs` |

---

## 10. 交接勾选

- [ ] 已 `git pull` 分支 `codex/remote-git-diff-iteration-1`
- [ ] 已配置 artifact URL + token
- [ ] 已 `pnpm publish:remote-cloud` 且 status 指向 0.0.18
- [ ] （可选）安装包已构建并上传 Release
- [ ] （可选）PR/审查已启动
- [ ] 未把 token 写入仓库或日志
