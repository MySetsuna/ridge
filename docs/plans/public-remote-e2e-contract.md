# 官方公网加速 · 端到端契约 (Public-Remote E2E Contract) v1

> 跨三处协作的**单一真源**：`wind` 桌面端 + `wind/packages/ridge-cli` + `C:\code\ridge-cloud`（后端 src/ + 前端 web/）。
> 所有并行 agent 必须严格按本契约的**接口形状**编码，不得自行改名/改字段。改契约须先改本文件。
> base 域：`9527127.xyz`（可被 `RIDGE_BASE_DOMAIN` 覆盖）。API 前缀：`/api/v1`。

## 0. 目标流程（用户原话归纳）

桌面端「官方公网加速」tab：
1. 未登录 → 显示**登录**按钮 → 打开**默认浏览器**到 ridge-cloud `/authorize`（类似 Claude Code 登录）。
2. 浏览器侧：未登录先登录；注册时提示「用户名将作为 remote 子域前缀」「升级才可任意公网远控」，并按注册语言展示订阅/卡密入口。
3. 批准后：**桌面端**由网页 `ridge://` 自动唤起回前台；**ridge-cli** 复制粘贴 code 到 TUI。两者都通过**轮询**拿到 user JWT（token 不进 URL）。
4. free/未登录：提示需 premium 才能任意公网远控；free 用户**每日签到得 2h** 免费远控；按语言展示**爱发电**(zh)/海外订阅(en)，已升级则不展示。
5. 登录后：出现**设备名输入框** + **公网 remote 按钮** → 走设备激活 → 重定向到该设备专属子域 `{device}-{username}.{base}`。
6. 此时桌面端与 ridge-cli 都展示 **TOTP code**（桌面端**复用局域网页面布局与代码**）；打开子域的浏览器**必须输入 TOTP** 才能真正控制。
7. 管理端（`admin.{base}`）监控用户/子域/设备的登录状态、连接时长等。

## 1. 自定义 scheme `ridge://`（仅"唤起回前台"，**不携带 token**）

- 桌面端注册 `ridge://`（tauri deep-link plugin + single-instance；`on_open_url`）。
- 网页授权批准后，若 `client=desktop`：`window.location = 'ridge://auth/focus'` 把桌面端拉回前台并立即触发一次轮询。
- URI 仅作信号，**绝不放 JWT/敏感数据**（token 一律走轮询接口）。CLI 不用 scheme，提示用户切回 TUI。

## 2. 登录授权（浏览器登录 → host 轮询拿 user JWT）

复用 device-code 形状，但产出 **user token**（不绑设备）。新表 `auth_requests`。

### 2.1 后端端点（ridge-cloud src/api）
- `POST /api/v1/auth/request`（public）
  - body: `{ "client": "desktop" | "cli" }`
  - 200: `{ "request_code": "<8位大写>", "poll_token": "<32位>", "authorize_url": "https://{base}/authorize?code=<request_code>&client=<client>", "expires_in": 600, "interval": 2 }`
  - 落 `auth_requests(request_code PK, poll_token UNIQUE, client, status='pending', user_id NULL, expires_at)`。
- `POST /api/v1/auth/approve`（Bearer **user** JWT）
  - body: `{ "request_code": "<...>" }`
  - 校验 request 存在 + pending + 未过期 → `UPDATE status='approved', user_id=<claims.sub>`。
  - 200: `{ "ok": true }`；错误码 `AUTH_REQUEST_NOT_FOUND` / `AUTH_REQUEST_EXPIRED`。
- `POST /api/v1/auth/poll`（public）
  - body: `{ "poll_token": "<...>" }`
  - pending: `{ "status": "pending" }`；expired: `{ "status": "expired" }`
  - approved: `{ "status": "approved", "token": "<userJWT>", "user": <UserDto> }`（签发后**作废该 request**，一次性）。

