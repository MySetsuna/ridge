# dev-cli.ps1 — ridge-cli (rdg) 开发模式启动脚本
# 用法:
#   .\scripts\dev-cli.ps1                              # 进入 TUI（默认）
#   .\scripts\dev-cli.ps1 remote --enable               # 设备码配对
#   .\scripts\dev-cli.ps1 remote --daemon               # 守护模式
#   .\scripts\dev-cli.ps1 login                         # 账号密码登录
#   .\scripts\dev-cli.ps1 connect <host>                # 连接 LAN host
#
# 环境变量:
#   RIDGE_BASE_DOMAIN   cloud 地址（默认 localhost:3000）

$RIDGE_BASE_DOMAIN = if ($env:RIDGE_BASE_DOMAIN) { $env:RIDGE_BASE_DOMAIN } else { "localhost:3000" }
$env:RIDGE_BASE_DOMAIN = $RIDGE_BASE_DOMAIN

Write-Host "═══════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  ridge-cli (rdg) 开发模式" -ForegroundColor Cyan
Write-Host "  RIDGE_BASE_DOMAIN = $RIDGE_BASE_DOMAIN" -ForegroundColor Yellow
Write-Host "═══════════════════════════════════════════" -ForegroundColor Cyan

cargo run -p ridge-cli -- $args
