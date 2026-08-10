# Wave 73 闭环记录

日期：2026-08-10

## 结论

Cloud/Postgres、DPR 1.5、跨卷 ACL、Remote mobile headless 链路均取得真实本机证据；未发布、未 push、未创建 Release。

## 已修复并验证

- Tauri invoke 与 LAN invoke-result 的 `{Ok: ...}` / `{Err: ...}` envelope 已统一解包；硬错误仍拒绝，单测覆盖成功与失败分支。
- Remote teammate/HITL topology 在内核明确“不支持”时降级为空拓扑；其他错误仍抛出，避免把真实故障伪装成空数据。
- Cloud E2E 使用实际 Vite module URL；CDP 目标拒绝 `about:blank`，并按 Ridge/local target 选择；动态 CDP 端口变化会重新写入 `cdp-port.txt`。
- CSP 保留当前 inline script hash，并允许本地动态端口、tenant localhost 与运行时 Monaco/terminal 样式；新增 CSP 自检。
- Mobile headless 探针等待 host invoke、激活新建 pane 的 PTY、兼容 wrapped `Err`；内核不支持 shell/team 能力时输出可解释的空能力，不把不支持误报为失败。
- Cargo 工具路径支持 Cargo home 与 PATH；`ridge-core` 文件系统命令补齐 `Path` 导入，真实 `cargo test -p ridge-core --lib` 通过。

## 真实验收

### Cloud/Postgres

`node scripts/cdp-cloud-full-e2e.mjs`：exit 0。能力包含 `pane/invoke/fs/git/search/workspace/theme/teammate`；列表分页 3 页均通过；host online、controller connected。

Windows 无 wildcard `.localhost` 解析，验收期间临时添加精确 hosts 映射，脚本结束后已移除。此为本地 Windows DNS 限制，不是 Cloud 路由结论。

### DPR

`node scripts/cdp-dpr-e2e.mjs`：exit 0；`dpr=1.5`，`canvasCount=3`，`backingCanvasCount=3`，截图写入 `.iteration/artifacts/dpr/desktop-shot.png`。

该证据证明非整数 DPR 的运行几何与 backing canvas 存在；不等同于与原生 PowerShell 的像素级视觉矩阵，后者仍需人工/物理矩阵。

### 跨卷 ACL

`node scripts/cdp-cross-volume-acl-e2e.mjs`：exit 0。真实 Windows 账户下完成跨卷复制；源目录 DELETE 被拒；目标副本可读；源目录与源文件保留；D 盘临时目标已清理。

### Remote mobile

`node scripts/cdp-remote-mobile-agents.mjs`：`[remote-mobile] GATE: PASS`，覆盖 TOTP data-plane、移动 SPA、host 能力降级、wrapped `Err`、pane PTY 激活与 shell picker。

运行日志仍有两类非阻塞现象：自签名证书导致的 Service Worker 注册提示、以及偶发旧 Vite 端口的 HMR 动态模块错误。前者属于本地 HTTPS 信任环境；后者已记录为下一轮 `BUG-CDP-HMR-STALE-PORT-01`，不得宣称物理移动 PWA 已闭环。

## 测试与覆盖率

- 全量 Vitest：213 个测试文件通过。
- 本轮串行 V8 覆盖率：Statements `66.19%`，Branches `60.36%`，Functions `68.32%`，Lines `70.32%`。
- 覆盖率目录：`.iteration/artifacts/coverage-wave73`；规范化 LCOV：`coverage/lcov.info`。
- 若干未执行 CDP harness 的 V8 remap `PARSE_ERROR` 仍属已知扫描噪声；未伪装成覆盖率。
- Rust：`cargo test -p ridge-core --lib`，328 passed、0 failed；仅有既有 unused `copy_directory` warning。

## SonarQube

本机 `http://127.0.0.1:9000`，版本 `26.7.0.124771`，状态 `UP`。本轮未取得新的 scanner/CE 上传成功证据：Scanner 8 要求 `sonar.token`，浏览器控制面不可用，故未将当前本地覆盖率冒充 Sonar 指标。

权威服务器基线仍以 [Wave72 handoff](2026-08-10-wave72-quality-scan-handoff.md) 的最近成功 CE 结果为准：coverage `80.2%`、line `86.4%`、branch `71.6%`、violations `730`；Quality Gate `ERROR`，原因 `new_violations=152`。当前 Gate 不得称为绿色。

交接与安全用法见 [SonarQube Wave73 handoff](2026-08-10-sonarqube-handoff-wave73.md)。密码不写入仓库、日志或本交接文档；扫描使用临时 token，扫描后撤销。

## NLM 下一批需求

已用代理 `http://127.0.0.1:51081` 完成认证刷新，读取 Ridge 项目两本笔记的来源、Note、旧对话，并发起两轮只读查询。NLM 结果仅作假设，未替代代码事实：

1. 物理手机可信 HTTPS/PWA 与后台自愈仍开放，当前不能本地闭环。
2. NLM 提议 Message Hub / A2A envelope；CodeGraph 已确认仓库已有 `teammate::communication` envelope、delivery policy 与跨 workspace 拒绝测试，故该提议不能直接视为缺失功能，也不能直接新增 SQLite 大模块。
3. Postgres checkpoint 与 goal 恢复属于另一条需求，需先确认仓库现有 Checkpointer 与 Cloud 数据边界。

本轮不擅自落地上述大切片；仅将已观测的 HMR stale-port 作为下一轮本地 bug，并保留物理 PWA、公共 WebRTC/TURN、长时多 shell 为外部证据项。
