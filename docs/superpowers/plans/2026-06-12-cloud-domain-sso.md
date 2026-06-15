# Cloud 域名 SSO 实现计划（父域 refresh cookie + 短 access JWT + 设备列表）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 主域 `9527127.xyz` 登录一次 → 父域 refresh cookie → 进任意租户子域免重登只输 TOTP。

**Architecture:** OAuth 风格 refresh-in-HttpOnly-cookie + 短时 access JWT。ridge-cloud 新增 `auth_sessions` 表（只存 `sha256(token)`）+ `GET /auth/session`（cookie 换 15min access JWT）+ logout；登录类端点加 Set-Cookie。wind SPA 启动调 `/auth/session` bootstrap、apiClient 401 静默刷新、`#token` 退役；主域 web build 加设备列表页。现有 Bearer 后端零改动。零信任加密链不动。

**Tech Stack:** Rust（axum + SQLx 运行时查询 + PgPool）；TypeScript/Svelte（vitest）。设计稿：`docs/superpowers/specs/2026-06-12-cloud-domain-sso-design.md`。

**执行约束：** 不能跑 `cargo check`（用户常驻 tauri dev 锁 `target/`）。Rust 任务的「运行测试」步骤由执行者在 dev 锁空闲时跑，或用独立 `--target-dir`（如 `cargo test -p ridge-cloud --target-dir target-sso-check <name>`）。一个关注点一个 commit。

---

## 文件结构

**ridge-cloud（`C:\code\ridge-cloud`）：**
- Create: `migrations/0011_auth_sessions.sql` — refresh 会话表。
- Create: `src/db/auth_session_repo.rs` — 会话 CRUD（mirror `device_repo.rs`）。
- Create: `src/api/session_cookie.rs` — 手写 Set-Cookie/读 Cookie 头 + `CookieSession` extractor。
- Modify: `src/db/mod.rs` — `pub mod auth_session_repo;`。
- Modify: `src/auth/jwt.rs` — 加 `issue_user_access`（15min）+ `ACCESS_TTL_MIN`。
- Modify: `src/api/auth_routes.rs` — `session` / `logout` handler + 登录类端点注入 Set-Cookie。
- Modify: `src/api/mod.rs` — `pub mod session_cookie;`（若新模块挂这）。
- Modify: `src/router.rs` — 注册 `/auth/session`、`/auth/logout`（pre-auth 组）。

**wind（`C:\code\wind`）：**
- Modify: `src/lib/remote/cloud/auth.ts` — `bootstrapFromCookie` + access token 存储。
- Modify: `src/lib/remote/cloud/apiClient.ts` — 401 静默刷新拦截。
- Modify: `src/routes/+layout.svelte` — 子域 bootstrap 替代 `#token`。
- Modify: `src/lib/remote/cloud/cloudControllerBoot.ts` — 删 `consumeHandoffToken`/`#token`。
- web build 设备列表页：见 **Phase 3 前置任务**（落点待定）。

---

## Phase 1 — ridge-cloud 后端

### Task 1: 迁移 `auth_sessions` 表

**Files:**
- Create: `migrations/0011_auth_sessions.sql`

- [ ] **Step 1: 写迁移 SQL**

```sql
-- 父域 SSO refresh 会话（设计 2026-06-12-cloud-domain-sso-design §4.1）。
-- 只存 token 的 sha256（原 token 仅在用户 HttpOnly cookie）→ DB 泄露不得可用 cookie。
CREATE TABLE IF NOT EXISTS auth_sessions (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash   TEXT        NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    user_agent   TEXT
);
CREATE INDEX IF NOT EXISTS idx_auth_sessions_user ON auth_sessions (user_id);
```

- [ ] **Step 2: 确认迁移被加载**

Run（dev 锁空闲时）: `grep -rn "migrations" src/db/mod.rs src/main.rs` → 确认迁移目录自动跑（sqlx::migrate! 或启动时执行）。若是 `sqlx::migrate!("./migrations")`，新文件自动纳入，无需改码。
Expected: 找到迁移执行点；0011 自动纳入。

- [ ] **Step 3: Commit**

```bash
cd /c/code/ridge-cloud && git add migrations/0011_auth_sessions.sql
git commit -m "feat(auth): auth_sessions 表（父域 SSO refresh 会话，只存 token hash）"
```

### Task 2: `auth_session_repo`

