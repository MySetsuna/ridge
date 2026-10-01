# Remote / Cloud 发布与验收

## 发布线

Remote 浏览器端是独立静态产物，使用 `.github/workflows/publish-remote.yml` 发布到
Cloud 持久卷；它不重新部署 `ridge-cloud`，并保留最近三个 release 供回滚。

Cloud 桌面端使用 `.github/workflows/release.yml` 构建 Tauri 安装包及 `ridge` CLI。该工作流
必须保持 `releaseDraft: true`：手动发布只生成 GitHub Release 草稿，不得直接转为正式版。

## 凭据边界

发布工作流只读取以下 GitHub 配置名称：

- Secret：`RIDGE_ARTIFACT_TOKEN`
- Variable：`RIDGE_CLOUD_ARTIFACT_URL`

Token、Cookie、私钥不得写入仓库、提交记录、临时报告或本机配置。需要发布时由 GitHub
Actions 将 Secret 注入工作流；本机只检查 Secret 名称是否存在，不读取 Secret 值。

## Remote 发布

1. 推送已通过门禁的提交。
2. GitHub Actions → `publish-remote` → `Run workflow`，正常发布保持 `rollback=false`
   且 `no_build=false`。
3. 工作流会构建 wasm，上传 desktop/mobile 两套 bundle，并校验 `/status`、公开
   `artifact.json` 的 `artifactSchema`、版本及提交短 SHA。
4. 在线验收至少检查健康端点、公开 Remote 页面、桌面与移动 bundle 指针均已切换。

回滚只勾选 `rollback=true`，不得同时用旧本机产物执行 `no_build`。回滚后再次检查
`/status` 与公开 `artifact.json`。

## Cloud 草稿发布

1. 以本次提交对应版本创建 `v<version>` tag，或手动运行 `release` 并填写 tag。
2. 等待 test gate 与各平台 build 完成；安装包、`ridge` CLI 和可选签名产物进入同一个 GitHub
   Release 草稿。
3. 只做草稿验收：下载资产、检查版本与 SHA、查看构建日志；本轮不得点击“Publish
   release”。

## TOTP 信任与重连

当前连接会话首次通过验证码后，控制端授予对应 controller 公钥 24 小时信任。信任记录
持久化于控制端安全存储，刷新页面或完整重连时先检查该 grant；有效则不再要求第二次
验证码。旧版 Host 不支持 trust grant 时，Remote 仅将当前内存中的验证码作为兼容回退，
不会把验证码写入持久化存储。

登出、撤销控制端或重置 TOTP 会使 grant 失效。若失联后重连失败，按此顺序检查：

1. Host 与 Remote 是否使用同一版本协议及同一 artifact SHA。
2. 控制端 controller 公钥是否未变化，24 小时 grant 是否仍有效。
3. 新 Host 是否已安装直接的 `totp_trust_check` / `totp_trust_record` 命令；旧 Host
   只应走兼容验证码回退。
4. 浏览器是否仍持有当前会话状态；必要时重新输入验证码，确认一次后再观察下一次
   刷新 / 重连是否走 trust grant。
5. 最后检查 Cloud 健康端点、公开 Remote 产物与 Host 日志；不得以关闭 TLS 校验或
   读取本机 Token 规避问题。

## 验收记录

发布报告至少记录：提交 SHA、Remote artifact 版本、Actions run URL、Release 草稿
URL、公开健康检查结果、Remote desktop/mobile 指针结果，以及是否执行回滚。敏感值
只写“已配置 / 未配置”，不写值本身。