### 2.2 前端页面（ridge-cloud web/src/routes/authorize/+page.svelte）
- 读 `?code=&client=`；未登录 → 引导登录（复用 `/login`，登录后回到本页带 code）。
- 已登录 → 展示「授权 Ridge {桌面端|CLI} 登录」+ 账号信息 + **批准**按钮 → `POST /auth/approve {request_code}`。
- 批准成功：`client=desktop` → `window.location='ridge://auth/focus'` 并显示「已授权，请返回 Ridge」；`client=cli` → 显示「已授权，请返回终端」。

### 2.3 host 侧（desktop + cli）
- 调 `POST /auth/request` → 用 opener 打开 `authorize_url` → 每 `interval`s 轮询 `POST /auth/poll {poll_token}`，超时 `expires_in`。
- approved → 存 user JWT（desktop: cloudAuth；cli: 仅在需要时，cli 一般只需 device 配对）。

## 3. 设备激活 → 子域（沿用现有 device-code，**不改契约**）

现有 `POST /device/code` → `/device/activate`（Bearer user，需 premium + username）→ `/device/poll` 已可用。
桌面端「公网 remote 按钮」：拿到 user JWT 后，`activateThisDevice(deviceName)`（已存在）→ 得 `public_entry` → opener 打开该子域。

## 4. 云端 TOTP 二次验证（E2EE 数据通道内，host 校验）

云远控连上（WebRTC+E2EE）后仍需 TOTP，**复用 host 端 LAN 的 RemoteAuth(RFC6238)**。
- host（desktop `CloudHostBridge` / cli）在 E2EE 通道上对**控制类帧**门控，未验证前拒绝 invoke/pane。
- 控制帧（JSON over E2EE，区别于 mux 业务帧）：
  - controller → host: `{ "t": "totp-verify", "code": "123456" }`
  - host → controller: `{ "t": "totp-result", "ok": true|false }`
- host 用本机 `RemoteAuth::verify(code)`（±1 窗口）判定；ok 后该连接放行。
- 桌面端 TOTP 展示**复用 LAN 布局/`get_remote_info`**；cli 在 TUI 打印 6 位 code + otpauth。
- 控制端子域页面：连上后若收到 host 的「需 TOTP」状态 → 弹 TOTP 输入框（复用 LAN 控制端输入组件）。

## 5. 每日签到（free 用户每日 2h 免费远控）

- `POST /api/v1/me/checkin`（Bearer user）
  - 若今日已签到（按 user 末次签到 UTC 日期判重）→ `{ "ok": false, "reason": "already", "premiumExpiresAt": <ts|null> }`。
  - 否则授予 2h 临时 premium 窗口（**与计费叠加但不缩短永久/已购**）：
    - 若 `plan='premium'` 且 `premium_expires_at IS NULL`（买断/订阅）→ 不动，返回 already-permanent。
    - 否则 `plan='premium', premium_expires_at = GREATEST(now(), COALESCE(premium_expires_at, now())) + 2h`，并记 `last_checkin_date`。
  - 200: `{ "ok": true, "premiumExpiresAt": <ts> }`。
- 新列 `users.last_checkin_date DATE`。判定有效 premium 复用 `is_premium_now()`（已含到期）。

## 6. 管理端 API（`admin.{base}`，is_admin 门控）—— Wave 2

- 静态：`static_host` 对 Host 首段 `admin` 兜底返回 `admin-app/`（独立 SPA，仿 `desktop_app_dir`）。
- 鉴权：Bearer user JWT 且 DB `is_admin=true`（新增 admin 提取器/中间件）。
- 端点（前缀 `/api/v1/admin`）：
  - `GET /admin/users?query=` → 用户列表（plan、premiumExpiresAt、premiumActive、设备数、是否在线）。
  - `GET /admin/sessions` → 当前在场会话（device、username、role、since、duration 秒）——来自 room registry + 接入时刻。
  - `POST /admin/users/:id/grant {tier|days|lifetime}` / `POST /admin/users/:id/revoke`。
  - `GET /admin/tiers` / `PUT /admin/tiers/:id`（改价改时长，复用 plan_tiers）。
  - `POST /admin/keys {tier, count}` → 在线发卡（复用 genkeys 逻辑）。

## 7. i18n / 支付链接