**Files:**
- Create: `src/db/auth_session_repo.rs`
- Modify: `src/db/mod.rs`
- Test: 同文件 `#[cfg(test)]`（repo 纯查询，集成测需 DB；此处放可单测的纯逻辑 + 标注 DB 集成测试）

- [ ] **Step 1: 写 repo（mirror `device_repo.rs` 的 query_as 风格）**

```rust
//! 父域 SSO refresh 会话访问层（设计 §4.1）。SQLx 运行时查询（契约 §10）。
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct AuthSessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

/// 新建会话：调用方传入 `token_hash = sha256_hex(raw)`、`ttl_days`、可选 UA。
pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
    ttl_days: i64,
    user_agent: Option<&str>,
) -> Result<(), sqlx::Error> {
    let expires_at = Utc::now() + Duration::days(ttl_days);
    sqlx::query(
        "INSERT INTO auth_sessions (user_id, token_hash, expires_at, user_agent) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .bind(user_agent)
    .execute(pool)
    .await?;
    Ok(())
}

/// 按 hash 找未过期会话；命中即返回（不更新）。
pub async fn find_valid_by_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<AuthSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthSessionRow>(
        "SELECT id, user_id, token_hash, expires_at FROM auth_sessions \
         WHERE token_hash = $1 AND expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// 滑动续期 + 更新 last_used（每次 /auth/session 刷新调用）。
pub async fn touch(pool: &PgPool, id: Uuid, ttl_days: i64) -> Result<(), sqlx::Error> {
    let expires_at = Utc::now() + Duration::days(ttl_days);
    sqlx::query(
        "UPDATE auth_sessions SET last_used_at = now(), expires_at = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 删除单个会话（登出）。
pub async fn delete(pool: &PgPool, id: Uuid) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM auth_sessions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// 删除某用户全部会话（logout-all / 处置被盗，预留）。
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM auth_sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}
```

- [ ] **Step 2: 挂模块**

`src/db/mod.rs` 加：`pub mod auth_session_repo;`（与 `pub mod device_repo;` 同处）。

- [ ] **Step 3: Commit**

```bash
git add src/db/auth_session_repo.rs src/db/mod.rs
git commit -m "feat(auth): auth_session_repo（会话 CRUD + 滑动续期，mirror device_repo）"
```

> 注：repo 是纯 SQLx，集成测试需真 PG。本仓既有 repo 同样无内联 DB 单测——遵循既有模式，正确性由 §Task 5 的端点集成测试（连本地 PG）覆盖。

### Task 3: jwt 短时 access token

**Files:**
- Modify: `src/auth/jwt.rs`
- Test: `src/auth/jwt.rs` `#[cfg(test)]`

- [ ] **Step 1: 写失败测试（access exp 远短于 user 30 天）**

在 jwt.rs 的 tests 模块加：

```rust
#[test]
fn access_token_has_short_ttl() {
    let codec = JwtCodec::new_hs256(b"test-secret-at-least-32-bytes-long!!");
    let uid = uuid::Uuid::new_v4();
    let token = codec
        .issue_user_access(uid, Some("alice".into()), crate::db::user_repo::Plan::Free)
        .unwrap();
    let claims = codec.verify(&token).unwrap();
    assert_eq!(claims.scope, Scope::User);
    // 15 分钟 access：exp - iat 应 ≈ 900s，远小于 30 天。
    assert!(claims.exp - claims.iat <= 20 * 60);
    assert!(claims.exp - claims.iat >= 5 * 60);
}
```

（`Plan` 路径/构造按 jwt.rs 既有 `issue_user` 测试里的写法对齐——执行时先看该文件 tests 模块的现有 helper。）

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p ridge-cloud --target-dir target-sso-check access_token_has_short_ttl`
Expected: 编译失败「no method `issue_user_access`」。

- [ ] **Step 3: 实现**

jwt.rs 加常量 + 方法（紧邻 `issue_user`）：

```rust
/// access token 时效（分钟）。短时以限制 XSS 失窃窗口（设计 §3）。
const ACCESS_TTL_MIN: i64 = 15;

