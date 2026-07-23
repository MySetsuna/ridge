# 30 分钟核验会动线（用户轨一次清偿）

更新：2026-07-23（iteration 12）。目的：把 `user-verification-checklist.md` 四件 + 历轮顺带核验项串成**一次约 30–40 分钟**的连续动线，降低启动成本。完成任一件即达成自动轨解冻条件（维护态 → 恢复迭代）。

## 准备（会前 5 分钟）

- 桌面构建：`pnpm tauri build`（或用现有 dev 环境 `pnpm tauri dev`）。
- 手机与桌面同 Wi-Fi；手机可切蜂窝。
- 若做生产件：备好 `RIDGE_ARTIFACT_TOKEN` 与生产域名；ridge-cloud 分支 `codex/remote-artifacts-status` 若未部署，status 探针预期 404（如实记录即「未部署」证据）。

## 第一段：桌面本机（约 8 分钟）

1. **暂停/恢复（G1 阶段一）**：开一个 teammate agent（或 tmux shim 注册）→ Agent Center 成员行悬停出 Pause → 点暂停：状态点转琥珀、agent send-keys 被拒（返回 agent suspended），**人类直接在 pane 打字仍通**；点 Play 恢复。
2. **暂停态跨重启（M1 切片一）**：保持一个 agent 暂停 → 完全退出 Ridge → 重启：Agent Center 该成员仍琥珀、send-keys 仍被拒；恢复后正常。
3. **HITL 只读链（P2 阶段 1 前置）**：设置开 HITL 网关（`set_hitl_enabled`），agent 触发一条高危命令（如 `rm -rf` 样式）→ 桌面弹审批 modal（含命令全文，正常）。**暂不裁决**，留给第二段远端看。

- 证据形态：每步一句结论；异常截图。

## 第二段：Remote 双入口（约 12 分钟）

4. **Team 面板（P1 UI）**：手机浏览器连 Remote（LAN 或 cloud）→ 头部/侧栏见 Team 标签 → roster 成员/状态点/Leader 冠标正确 → 点成员切到其 pane。暂停中的成员状态点应为非工作态。
5. **Pending 区（P2 阶段 1 脱敏）**：第一段第 3 步留下的待审批在 Team 面板顶部「Pending approvals」可见——**只见风险理由与发起者，绝不见命令全文**；无任何裁决按钮。回桌面裁决（拒绝），远端下轮轮询后消失。
6. **LAN 关区广播（iteration 10 缺陷修复）**：两个客户端（手机 + 桌面浏览器）同连 LAN → 任一端关一个工作区 → 另一端工作区列表**即时更新**（此前缺陷：不更新）。
7. **真机弱网 smoke（R1 真机轨，可选延长段）**：按 `cloud-remote-physical-smoke-runbook.md` 走 iOS Safari 或 Android Chrome 一遍（基线 → 换网 → 后台跨 token 窗 → 恢复）；evidence JSON 过 `node scripts/validate-remote-smoke-evidence.mjs <file>` exit 0。仅一平台不得宣称双平台通过。

## 第三段：生产两线（约 5 分钟）

8. `RIDGE_ARTIFACT_TOKEN=<token> node scripts/check-prod-status.mjs --base-url https://<域名>`：`service`/`artifacts` 两线输出存档（脱敏 token）。旧部署 status 404 = 「未部署」证据，亦有效。

## 第四段：合并审查（约 10 分钟起）

9. 读 `docs/review/branch-review-guide.md`（45+ 提交按协议面/安全面标注排序）→ 审查合并 wind `codex/remote-git-diff-iteration-1` → `main`。
10. ridge-cloud `codex/remote-artifacts-status` → 审查合并 `develop` 并部署 → 部署后重跑第 8 步，status 线应转绿。

## 会后

任一件完成即知会迭代循环（写 `docs/LOG.md` 或直接告知）；对应差距收口、维护态解冻。
