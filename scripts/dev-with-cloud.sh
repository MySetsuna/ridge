#!/usr/bin/env bash
# 组合启动：ridge-cloud 后端 (HTTPS) + wind 前端 (Tier 2: 组合)
# 用法: ./scripts/dev-with-cloud.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WIND_DIR="$(dirname "$SCRIPT_DIR")"
RIDGE_CLOUD_DIR="/c/code/ridge-cloud"

echo "════════════════════════════════════════════════"
echo "  组合开发环境: ridge-cloud (HTTPS:5001/5002) + wind (5173/5174)"
echo "════════════════════════════════════════════════"
echo "  ridge-cloud API:     https://localhost:5001"
echo "  ridge-cloud Admin:   http://localhost:5002"
echo "  wind Main:           http://localhost:5173"
echo "  wind Remote:         http://localhost:5174"
echo "════════════════════════════════════════════════"

# —— 1. 启动 ridge-cloud (HTTPS, 后台) ——
echo "🚀 启动 ridge-cloud 后端 (HTTPS)..."
cd "$RIDGE_CLOUD_DIR"

for f in .env.dev .env.local .env; do
    [[ -f "$f" ]] && { set -a; source "$f"; set +a; echo "📄 加载 $f"; }
done

# 检查必要变量
: "${DATABASE_URL:?❌ DATABASE_URL 未设置，请配置 .env.local}"
: "${JWT_SECRET:?❌ JWT_SECRET 未设置}"

export RIDGE_CLOUD_DEV_HTTPS=1

cargo run --bin ridge-cloud &
RIDGE_CLOUD_PID=$!
echo "   ridge-cloud PID: $RIDGE_CLOUD_PID"

# —— 2. 等待 ridge-cloud 就绪 (HTTPS 健康检查，跳过证书验证) ——
echo "⏳ 等待 ridge-cloud 就绪 (https://localhost:5001/api/v1/health)..."
for i in {1..30}; do
    if curl -k -s "https://localhost:5001/api/v1/health" >/dev/null 2>&1; then
        echo "✅ ridge-cloud 已就绪 (HTTPS)"
        break
    fi
    sleep 1
    [[ $i -eq 30 ]] && { echo "❌ ridge-cloud 启动超时"; kill $RIDGE_CLOUD_PID 2>/dev/null; exit 1; }
done

# —— 3. 启动 wind 前端 ——
echo "🚀 启动 wind 前端..."
cd "$WIND_DIR"

export RIDGE_CLOUD_BASE_DOMAIN="localhost:5001"

[[ ! -d "node_modules" ]] && { echo "📦 安装 wind 依赖..."; pnpm install; }

pnpm dev &
WIND_MAIN_PID=$!

sleep 2

pnpm dev:remote &
WIND_REMOTE_PID=$!

echo ""
echo "════════════════════════════════════════════════"
echo "  ✅ 所有开发服务已启动"
echo "════════════════════════════════════════════════"
echo "  ridge-cloud API:     https://localhost:3000"
echo "  ridge-cloud Admin:   http://localhost:5001"
echo "  wind Main:           http://localhost:5173"
echo "  wind Remote:         http://localhost:5174"
echo ""
echo "  👉 请在另一个终端运行: pnpm tauri dev"
echo "════════════════════════════════════════════════"

# —— 4. 清理 & 等待 ——
cleanup() {
    echo ""
    echo "🛑 正在停止所有服务..."
    kill $RIDGE_CLOUD_PID $WIND_MAIN_PID $WIND_REMOTE_PID 2>/dev/null || true
    exit 0
}
trap cleanup INT TERM

wait $RIDGE_CLOUD_PID $WIND_MAIN_PID $WIND_REMOTE_PID