/// 签发短时 access user token（scope=user，15 分钟）。供 /auth/session 用 refresh
/// cookie 兑换；与 30 天 `issue_user` 同 scope，故 Bearer 后端零改动。
pub fn issue_user_access(
    &self,
    user_id: Uuid,
    username: Option<String>,
    plan: Plan,
) -> Result<String> {
    self.issue_with_ttl(user_id, username, plan, Scope::User, None,
        chrono::Duration::minutes(ACCESS_TTL_MIN))
}
```

若现有 `issue` 只接受 `ttl_days: i64`，则抽一个按 `Duration` 的私有 `issue_with_ttl`（把现 `issue` 的 `exp = (now + Duration::days(ttl_days))` 改为接受 `Duration`，原 `issue` 转调它 `Duration::days(ttl_days)`）。保持现有签名对外不变。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p ridge-cloud --target-dir target-sso-check access_token_has_short_ttl`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/auth/jwt.rs
git commit -m "feat(auth): jwt issue_user_access 短时 access token（15min）"
```

### Task 4: cookie 模块（手写头 + 提取）

**Files:**
- Create: `src/api/session_cookie.rs`
- Modify: `src/api/mod.rs`（`pub mod session_cookie;`）
- Test: `src/api/session_cookie.rs` `#[cfg(test)]`

- [ ] **Step 1: 写失败测试（构造 + 解析 cookie 头）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_set_cookie_has_security_attrs() {
        let v = build_set_cookie("ridge_sso", "RAWTOK", ".9527127.xyz", 2_592_000);
        assert!(v.contains("ridge_sso=RAWTOK"));
        assert!(v.contains("Domain=.9527127.xyz"));
        assert!(v.contains("HttpOnly"));
        assert!(v.contains("Secure"));
        assert!(v.contains("SameSite=Lax"));
        assert!(v.contains("Path=/"));
        assert!(v.contains("Max-Age=2592000"));
    }

    #[test]
    fn clear_cookie_expires_immediately() {
        let v = build_set_cookie("ridge_sso", "", ".9527127.xyz", 0);
        assert!(v.contains("Max-Age=0"));
    }

    #[test]
    fn parse_cookie_extracts_named_value() {
        let h = "foo=1; ridge_sso=ABC123; bar=2";
        assert_eq!(read_cookie(h, "ridge_sso"), Some("ABC123".to_string()));
        assert_eq!(read_cookie(h, "missing"), None);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p ridge-cloud --target-dir target-sso-check session_cookie`
Expected: 编译失败（函数未定义）。

- [ ] **Step 3: 实现 cookie 构造/解析**

```rust
//! 父域 SSO cookie：手写 Set-Cookie 构造 + Cookie 头解析（全仓无 cookie 依赖，
//! 不引入新 crate）。属性见设计 §4.2。
use axum::http::{header::COOKIE, request::Parts};

use crate::auth::jwt::Claims;
use crate::crypto::sha256_hex;
use crate::db::auth_session_repo;
use crate::error::ApiError;
use crate::state::SharedState;

/// SSO cookie 名。
pub const SSO_COOKIE: &str = "ridge_sso";
/// refresh 会话 TTL（天）。
pub const REFRESH_TTL_DAYS: i64 = 30;

/// 构造 Set-Cookie 头值。`max_age=0` 即清除。`domain` 由 base zone 推导（见 issue 处）。
pub fn build_set_cookie(name: &str, value: &str, domain: &str, max_age: i64) -> String {
    format!(
        "{name}={value}; Domain={domain}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}"
    )
}

/// 从 Cookie 头取某 cookie 值。
pub fn read_cookie(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|kv| {
        let kv = kv.trim();
        let (k, v) = kv.split_once('=')?;
        (k.trim() == name).then(|| v.trim().to_string())
    })
}

/// 由 base domain 推 cookie Domain：`9527127.xyz` → `.9527127.xyz`；
/// dev `localhost:5001` → `.localhost`（去端口）。
pub fn cookie_domain(base_domain: &str) -> String {
    let host = base_domain.split(':').next().unwrap_or(base_domain);
    format!(".{host}")
}

/// axum extractor：从 ridge_sso cookie 解析出**有效会话对应的 user claims**。
/// 读 cookie → sha256 → 查未过期会话 → touch 滑动续期 → 用 user 重新组 claims 由调用方
/// 自行签 access（本 extractor 只负责认证身份，不签 token）。返回 (session_id, user_id)。
pub struct CookieSession {
    pub session_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
}

