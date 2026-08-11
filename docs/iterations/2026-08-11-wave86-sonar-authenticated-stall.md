# Wave 86：Sonar 已认证但扫描桥超时（2026-08-11）

## 结果

- 本地 SonarQube `http://127.0.0.1:9000` 状态 `UP`。
- 使用当前 `admin` 凭据调用 `/api/authentication/validate`：HTTP `200`，`valid=true`。
- 临时 scanner token 仅用于本次尝试，扫描结束后已撤销；凭据和 token 未写日志、文档或仓库。
- scanner 使用 Node `C:\DevKit\nvm\v20.19.0\node.exe`，命令带
  `-Dsonar.qualitygate.wait=true`，并以 180 秒墙钟上限运行。
- 扫描完成 Rust、HTML、coverage、text/secrets 等阶段，卡在
  `JavaScript/TypeScript/CSS analysis` 的 `Resolving provided TSConfig files`；未取得
  新 CE task / Quality Gate 结果。精确 scanner 进程树已清理。

## 服务端状态

最新服务端分析仍为 `c271e74b-ac3f-4277-bbef-74418f48b822`，Quality Gate 为
`ERROR`；本次尝试没有形成新的上传分析，故不能把本地扫描日志或 LCOV 当作 Gate
通过证据。

完整脱敏运行日志：`.iteration/artifacts/sonar-wave86.log`（运行态，不提交）。

## 下一步

1. 已由 `4270dc55` 移除失效的 `packages/rg-split/examples/tsconfig.json`；直接运行
   `tsc --showConfig` 曾稳定返回 `TS5083`（缺少 examples 自有 `.svelte-kit/tsconfig.json`），
   根、Remote、rg-split 三份配置现均解析成功。
2. 以新的临时 token、Node 20 和进程树上限重跑；必须同时取得 CE 成功、项目
   coverage、`new_violations=0` 与 Quality Gate `OK`。
3. 扫描成功前，`REQ-SONAR-COVERAGE-80-01` 和 Sonar 质量门禁继续保持 ACTIVE；
   当前浏览器连接不可用，未重试密码或创建新 token。
