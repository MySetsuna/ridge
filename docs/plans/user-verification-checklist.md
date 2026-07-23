# 用户必办核验清单（iteration 7 用户轨）

更新：2026-07-23。自动化迭代已领先物理验证若干版本；下列四件只能由人完成。每件给「怎么做」与「完成证据形态」。完成后知会迭代循环（写入 LOG 或直接告知），对应差距即可收口。

## 1. 真机双平台 Cloud Remote smoke（R1，iteration 4 遗留）

- 怎么做：按 `docs/plans/cloud-remote-physical-smoke-runbook.md`，iOS Safari 与 Android Chrome 各一遍（基线连接/TOTP → Wi-Fi→蜂窝→Wi-Fi → 后台跨 15 分钟 token 窗 → 恢复核验）。
- 证据形态：每平台一份 evidence JSON 放 `artifacts/remote-smoke/`，过 `node scripts/validate-remote-smoke-evidence.mjs <file>`（exit 0），附脱敏截图路径。
- 注意：仅完成一个平台不得宣称双平台通过；恢复 >45s 按失败证据记录。

## 2. 生产两条版本线状态实跑（T3 收口）

- 怎么做：`RIDGE_ARTIFACT_TOKEN=<生产 token> node scripts/check-prod-status.mjs --base-url https://<生产域名>`。
- 前置：先完成第 4 件（ridge-cloud 分支合并部署），status 端点才在线；旧部署会 404（如实记录亦可作为「未部署」证据）。
- 证据形态：命令输出 JSON 存档（脱敏 token），`service`/`artifacts` 两线均「通过」。

## 3. Team 面板人工核验（P1 MVP UI 验收）

- 怎么做：桌面或手机浏览器连一次 Remote（LAN 或 cloud），工作区内有 teammate agent 时：确认头部/侧栏出现 Team 标签（Users 图标）→ roster 列出成员与状态点 → 点按成员切到其 pane；无团队时确认无 Team 噪声（能力未协商时标签隐藏——可用 rdg 无头 host 验 denied 路径）。
- 证据形态：一句结论 + 截图（可选）。

## 4. 两分支审查合并（Level 2 → 主线）

- wind：`codex/remote-git-diff-iteration-1`（iteration 1–7 全部提交）→ 审查合并 `main`。
- ridge-cloud：`codex/remote-artifacts-status`（status 端点 + current.json 标记）→ 审查合并 `develop` 并部署。
- 证据形态：合并记录；ridge-cloud 部署后 `GET /api/v1/health` 版本更新。
