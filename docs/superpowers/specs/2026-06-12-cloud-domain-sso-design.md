# Cloud 远控生产域名 SSO 设计（父域 Cookie + 主域设备列表）

> 状态：设计稿（待用户审）。日期：2026-06-12。作者：team-lead（remote-review）。
> 关联：`docs/contracts/ridge-cloud-protocol.md`、`2026-06-11-remote-zero-trust-crypto-design.md`、
> `project_desktop_web_remote` / `project_ridge_cloud` 记忆。

## 1. 背景与现状

生产域名拓扑（契约 §1，代码已落）：

- 主域（base zone）：`9527127.xyz`，服务 **web build**（登录/落地 SPA）；HTTP API 挂 `https://9527127.xyz/api/v1`。
- 每设备租户子域：`{device}-{username}.9527127.xyz`，服务 **desktop-app SPA**（cloud controller）。
- 信令 WS：`wss://{device}-{username}.9527127.xyz/ws`。

当前登录态机制：

- 认证全程 **Bearer-only**（`ridge-cloud/src/auth/extract.rs`：只读 `Authorization: Bearer <jwt>`，无 cookie）。
- user JWT scope=user、**exp=30 天**（`auth/jwt.rs`），登录/注册/验证/改名/afdian 各端点经 `state.jwt.issue_user()` 回 `{token, user}`。
- SPA 把 user JWT 存 **localStorage**（`src/lib/remote/cloud/auth.ts`：`ridge.cloud.userToken`）。localStorage 是 **per-origin** 的——主域与子域是不同 origin，**主域登录态不会自动带到子域**。
- 现状跨子域靠 `#token=<jwt>` URL 片段握手（`+layout.svelte` / `consumeHandoffToken`），把 JWT 手动搬到子域。
- 后端已有 `GET /devices`（Bearer user）回用户设备列表 + **在线状态**（`device_routes.rs::list_devices` → `rooms.is_host_present`）；另有 `GET /devices/:name/sessions`、`DELETE /devices/:name`、踢会话等。
- 全仓**无 cookie 先例**。

### 痛点（本设计要解决的）

用户要的简约流程：**主域登录一次 → 进任意设备子域免重登、只输 TOTP**。当前因 localStorage per-origin + `#token` 握手，体验割裂且 token 经 URL 片段传递。

### 零信任约束（不可破坏）

cookie / JWT 只认证**信令与 relay 接入**，**永不进入 E2EE 密钥材料**（设备签名 / TOTP 信道绑定 / 会话密钥派生都在 DataChannel 内，relay 零密码学材料）。本设计只动「接入认证」层，不触碰零信任加密链。

## 2. 目标流程（UX）

```
┌─ 用户访问 9527127.xyz ─────────────────────────────────────────┐
│  未登录 → 登录表单 → 登录成功：后端 Set-Cookie 父域 refresh 会话  │
│  已登录（cookie 有效）→ 直接展示【设备列表】                      │
│                                                                  │
│  【设备列表】：GET /devices（名称 + 在线状态）                    │
│     点某设备 → 跳 https://{device}-{username}.9527127.xyz         │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─ 子域 SPA 加载 ────────────────────────────────────────────────┐
│  启动 bootstrap：GET /auth/session（父域 cookie 自动带上）        │
│   ├─ cookie 有效 → 后端新签【短时 access JWT】+ 回 user           │
│   │     → seed 内存/localStorage → 免重登 → 进 TOTP 门 → 控制     │
│   └─ 无 cookie / 失效 → 跳主域 9527127.xyz 登录（带 redirect 回跳）│
└──────────────────────────────────────────────────────────────┘
```

`#token=` 握手退役。

## 3. Token 模型（refresh 会话 cookie + 短时 access JWT）

采用 **Option B**（OAuth 风格 refresh-in-HttpOnly-cookie + 短时 access token）：

| | refresh 会话（cookie） | access token（JWT） |
|---|---|---|
| 载体 | HttpOnly cookie `ridge_sso`，`Domain=.9527127.xyz` | localStorage/内存，SPA 走 Bearer |
| 内容 | **不透明随机 token**（256-bit CSPRNG），JS 永不可读 | scope=user 的短时 JWT |
| 时效 | 30 天（滑动续期，见 §4.4） | **15 分钟**（短，限 XSS 失窃窗口） |
| 存储 | 服务端 `auth_sessions` 表，**只存 token 的 SHA-256**（原 token 仅在 cookie） | 无状态，签名校验 |
| 可吊销 | ✅ 删行即吊销（登出/踢全端/被盗处置） | ❌ 但 15 分钟自然失效 |