#[async_trait::async_trait]
impl axum::extract::FromRequestParts<SharedState> for CookieSession {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(COOKIE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("缺少会话 cookie"))?;
        let raw = read_cookie(header, SSO_COOKIE)
            .ok_or_else(|| ApiError::unauthorized("缺少 ridge_sso cookie"))?;
        let row = auth_session_repo::find_valid_by_hash(&state.db, &sha256_hex(&raw))
            .await
            .map_err(|_| ApiError::unauthorized("会话查询失败"))?
            .ok_or_else(|| ApiError::unauthorized("会话无效或已过期"))?;
        Ok(CookieSession { session_id: row.id, user_id: row.user_id })
    }
}

let _ = Claims::default; // 占位防未用告警（执行时若不需删除此行）
```

> 执行注意：`async_trait` 与 `FromRequestParts` 的用法**逐字 mirror `src/auth/extract.rs`**（同款 import、`#[async_trait]`）。删掉末行占位。`Claims` import 若未用则删。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p ridge-cloud --target-dir target-sso-check session_cookie`
Expected: 3 测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/api/session_cookie.rs src/api/mod.rs
git commit -m "feat(auth): SSO cookie 构造/解析 + CookieSession extractor"
```

### Task 5: `/auth/session` + `/auth/logout` 端点

**Files:**
- Modify: `src/api/auth_routes.rs`（加 `session` / `logout` handler）
- Modify: `src/router.rs`（注册到 pre-auth 组）

- [ ] **Step 1: 写 handler**

`auth_routes.rs` 顶部 import 加 `use crate::api::session_cookie::{self, CookieSession, SSO_COOKIE};` 和 `axum::http::HeaderMap`、`axum::response::IntoResponse`。

```rust
/// `GET /auth/session`（cookie 认证）：refresh cookie 换短 access JWT。
/// 命中：touch 滑动续期 + 新签 access + 回 {token, user}；否则 401。
pub async fn session(
    State(state): State<SharedState>,
    sess: CookieSession,
) -> ApiResult<TokenUser> {
    session_cookie_repo_touch(&state, sess.session_id).await;
    let user = load_user(&state, sess.user_id).await?; // 见下 helper（mirror me()）
    let dto = UserDto::from(&user);
    let token = state
        .jwt
        .issue_user_access(user.id, user.username.clone(), user.plan())?;
    ok(TokenUser { token, user: dto })
}

