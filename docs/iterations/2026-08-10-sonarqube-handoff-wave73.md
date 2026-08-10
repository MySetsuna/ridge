# SonarQube 交接（Wave 73）

## 服务

- 地址：`http://127.0.0.1:9000`
- 项目：`Ridge`
- project key：`MySetsuna_ridge`
- 管理账户：`admin`
- 当前密码：由用户自行保管，已轮换；不写入仓库、环境文件、日志或交接文档。

## 当前状态

- SonarQube `26.7.0.124771`，`UP`。
- 最近可引用的成功 CE 分析：coverage `80.2%`、line `86.4%`、branch `71.6%`、violations `730`。
- Quality Gate：`ERROR`；`new_violations=152`。覆盖率达标不代表 Gate 已绿。
- Wave 73 本地 V8：Statements `66.19%`、Branches `60.36%`、Functions `68.32%`、Lines `70.32%`；这是本地统计，不是服务器指标。

## 扫描

Scanner：`.tools/sonar-scanner-8.0.1.6346-windows-x64/bin/sonar-scanner.bat`

Scanner 8 不再接受密码式 `sonar.login` / `sonar.password`。推荐用一次性 token：

```powershell
$env:SONAR_TOKEN = '<临时 token>'
& .tools/sonar-scanner-8.0.1.6346-windows-x64/bin/sonar-scanner.bat `
  '-Dsonar.scanner.skipJreProvisioning=true' `
  '-Dsonar.working.directory=.scannerwork-wave73'
Remove-Item Env:SONAR_TOKEN
```

token 由 Sonar 管理员临时生成；扫描结束后立即撤销。不要把 token 写入命令历史、仓库或日志。`sonar-project.properties` 已配置 LCOV、真实 tsconfig 路径与脚本覆盖率排除项。

## 手动接管

1. 打开上述地址，以 `admin` 和当前用户管理密码登录。
2. 进入 `Ridge` 项目，检查 Dashboard、Measures、Issues、Quality Gate。
3. 生成短期 token，按上面命令执行扫描。
4. 在 SonarQube 后台确认 CE task 为 `SUCCESS`，再读取项目 measures 与 Gate。
5. 撤销 token；不要把密码或 token 交给后续 agent。后续 agent 只应获得地址、项目 key 与临时 token 的运行时注入方式。

## 本轮未闭合

本轮没有新的 scanner/CE 成功上传证据：浏览器控制面不可用，且不允许把用户密码塞进命令行。故仍以最近成功 CE 结果作服务器基线，不声称本轮质量门已修复。
