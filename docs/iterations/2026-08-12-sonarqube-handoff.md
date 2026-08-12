# SonarQube 交接（2026-08-12）

## 连接

- URL：`http://127.0.0.1:9000`
- 项目 key：`MySetsuna_ridge`
- 本机版本：`26.7.0.124771`
- 账号：`admin`
- 密码：由操作者在本机安全管理；不写入仓库、不写入脚本、不在日志输出。

SonarQube 服务需先启动。检查：

```powershell
Invoke-RestMethod http://127.0.0.1:9000/api/system/status
```

期望 `status` 为 `UP`。监控页面：

`http://127.0.0.1:9000/dashboard?id=MySetsuna_ridge`

## 扫描

项目配置位于 `sonar-project.properties`。先生成 LCOV：

```powershell
pnpm test:coverage:sonar
```

本机扫描器位于 `.tools/sonar-scanner-8.0.1.6346-windows-x64/bin/sonar-scanner.bat`。
推荐使用本地一次性 token 流程：由安全环境提供 `SQ_ADMIN_PASSWORD`，生成 token，
执行 scanner，最后撤销 token；不要把 token 或密码写入命令历史、文档或 CI 日志。

```powershell
$env:SQ_ADMIN_PASSWORD = '<operator-secret>'
try { .\.tools\sonar-scan-session.ps1 }
finally { Remove-Item Env:SQ_ADMIN_PASSWORD -ErrorAction SilentlyContinue }
```

分析完成后查询：

```powershell
$base = 'http://127.0.0.1:9000'
Invoke-RestMethod "$base/api/qualitygates/project_status?projectKey=MySetsuna_ridge"
```

本轮最终结果：CE `SUCCESS`、Gate `OK`、new coverage `84.7%`、new duplication
`1.22888%`、new violations `0`。CE task 为
`57a230d7-2fff-4c5a-a906-3161a469b304`，analysis 为
`71790ecc-e5bf-4402-9829-7405cf3c0a2f`。Sonar 日志只保留在本地 `.tools/`，不作为凭据
交接渠道。

扫描器 8.0.1 对多份 CRLF Rust 注释出现列偏移报错（源文件实际行长比 analyzer
报告列尾少 1）。正式扫描应使用 LF 临时工作树/扫描输入并恢复原始 CRLF 字节；
不得逐文件排除 Rust 生产代码。PTY 行为仍由 Rust 单测与 CDP PTY E2E 验证。
