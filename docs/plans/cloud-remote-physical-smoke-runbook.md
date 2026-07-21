# Cloud Remote 双平台真机 smoke runbook

适用范围：iOS Safari、Android Chrome；Cloud Remote 真实换网、后台冻结与 token 跨窗恢复。  
证据格式：`docs/plans/remote-smoke-evidence.schema.json`。  
本地证据目录：`artifacts/remote-smoke/<platform>-<timestamp>/`（已 gitignore）。

## 1. 安全与停机规则

- evidence、截图、录屏和日志不得包含 JWT、TOTP、密码、账号、完整设备域名或私有仓库内容。
- 开始前关闭浏览器密码自动填充提示；截图时遮蔽 URL 中的设备/租户信息。
- 任一平台基线连接失败，停止该平台后续场景，仅记录首因。
- 自动门禁失败时不做真机 smoke：先修状态机。
- 不刷新页面来“修复”换网/后台场景；一旦手动刷新，该场景记 `fail`。
- 建议恢复目标为 45 秒；超过即记 `fail`，不得改 evidence 数值或放宽阈值。

## 2. 准备

1. 在 wind 仓库记录：

   ```powershell
   git rev-parse HEAD
   git status --short
   pnpm exec vitest run packages/remote/src/shared/cloud/faultInjection.test.ts
   ```

2. 记录实际部署的 Remote build 标识和 ridge-cloud commit/deployment 标识；若无法证明部署版本，结果记 `blocked`。
3. 在 `artifacts/remote-smoke/<platform>-<timestamp>/` 复制 `remote-smoke-evidence.example.json`，命名为 `evidence.json`。
4. 准备一个无敏感内容的测试 workspace/pane，终端运行持续递增的可见序号；确认 scrollback 中有可辨识的起止标记。
5. 开启系统屏幕录制或秒表。只记录用户可见状态；浏览器日志必须先脱敏。

## 3. 基线场景

1. 手机连接 Wi-Fi，打开 Cloud Remote。
2. 完成 TOTP 或受信控制器授权。
3. 选择测试 pane，输入唯一无敏感标记并确认回显。
4. 滚动到历史标记，再回到底部；确认 pane、scrollback、Files/Git/Search capability UI 与当前 host 一致。
5. 记录 `baseline` 场景开始/结束时间、恢复耗时 `0`、截图路径和 `pass/fail`。

失败即停：黑屏、错误 pane、输入无回显、授权失败或 capability UI 与 host 明显不符。

## 4. Wi-Fi → 蜂窝

1. 保持 Remote 页面前台且测试 pane 可见，记开始时间。
2. 关闭 Wi-Fi，确认设备实际切到蜂窝网络；不要刷新页面。
3. 等待连接状态恢复，在同一 pane 输入新标记并确认回显。
4. 检查断线前 scrollback 仍可见、没有跳到其它 pane、没有重复输出明显迹象。
5. 记恢复时间、`wifi_to_cellular` 结果及截图/录屏路径。

## 5. 蜂窝 → Wi-Fi

按上一节反向执行，场景名为 `cellular_to_wifi`。必须再次输入新标记，不能仅凭“画面看起来正常”判通过。

## 6. 后台跨 token 窗口

1. 在连接正常且同一 pane 可写时记开始时间。
2. 将浏览器切到后台至少 16 分钟（≥960 秒），期间不要主动保持页面前台。
3. 回到浏览器，等待自动 token refresh/wake-up/reconnect；不要刷新。
4. 在 45 秒内验证同一 pane、scrollback、输入回显和 capability UI。
5. 场景名 `background_token_window`，填写 `backgroundDurationSeconds`、恢复耗时和附件。

## 7. 收尾与校验

1. `overallResult` 只有在四个场景均 `pass` 时才写 `pass`；存在失败写 `fail`，无法执行写 `blocked`。
2. 对每个平台执行：

   ```powershell
   node scripts/validate-remote-smoke-evidence.mjs artifacts/remote-smoke/<platform>-<timestamp>/evidence.json
   ```

3. iOS 与 Android 各需一份通过校验的独立 evidence。只有一个平台时不得宣称双平台完成。
4. 发现稳定复现缺陷时，保留脱敏证据并停止扩项；下一轮只处理该单一缺陷。