关键安全点：DB 只存 `sha256(token)`——**服务器被攻破也拿不到可用 cookie**（与零信任「服务器被攻破」主题一致）；长期凭证（refresh token）**永不进 JS**；短 access token 限制失窃影响面。

## 4. 后端改动（ridge-cloud）

### 4.1 新表 `auth_sessions`（迁移 + `db/auth_session_repo.rs`）

```sql
CREATE TABLE auth_sessions (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash  TEXT NOT NULL UNIQUE,         -- sha256_hex(raw_token)
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  expires_at  TIMESTAMPTZ NOT NULL,
  last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  user_agent  TEXT,                         -- 仅展示/审计，可空
  CONSTRAINT  auth_sessions_user_fk ...
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions(user_id);
```

repo 方法：`create(user_id, token_hash, ttl, ua)`、`find_valid_by_hash(token_hash) -> Option<Row>`（含未过期判定）、`touch(id)`（更新 last_used + 滑动续期 expires_at）、`delete(id)`、`delete_for_user(user_id)`、`gc_expired()`。

### 4.2 cookie 处理

引入 `axum-extra` 的 `CookieJar`（或手写 `Set-Cookie`/读 `Cookie` 头，二选一，writing-plans 定）。cookie 属性：

```
ridge_sso=<raw_token>; Domain=.9527127.xyz; Path=/; HttpOnly; Secure;
          SameSite=Lax; Max-Age=2592000
```

dev 模式（`is_dev_mode()`，base 含 localhost）：`Domain=.localhost`、保留 `Secure`（本地 HTTPS 自签）；`SameSite=Lax`。

### 4.3 签发点改造（登录类端点）

`register`(验证后) / `login` / `verify_email` / `set_username` / `afdian_claim` 等当前回 `{token, user}` 的路径，统一改为：

1. 生成 raw refresh token（`generate_poll_token` 同款 CSPRNG）→ `auth_session_repo::create(user, sha256_hex(raw), 30d, ua)`。
2. `Set-Cookie ridge_sso=<raw>`（§4.2）。
3. body 的 `token` 改为**短时 access JWT**（`issue_user_access`，15 分钟），不再回 30 天 JWT。

抽出 `issue_session_and_cookie(state, user, jar) -> (jar, TokenUser)` 复用。

### 4.4 新端点

- **`GET /auth/session`**（cookie 认证，非 Bearer）：读 `ridge_sso` → `find_valid_by_hash(sha256(raw))` → 命中：`touch`（滑动续期）+ 新签 access JWT + 回 `{token, user}`；未命中/过期：401。SPA 启动 + access 过期时调它（静默刷新）。
- **`POST /auth/logout`**（cookie）：`delete(session)` + 清 cookie（`Max-Age=0`）。
- （可选，未来）`POST /auth/logout-all`：`delete_for_user`，吊销全端 refresh 会话。

### 4.5 jwt.rs

补 `issue_user_access(user_id, username, plan)`：复用 `issue(...)` 但 ttl 为分钟级（新增按 `Duration` 而非 `ttl_days` 的内部签发，或加 `ACCESS_TTL_MIN=15`）。scope 仍 `User`（access 与原 user token 同 scope，仅 exp 更短）——Bearer 后端零改动。

### 4.6 CORS / 同源

优先**让租户子域自身的 `/api/v1/auth/session` 同源可用**（cookie 因 `Domain=.9527127.xyz` 对子域自动带上，无 CORS）。**待确认**：ridge-cloud 是否在租户子域也路由 `/api`（`router.rs`）。若仅主域有 `/api`，则子域调主域 `/auth/session` 需补 CORS：`Access-Control-Allow-Origin: <子域 origin>` + `Allow-Credentials: true`（动态回显校验过的租户 origin）。

## 5. 前端改动（wind SPA + web build）

### 5.1 `auth.ts` — access token 生命周期

- 不再假设 localStorage 有 30 天 JWT。改为：access JWT 存内存（或 localStorage，短时）+ 启动调 `GET /auth/session` 拉取。
- 暴露 `bootstrapFromCookie(): Promise<bool>`：调 /auth/session，成功 seed user+access、返 true；401 返 false。

### 5.2 `apiClient` — 401 静默刷新拦截

invoke API 收 401 → 调 `/auth/session` 换新 access → 重试一次；再 401 → 判未登录、跳主域登录。避免并发刷新风暴（单 in-flight 刷新 promise 复用）。

