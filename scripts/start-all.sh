#!/usr/bin/env bash
# 全栈启动：PostgreSQL + ridge-cloud (HTTPS) + wind 前端 (Tier 3: 全栈)
# 用法: ./scripts/start-all.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WIND_DIR="$(dirname "$SCRIPT_DIR")"
RIDGE_CLOUD_DIR="/c/code/ridge-cloud"

echo "═══════════════════════════════════════════════════════"
echo "  全栈开发环境: PostgreSQL + ridge-cloud (HTTPS) + wind"
echo "═══════════════════════════════════════════════════════"
echo "  PostgreSQL:          localhost:5432"
echo "  ridge-cloud API:     https://localhost:5001"
echo "  ridge-cloud Admin:   http://localhost:5002"
echo "  wind Main:           http://localhost:5173"
echo "  wind Remote:         http://localhost:5174"
echo "═══════════════════════════════════════════════════════"

# —— 1. 启动/检查 PostgreSQL (Docker) ——
echo "🐘 检查 PostgreSQL..."
if ! pg_isready -h localhost -p 5432 >/dev/null 2>&1; then
    echo "🚀 启动 PostgreSQL (Docker)..."
    docker run -d --name ridge-cloud-postgres \
        -e POSTGRES_PASSWORD=*** \
        -e POSTGRES_DB=ridge_cloud \
        -p 5432:5432 \
        -v ridge-cloud-pgdata:/var/lib/postgresql/data \
        postgres:16 >/dev/null 2>&1 || docker start ridge-cloud-postgres >/dev/null 2>&1 || true
    echo "⏳ 等待 PostgreSQL 就绪..."
    for i in {1..15}; do
        pg_isready -h localhost -p 5432 >/dev/null 2>&1 && { echo "✅ PostgreSQL 就绪"; break; }
        sleep 1
        [[ $i -eq 15 ]] && { echo "❌ PostgreSQL 启动超时"; exit 1; }
    done
else
    echo "✅ PostgreSQL 已运行"
fi

# —— 2. 设置 DATABASE_URL（如果未设置） ——
export DATABASE_URL="${DATABASE_URL:-postgres://postgres:***@localhost:5432/ridge_cloud}"

# —— 3. 启动 ridge-cloud (HTTPS, 后台) ——
echo "🚀 启动 ridge-cloud 后端 (HTTPS)..."
cd "$RIDGE_CLOUD_DIR"

for f in .env.dev .env.local .env; do
    [[ -f "$f" ]] && { set -a; source "$f"; set +a; }
done

export RIDGE_CLOUD_DEV_HTTPS=1

cargo run --bin ridge-cloud &
RIDGE_CLOUD_PID=$!
echo "   ridge-cloud PID: $RIDGE_CLOUD_PID"

# —— 4. 等待 ridge-cloud 就绪 (HTTPS) ——
echo "⏳ 等待 ridge-cloud 就绪 (https://localhost:5001/api/v1/health)..."
for i in {1..30}; do
    if curl -k -s "https://localhost:5001/api/v1/health" >/dev/null 2>&1; then
        echo "✅ ridge-cloud 已就绪 (HTTPS)"
        break
    fi
    sleep 1
    [[ $i -eq 30 ]] && { echo "❌ ridge-cloud 启动超时"; kill $RIDGE_CLOUD_PID 2>/dev/null; exit 1; }
done

# —— 5. 启动 wind 前端 ——
echo "🚀 启动 wind 前端..."
cd "$WIND_DIR"

export RIDGE_CLOUD_BASE_DOMAIN="localhost:5001"

[[ ! -d "node_modules" ]] && { echo "📦 安装依赖..."; pnpm install; }

pnpm dev &
WIND_MAIN_PID=$!
sleep 2
pnpm dev:remote &
WIND_REMOTE_PID=$!

echo ""
echo "═══════════════════════════════════════════════════════"
echo "  ✅ 全栈开发环境已启动"
echo "═══════════════════════════════════════════════════════"
echo "  PostgreSQL:          localhost:5432"
echo "  ridge-cloud API:     https://localhost:5001"
echo "  ridge-cloud Admin:   http://localhost:5002"
echo "  wind Main:           http://localhost:5173"
echo "  wind Remote:         http://localhost:5174"
echo ""
echo "  👉 请在另一个终端运行: pnpm tauri dev"
echo "═══════════════════════════════════════════════════════"

# —— 6. 清理 & 等待 ——
cleanup() {
    echo ""
    echo "🛑 正在停止所有服务..."
    kill $RIDGE_CLOUD_PID $WIND_MAIN_PID $WIND_REMOTE_PID 2>/dev/null || true
    # 注意: 不自动停止 PostgreSQL (保持数据持久化)
    exit 0
}
trap cleanup INT TERM

wait $RIDGE_CLOUD_PID $WIND_MAIN_PID $WIND_REMOTE_PID