#!/usr/bin/env bash
# dev-cli.sh — ridge-cli (rdg) 开发模式启动脚本
# 用法:
#   ./scripts/dev-cli.sh                              # 进入 TUI（默认）
#   ./scripts/dev-cli.sh remote --enable               # 设备码配对
#   ./scripts/dev-cli.sh remote --daemon               # 守护模式
#   ./scripts/dev-cli.sh login                         # 账号密码登录
#   ./scripts/dev-cli.sh connect <host>                # 连接 LAN host
#
# 环境变量:
#   RIDGE_BASE_DOMAIN  cloud 地址（默认 localhost:3000）

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

export RIDGE_BASE_DOMAIN="${RIDGE_BASE_DOMAIN:-localhost:3000}"

echo "═══════════════════════════════════════════"
echo "  ridge-cli (rdg) 开发模式"
echo "  RIDGE_BASE_DOMAIN=$RIDGE_BASE_DOMAIN"
echo "═══════════════════════════════════════════"

cd "$ROOT_DIR"
exec cargo run -p ridge-cli -- "$@"