- zh（billingRegion=cn）：爱发电 `https://ifdian.net/a/ridge` + 卡密输入。
- en（intl）：海外订阅（Lemon Squeezy，真实链接待运营填）。
- 已 premium（`premiumActive`）→ 不展示任何升级入口。
- 替换桌面端 `CloudProModal` 与 web `PremiumGate`/`constants.ts` 里的 `mbd.pub/PLACEHOLDER` 为爱发电真实链接。

## 8. 「深根」改名 → 最小化省资源（双 tab）

- 文案（zh）：按钮「最小化·后台保活」；副文案「最小化窗口释放界面资源，远控会话在后台保持连接，可随时从托盘恢复」。
- 文案（en）：button "Minimize · keep alive"；hint "Frees UI resources; the remote session stays connected in the background — restore anytime from the tray."
- 行为不变（`enter_deep_root_mode` 仍 hide+通知+托盘恢复），**仅改 i18n 文案 key 与展示位置**：LAN tab 与 Cloud tab **都**显示该按钮（命令内部名可保留，UI 文案去「深根」）。
- 注意：当前 `enter_deep_root_mode` 有 `cloud_remote_active` 前置校验；LAN tab 复用时，LAN 活跃也应视为"有活跃远控"（W2 放宽前置或加 lan_remote_active 旗标）。

## 9. 波次（Wave）划分

- **Wave 1（本轮并行）**：§2 登录授权全链路（后端端点+表 / web `/authorize` 页 / 桌面端 ridge://+登录按钮+轮询）；§8 深根改名双 tab；§7 爱发电链接 + 注册页"用户名=子域前缀"文案。
- **Wave 2**：§4 云端 TOTP；§5 每日签到；§6 管理端（API+admin SPA+静态路由）；公网 remote 按钮跳子域打磨。

## 10. 协作约束

- ridge-cloud 有**另一并行改动**（设备在线态：`ws/`、`device_repo`、`dto.DeviceDto`、`rooms`、迁移 0004）。本契约的 agent **不得改这些文件**，只新增文件或在 `auth_routes.rs`/新迁移/web 新路由内追加，避免冲突。
- 迁移编号：本契约新增用 `0005_auth_requests.sql`（登录授权）、`0006_daily_checkin.sql`（签到列）——避开 0004。
- 改动后各自 `cargo check`（ridge-cloud）/`npm run check`（web、wind）自验，回报 file:line 与验证结果。
- 迁移编号：登录授权 `0005`（已建）；每日签到 `0006`；管理端 `0007`。避开并行 actor 的 `0004`。

## 11. 已知对齐缺口（实现中发现）

**11.1 cli-host 与 desktop-host 的 controller 线协议不一致（S3 收敛未做）**
- **desktop host**（`cloudHostBridge.ts`）：controller↔host 走 **cloudMux**（`PANE_RAW 0x10` / `JSON-RPC 0x11` / `CONTROL 0x12`）+ JSON-RPC（invoke/pane）。TOTP 在 `0x12`，**两端已对齐打通**。
- **cli host**（`packages/ridge-cli/src/protocol.rs`）：controller→host 是**裸 `ControlMsg` JSON（无通道字节）**；host→controller 用 `0x10 PTY_OUTPUT` / `0x11 JSON`。TOTP 用 `ControlMsg::TotpVerify` + `HostMsg::TotpResult(0x11)`，**host 侧已实现**，但 **wind 浏览器 controller（`cloudControllerBoot` 是 JSON-RPC）并不讲这套协议** → 当前没有浏览器 controller 能驱动 cli host（cli 面向 terminal 控制端 / wasm-vte，走裸字节）。
- **影响**：桌面端公网远控（含 TOTP）端到端可用；**cli host 的公网 controller 缺一个对端**（要么把 cli 收敛到 mux+JSON-RPC = 统一远控 S3，要么补一个讲 cli 协议的 terminal 云 controller）。这是真正的"host 端没对齐"根因之一。
- **决定（待定/推荐）**：把 cli host 收敛到 `0x10/0x11/0x12` mux + 与桌面同款 controller 协议（S3），一个浏览器 controller 同时驱动桌面端与 cli host，TOTP 统一在 `0x12`。本项列入 Wave 4。
