#!/usr/bin/env bash
# wind 前端开发启动脚本 (Tier 1: 单服务)
# 用法: ./scripts/dev-wind.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "═══════════════════════════════════════"
echo "  wind 前端开发环境 (Main:5173, Remote:5174)"
echo "═══════════════════════════════════════"

# —— 1. 连接本地 ridge-cloud (HTTPS) ——
export RIDGE_CLOUD_BASE_DOMAIN="${RIDGE_CLOUD_BASE_DOMAIN:-https://localhost:5000}"
echo "✅ RIDGE_CLOUD_BASE_DOMAIN=$RIDGE_CLOUD_BASE_DOMAIN"

# —— 2. 检查依赖 ——
cd "$ROOT_DIR"
if [[ ! -d "node_modules" ]]; then
    echo "📦 安装依赖..."
    pnpm install
fi

# —— 3. 启动主 Vite dev server (5173) ——
echo "🚀 启动主 Vite dev server (端口 5173)..."
pnpm dev &
MAIN_PID=$!
echo "   Main PID: $MAIN_PID"

# 等待主服务器启动
sleep 2

# —— 4. 启动 Remote Vite dev server (5174) ——
echo "🚀 启动 Remote Vite dev server (端口 5174)..."
pnpm dev:remote &
REMOTE_PID=$!
echo "   Remote PID: $REMOTE_PID"

echo ""
echo "═══════════════════════════════════════"
echo "  ✅ 前端服务已启动"
echo "  Main:  http://localhost:5173"
echo "  Remote: http://localhost:5174"
echo ""
echo "  👉 请在另一个终端运行: pnpm tauri dev"
echo "═══════════════════════════════════════"

# —— 5. 清理 & 等待 ——
cleanup() {
    echo ""
    echo "🛑 正在停止前端服务..."
    kill $MAIN_PID $REMOTE_PID 2>/dev/null || true
    exit 0
}
trap cleanup INT TERM

wait $MAIN_PID $REMOTE_PID