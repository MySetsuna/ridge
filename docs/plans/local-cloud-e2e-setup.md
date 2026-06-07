# 本地 cloud e2e 环境（2026-06-07，已跑通后端）

> 目的：在本机起完整 cloud 链路以诊断 B1（dir-children 经云返回空）+ 验证 B2/B3。
> 状态：**cloud 后端已本地跑通**；host↔controller 的 WebRTC e2e 还需若干接线（见 §3）。

## 1. 已跑通：postgres(docker) + ridge-cloud(dev)

本机 PostgreSQL 18 安装损坏（缺 `share/`）、网络限速（无法下载修复）。改用 **docker 缓存的 supabase postgres 镜像**起一个独立容器（无需联网拉取）：

```bash
# 复用缓存镜像 public.ecr.aws/supabase/postgres:15.8.1.085（无 POSTGRES_USER 以绕开 supabase usermap）
docker run -d --name ridge-pg -e POSTGRES_PASSWORD=ridge -p 5433:5432 public.ecr.aws/supabase/postgres:15.8.1.085
docker exec ridge-pg psql -U postgres -c "CREATE DATABASE ridge_cloud;"
# TCP 超级用户：postgres:ridge@localhost:5433
```

ridge-cloud（Rust，**运行时 query 无 query! 宏 → 构建期不需 DB**；启动跑 `sqlx::migrate!` 8 个迁移）：

```bash
cd C:/code/ridge-cloud
DATABASE_URL="postgres://postgres:ridge@localhost:5433/ridge_cloud" \
JWT_SECRET="0123456789abcdef0123456789abcdef0123456789abcdef" \
LEMON_SQUEEZY_SECRET="dummy_local_dev_secret" \
BASE_DOMAIN="localhost" PORT="5050" \
cargo run --bin ridge-cloud
# → "数据库就绪，迁移完成" + "监听就绪 addr=0.0.0.0:5050"
# 验证：curl localhost:5050/ →200, /healthz →200, /ws →400(需 upgrade，路由在)
```

## 2. B1 已诊断到 controller 侧（无需全 e2e）

`scripts/cdp-dirchildren-probe.mjs`（dev:cdp）实测 host `get_directory_children` offset 0/3/6 **分页正确**（total=92）。叠加 S7 conformance（cloudWebrtcAdapter+rpcClient invoke 往返 32 测全过）→ **host OK + transport OK**。故 B1「经云返回空」是 **controller/UI 侧窄边角**（疑 `fileExplorer.ts:490` catch 吞 cloud invoke 错误/超时，或 FileTree 懒加载追加），且可能自 2026-06-04 多次提交后已修。

## 3. 全 WebRTC e2e（✅ 已跑通 2026-06-07）

> 更新：单 realm WebRTC harness 已实现并跑通（`src/lib/remote/cloud/__cloudE2eHarness.ts`
> + `scripts/cdp-cloud-seed.mjs`）。**B1 证伪**（dir-children 经云分页正确 total=92），
> 并**实测确认审计 ①-1 RCE**（云控制端经 `get_remote_info` 读到宿主 LAN TOTP 密钥）。
> 详见 `remote-cloud-security-audit-2026-06-07.md` §5.5。所需的两处使能改动已落地：
> (1) cloud scheme 按回环判定 http/ws（`apiClient.ts`，commit 4e2022a）；
> (2) `app.html` CSP connect-src 放行 `http://localhost:* ws://*.localhost:*`。
> 下面是当时的接线计划，保留供参考。

### （历史）当时仍需接线项

要真机复现 B1 / 验 B2/B3，需把 host(wind) + controller(浏览器) 接到本地 relay：

1. **连接 URL**：`controllerCloudProvider.ts:246` / `ridgeCloudProvider.ts:367` 硬编 `wss://{device}-{username}.{baseDomain}/ws`（TLS + 租户子域）。本地 relay 是 HTTP 单机。需二选一：
   - (a) 本地 TLS（ridge-cloud 自身只 HTTP；需前置 caddy/nginx 自签 + `*.localtest.me`→127.0.0.1 解析），或
   - (b) 给 provider 加 **dev-mode**：`ws://` + 直连 host:port（gate 在构建 flag，勿污染生产路径）。`apiClient.ts` 已有 `RIDGE_CLOUD_BASE_DOMAIN` 构建 define 可复用。
2. **鉴权**：controller 需 userJWT、host 需 deviceJWT（ridge-cloud `auth/jwt.rs`；JWT_SECRET 已知=上面那串）。可经 `/auth/register`+`/auth/login` 拿 userJWT；`genkeys` 二进制或 `/device/bind` 拿 device JWT。
3. **premium 门控**：cloud gate premium → `docker exec ridge-pg psql -U postgres -d ridge_cloud -c "UPDATE users SET plan='premium' WHERE ..."`。
4. **设备配对**：`/device/code|activate|poll`。
5. **两端点**：host = wind `dev:cdp`（CDP 9222）；controller = 第二个浏览器（chrome-devtools MCP）加载本地 SPA（`?cloudHost=<device>&u=<username>`）。
6. **WebRTC**：localhost↔localhost 用 host ICE 候选即可；E2EE 握手在 provider 内。

§3 是一个多组件、多失败点的较大 bring-up（provider dev-mode + 鉴权/配对/premium + 双浏览器 + WebRTC）。鉴于 §2 已证 host/transport OK（B1 大概率窄/已修），是否投入全 bring-up 值得权衡。

## 4. 运行态（本会话）

- docker 容器 `ridge-pg`（postgres 15.8，:5433）：起着。
- ridge-cloud（:5050）：起着（后台 cargo run）。
- 收尾时可 `docker rm -f ridge-pg` + 停 ridge-cloud 进程。