### 5.3 `+layout.svelte` / `cloudControllerBoot` — bootstrap 替代 #token

子域加载：先 `bootstrapFromCookie()`；成功 → 进 cloud controller（TOTP 门）；失败 → 跳 `https://9527127.xyz/?redirect=<本子域 url>` 登录。删除 `#token=` 握手与 `consumeHandoffToken`。

### 5.4 web build（主域）设备列表页

主域登录后（或已有 cookie）展示设备列表：`GET /devices`（名称 + 在线状态徽标）。点设备 → `location.href = https://{device}-{username}.9527127.xyz`。离线设备置灰/提示。复用既有 `GET /devices/:name/sessions`、踢会话可作二期。

## 6. 安全分析

- **cookie**：HttpOnly（XSS 读不到 refresh）+ Secure + SameSite=Lax（挡跨站 XHR 偷 token；同站子域 XHR 仍带）+ `Domain=.9527127.xyz`。
- **DB 泄露**：只存 `sha256(token)`，原 token 仅在用户 cookie → 攻破 DB 拿不到可用 cookie。
- **access 短时**：15 分钟，限 XSS 失窃 access token 的滥用窗口。
- **可吊销**：删 `auth_sessions` 行即吊销（登出/被盗处置/未来 logout-all）。
- **CSRF**：mutating API 仍 Bearer（cookie 单独不授权写操作）；`/auth/session` 受 SameSite=Lax + CORS 双重保护，跨站既不带 cookie 也读不到响应。
- **零信任**：cookie/JWT 只认证信令/relay 接入，**不进 E2EE 材料**，服务器被攻破仍无法解密/冒充在用会话（与 `2026-06-11-remote-zero-trust-crypto-design.md` 一致）。
- **会话固定**：登录时新建 session（不复用预置 token），天然免会话固定。
- **（未来硬化，YAGNI 暂不做）**：refresh token 每次刷新轮换 + 复用检测（盗用告警）；登录设备/会话管理 UI。

## 7. 迁移与兼容

- 存量 localStorage 30 天 JWT 用户：下次访问无 refresh cookie → `/auth/session` 401 → 引导重登一次 → 获 cookie。一次性、可接受。
- `#token=` 握手删除；旧链接（带 #token）失效后走正常 cookie/登录路径，不报错。
- 双端同步发版（与 `#token` 删除、access 短时化绑定）：web build + desktop-app + ridge-cloud 同批上线，避免新子域 SPA 调旧后端无 `/auth/session`。

## 8. 测试策略

- **后端**：`auth_session_repo` CRUD + 过期/滑动续期单测；`/auth/session` 命中/过期/无 cookie 三态；登录 Set-Cookie 属性断言；logout 删行 + 清 cookie；`sha256(token)` 存储（原 token 不落库）断言。
- **前端**：`auth.ts.bootstrapFromCookie` 三态（成功/401/网络错）；apiClient 401 单刷新重试 + 并发去重；`+layout` bootstrap 分支（cookie 成功进 controller / 失败跳登录）。
- **集成（运行时清单）**：主域登录 → 设备列表 → 点设备 → 子域免重登只输 TOTP → 控制，本地 `localhost:5001` 全栈复跑；access 过期后静默刷新不掉线；logout 后子域 401 跳登录。

## 9. 待确认项（writing-plans 阶段定）

1. ridge-cloud 是否在租户子域路由 `/api`（决定 `/auth/session` 同源 vs CORS）。
2. access TTL（15 min？）与 refresh TTL（30 天？）、是否滑动续期。
3. cookie 处理选 `axum-extra::CookieJar` 还是手写头。
4. access token 存内存（更安全、刷新页丢失需 re-bootstrap）还是 localStorage（持久但 XSS 面）。建议**localStorage**（与现状一致、刷新页不闪登录），靠 15 分钟时效 + refresh 吊销兜底。
5. refresh 轮换是否本期纳入（默认否，YAGNI）。
6. 主域 `9527127.xyz` 的 **web build 源码位置**（设备列表页落点）——是 wind 仓内构建产物还是独立仓/独立构建，决定 §5.4 改在哪。

## 10. 范围边界（YAGNI）

**本期做**：父域 refresh cookie + `/auth/session` + 短 access + 登录 Set-Cookie + logout + 主域设备列表页 + 子域 bootstrap + `#token` 退役。

**本期不做**：refresh 轮换/复用检测、登录会话管理 UI、记住设备/受信设备、多因子（除既有 TOTP 远控门）、跨账号设备共享。