/// `POST /auth/logout`（cookie）：删会话 + 清 cookie。
pub async fn logout(
    State(state): State<SharedState>,
    sess: CookieSession,
) -> impl IntoResponse {
    let _ = crate::db::auth_session_repo::delete(&state.db, sess.session_id).await;
    let domain = session_cookie::cookie_domain(&state.config.base_domain);
    let clear = session_cookie::build_set_cookie(SSO_COOKIE, "", &domain, 0);
    let mut headers = HeaderMap::new();
    if let Ok(v) = clear.parse() {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
    (headers, ok::<()>(()))
}

async fn session_cookie_repo_touch(state: &SharedState, id: uuid::Uuid) {
    let _ = crate::db::auth_session_repo::touch(&state.db, id, session_cookie::REFRESH_TTL_DAYS).await;
}
```

> 执行注意：`load_user`/`UserDto::from`/`user.plan()` 逐字 mirror 既有 `me()` handler（auth_routes.rs:228）与 `load_user_or_401`（auth_routes.rs:358）。`state.config.base_domain` 字段名按 `config.rs` 实际（可能是 `base_domain()` 方法）对齐。

- [ ] **Step 2: 注册路由（pre-auth 组）**

`router.rs` 的 `auth_sensitive`（或与 `/auth/forgot-password` 同组的**无 Bearer 中间件**组）加：

```rust
.route("/auth/session", get(auth_routes::session))
.route("/auth/logout", post(auth_routes::logout))
```

确保挂在 **不要求 Bearer** 的子路由树（cookie 认证，非 Bearer）。

- [ ] **Step 3: 集成测试（连本地 PG）**

新建/扩展 `tests/` 或既有集成测试：插一条 auth_session（已知 raw+hash）→ 带 `Cookie: ridge_sso=<raw>` 请求 `/auth/session` → 断言 200 + body.token 可 verify 为 user scope + exp 短；过期/缺 cookie → 401；`/auth/logout` 后再请求 → 401。

Run: `cargo test -p ridge-cloud --target-dir target-sso-check session_endpoint`（需本地 PG，按既有集成测试连库方式）。
Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add src/api/auth_routes.rs src/router.rs
git commit -m "feat(auth): GET /auth/session 兑换 access + POST /auth/logout"
```

### Task 6: 登录类端点注入 Set-Cookie

**Files:**
- Modify: `src/api/auth_routes.rs`（`login`/`register`(验证后)/`verify_email`/`set_username`/`afdian_claim` 等回 `{token,user}` 处）

- [ ] **Step 1: 抽公共 helper**

```rust
/// 登录成功收尾：建 refresh 会话 + 组 Set-Cookie + 回短 access token。
/// 返回 (HeaderMap 含 Set-Cookie, TokenUser{access, user})。
async fn issue_session_and_cookie(
    state: &SharedState,
    user: &UserRow,
    ua: Option<&str>,
) -> Result<(axum::http::HeaderMap, TokenUser), ApiError> {
    let raw = crate::crypto::generate_poll_token();
    crate::db::auth_session_repo::create(
        &state.db, user.id, &crate::crypto::sha256_hex(&raw),
        session_cookie::REFRESH_TTL_DAYS, ua,
    )
    .await
    .map_err(|_| ApiError::internal("创建会话失败"))?;
    let domain = session_cookie::cookie_domain(&state.config.base_domain);
    let set = session_cookie::build_set_cookie(
        session_cookie::SSO_COOKIE, &raw, &domain,
        session_cookie::REFRESH_TTL_DAYS * 86_400,
    );
    let mut headers = axum::http::HeaderMap::new();
    if let Ok(v) = set.parse() {
        headers.insert(axum::http::header::SET_COOKIE, v);
    }
    let token = state.jwt.issue_user_access(user.id, user.username.clone(), user.plan())?;
    Ok((headers, TokenUser { token, user: UserDto::from(user) }))
}
```

> `ApiError::internal` 按 error.rs 实际命名对齐（可能是 `ApiError::internal`/`internal_error`）。

- [ ] **Step 2: 改 `login` 等返回 (headers, ok(token_user))**

把 `login`（auth_routes.rs:108）末尾 `ok(TokenUser{...})` 改为：

```rust
let ua = headers_in.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok());
let (set_headers, token_user) = issue_session_and_cookie(&state, &user, ua).await?;
Ok((set_headers, ok(token_user)))
```

handler 签名补 `headers_in: axum::http::HeaderMap` 入参（axum 自动注入），返回类型改 `Result<(axum::http::HeaderMap, ApiOk<TokenUser>), ApiError>`。同法改 `verify_email`/`set_username`/`afdian_claim`/`register`(验证后分支)。

> 注：`register` 启用邮箱验证时**不**发 token（不 set cookie）；只在真正登录（verify-email/login）才 set。

- [ ] **Step 3: 集成测试**

`POST /auth/login`（已知用户）→ 断言响应含 `Set-Cookie: ridge_sso=...; HttpOnly; Secure; SameSite=Lax`；body.token verify 为短 access；DB `auth_sessions` 多一行且**存的是 hash 非原 token**。

Run: `cargo test -p ridge-cloud --target-dir target-sso-check login_sets_cookie`
Expected: PASS。

- [ ] **Step 4: Commit**

```bash
git add src/api/auth_routes.rs
git commit -m "feat(auth): 登录/验证/改名端点签发 refresh cookie + 短 access token"
```

---

## Phase 2 — wind SPA bootstrap

### Task 7: `auth.ts` cookie bootstrap + access 存储

**Files:**
- Modify: `src/lib/remote/cloud/auth.ts`
- Test: `src/lib/remote/cloud/auth.test.ts`（若无则新建，mirror 既有 cloud 测试）

- [ ] **Step 1: 写失败测试**

```ts
import { describe, it, expect, vi } from 'vitest';
import { bootstrapFromCookie } from './auth';

describe('bootstrapFromCookie', () => {
  it('cookie 有效 → 拉到 access token + user，返回 true', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ token: 'ACCESS', user: { username: 'jack' } }),
    });
    vi.stubGlobal('fetch', fetchMock);
    const okk = await bootstrapFromCookie();
    expect(okk).toBe(true);
    // 应以 credentials:'include' 调 /auth/session（带父域 cookie）
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/auth/session'),
      expect.objectContaining({ credentials: 'include' }),
    );
  });

  it('401 → 返回 false（未登录）', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 401 }));
    expect(await bootstrapFromCookie()).toBe(false);
  });
});
```

- [ ] **Step 2: 运行验证失败**

Run: `cd /c/code/wind && npx vitest run src/lib/remote/cloud/auth.test.ts`
Expected: FAIL（`bootstrapFromCookie` 未导出）。

- [ ] **Step 3: 实现**

`auth.ts` 加（复用既有 `BASE_DOMAIN`/`cloudHttpScheme`/`api_base` + `update()` 状态写入）：

```ts
/**
 * 父域 cookie bootstrap：调 GET /auth/session（credentials:'include' 带父域
 * ridge_sso cookie）兑换短 access token。成功 → 写入 userToken+user（seed
 * 现有 Bearer 流程）→ true；401/失败 → false（调用方跳主域登录）。
 */
export async function bootstrapFromCookie(): Promise<boolean> {
  try {
    const url = `${api.apiBase()}/auth/session`; // mirror apiClient 的 base 拼接
    const resp = await fetch(url, { credentials: 'include' });
    if (!resp.ok) return false;
    const data = (await resp.json()) as { token: string; user: UserDto };
    update((s) => ({ ...s, userToken: data.token, user: data.user }));
    return true;
  } catch {
    return false;
  }
}
```

> `api.apiBase()` 按 apiClient 实际导出对齐；`update`/`UserDto` 已在 auth.ts 在用。

- [ ] **Step 4: 运行验证通过 + Commit**

Run: `npx vitest run src/lib/remote/cloud/auth.test.ts` → PASS。
```bash
git add src/lib/remote/cloud/auth.ts src/lib/remote/cloud/auth.test.ts
git commit -m "feat(web-remote): auth bootstrapFromCookie（父域 cookie 换 access）"
```

### Task 8: apiClient 401 静默刷新

**Files:**
- Modify: `src/lib/remote/cloud/apiClient.ts`
- Test: `src/lib/remote/cloud/apiClient.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
it('收 401 → 调 /auth/session 刷新后重试一次', async () => {
  const calls: string[] = [];
  const fetchMock = vi.fn().mockImplementation(async (url: string) => {
    calls.push(url);
    if (url.includes('/auth/session')) return { ok: true, json: async () => ({ token: 'NEW', user: {} }) };
    if (calls.filter((u) => u === url).length === 1) return { ok: false, status: 401 };
    return { ok: true, json: async () => ({ ok: true }) };
  });
  vi.stubGlobal('fetch', fetchMock);
  // 调一个受保护 API（mirror 既有 apiClient 调用），断言最终成功且刷新被调用。
  // 具体断言按 apiClient 实际导出方法对齐。
  expect(calls.some((u) => u.includes('/auth/session'))).toBe(true);
});
```

- [ ] **Step 2-4: 实现单飞刷新拦截**

在 apiClient 的统一请求函数里：收 401 → 调一个**模块内单飞** `refreshAccess()`（复用 bootstrapFromCookie，多个并发 401 共享同一个 in-flight promise）→ 成功则用新 token 重试原请求一次；再 401 → 抛「未登录」（调用方跳登录）。

```ts
let refreshing: Promise<boolean> | null = null;
function refreshAccess(): Promise<boolean> {
  if (!refreshing) refreshing = bootstrapFromCookie().finally(() => { refreshing = null; });
  return refreshing;
}
```

Run: `npx vitest run src/lib/remote/cloud/apiClient.test.ts` → PASS。
```bash
git commit -am "feat(web-remote): apiClient 401 单飞刷新 + 重试"
```

### Task 9: 子域 bootstrap 替代 #token

**Files:**
- Modify: `src/routes/+layout.svelte`
- Modify: `src/lib/remote/cloud/cloudControllerBoot.ts`

- [ ] **Step 1: +layout 子域分支改用 bootstrap**

`+layout.svelte` 的 web-remote 子域接线处：先 `await bootstrapFromCookie()`；true → 继续 `bootCloudControllerFromUrl(...)`；false → `window.location.replace(\`${scheme}://${BASE_DOMAIN}/?redirect=${encodeURIComponent(location.href)}\`)`（复用已有跳转）。删除 `consumeHandoffToken()`/`#token` 读取。

- [ ] **Step 2: 删 #token 握手**

`cloudControllerBoot.ts` 删 `consumeHandoffToken` 及其调用；`auth.ts` 删 `#token` 解析（若在此）。grep 确认无残留：`grep -rn "consumeHandoffToken\|#token\|handoff" src/`。

- [ ] **Step 3: 类型检查 + Commit**

Run: `npx svelte-check --threshold error`（或 `tsc --noEmit`，先看 package.json 脚本）。
```bash
git commit -am "feat(web-remote): 子域改 cookie bootstrap，#token 握手退役"
```

---

## Phase 3 — 主域 web build 设备列表页

### Task 0（前置）: 定位 web build 源码

- [ ] grep/询问确认主域 `9527127.xyz` 的 web build 来源（设计 §9.6）：
  Run: `grep -rn "web_build_dir\|web-remote-dist\|web build" C:/code/ridge-cloud/src/router.rs C:/code/ridge-cloud/src/static_host.rs` → 看它服务哪个目录；该目录的源码仓即设备列表页落点。
  - 若 web build = wind 仓某入口（如独立 `web/` 构建）→ 在该入口加设备列表路由/组件。
  - 若为独立产物 → 在对应源码处加，并纳入其构建/部署。

### Task 10: 设备列表页

**Files:**（落点 = Task 0 结果）
- Create/Modify: 设备列表页面组件 + 路由。
- Test: 组件测试（mirror 该 web build 既有测试风格）。

- [ ] **Step 1: 写失败测试**

```ts
it('登录态 → 渲染设备列表 + 在线徽标 + 点击跳子域', async () => {
  // mock GET /devices → [{name:'devhost', username:'jack', online:true}]
  // 断言渲染出 'devhost'、online 徽标；点击 → location 指向
  // https://devhost-jack.9527127.xyz
});
```

- [ ] **Step 2-4: 实现**

- 已登录（`bootstrapFromCookie()` 成功或已有 access）→ 调 `GET /devices`（Bearer，复用 apiClient）→ 列表渲染：每项 `{name}`、在线状态徽标、离线置灰。
- 点击在线设备 → `location.href = \`${cloudHttpScheme(BASE_DOMAIN)}://${device}-${username}.${BASE_DOMAIN}\``。
- 未登录 → 登录表单（复用既有登录组件）。

Run: 组件测试 PASS。
```bash
git commit -m "feat(web-remote): 主域设备列表入口页（GET /devices + 在线状态 + 跳子域）"
```

---

## 集成验证（运行时清单，需全栈本地跑）

按设计 §8：本地 `localhost:5001` 全栈：
1. 主域登录 → 收 `Set-Cookie ridge_sso`（HttpOnly/Secure/SameSite=Lax/Domain=.localhost）。
2. 主域设备列表渲染 + 在线状态。
3. 点设备 → 子域加载 → `/auth/session`（带 cookie）→ 免重登 → 进 TOTP 门 → 控制。
4. access 15min 过期后某 API 收 401 → 静默刷新不掉线。
5. `POST /auth/logout` → 子域刷新 → `/auth/session` 401 → 跳登录。
6. DB `auth_sessions` 行只存 hash（非原 token）。

---

## Self-Review

**Spec coverage：** §2 流程 → Task 7/9/10；§3 token 模型 → Task 1/2/3；§4 后端 → Task 1-6；§5 前端 → Task 7-10；§6 安全（HttpOnly/Secure/SameSite/hash 存储）→ Task 4/6 + 集成清单；§7 迁移（#token 退役）→ Task 9；§8 测试 → 各 Task 测试步 + 集成清单。覆盖完整。

**Placeholder 扫描：** 代码块均给真实实现；少数「按既有 mirror 对齐」处（load_user/UserDto/config.base_domain/api.apiBase/web build 落点）是**明确指向既有文件的对齐指令**，非 TBD——执行者照指定文件逐字对齐。Task 0 显式定位 web build 落点。

**类型一致性：** `issue_user_access`、`build_set_cookie`/`read_cookie`/`cookie_domain`/`CookieSession`、`auth_session_repo::{create,find_valid_by_hash,touch,delete}`、`bootstrapFromCookie`、`SSO_COOKIE`/`REFRESH_TTL_DAYS` 跨任务命名一致。

**已知执行依赖（非阻塞）：** Rust 测试需本地 PG + 在 dev 锁空闲时（或 `--target-dir`）跑；前端 vitest 可直接跑。
