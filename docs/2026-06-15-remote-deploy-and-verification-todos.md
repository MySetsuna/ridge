# 公网远控修复 — 部署与验证待办（交接）

> 日期：2026-06-15。本轮修复了 diff/编辑器、web-remote 工作区、公网鉴权、文件搜索等问题。
> 代码已实现并自测（svelte-check 0 错误、vitest 807 通过、ridge-cloud 91 测试通过）。
> 本文档汇总**只有你能做**的事（部署 / 基建 / 真机·GUI 验证），以及代码与推送状态。

---

## 一、只有你能做的事（待办）

### ✅/⏳ 1. 部署 ridge-cloud（修复 Bug 4A 桌面浏览器登录态）

- 代码状态：**已推送** `origin/develop`（含 `a00fbd2` .env.example、`929e9e0` 及 CORS/TURN）。
- 待你做：**触发/确认线上部署**（看你们流水线：若 push 自动触发 CI/dokku 则等其完成；否则手动 deploy/重启服务）。
- 生效内容：`/auth/session` 跨域响应补上 `Access-Control-Allow-Credentials: true` → 租户子域桌面浏览器的 `bootstrapFromCookie` 不再被 CORS 丢弃，登录态校验恢复。
- 仅重新部署即生效，**无额外依赖**。

### ⏳ 2. 部署 coturn + 配置 env（Bug 4B 移动端 TOTP「网络错误」的生效门槛）

> 仅部署 ridge-cloud **不足以**根治移动端：还需一台 TURN relay。未配置时 `/ice-servers`
> 仍只返回 STUN（行为同今日，向后兼容），移动蜂窝弱网仍可能连不通。

**2.1 在 ridge-cloud 配置 env（与已推送的 `.env.example` 对应）：**

```env
TURN_HOST=turn.9527127.xyz            # coturn 对外主机名（须有 turns:443 的有效 TLS 证书）
TURN_STATIC_AUTH_SECRET=<openssl rand -hex 32>   # 机密，勿复用 JWT_SECRET
TURN_TTL_SECS=86400                   # 可选，默认 24h
```
生产经 `dokku config:set TURN_HOST=... TURN_STATIC_AUTH_SECRET=...` 注入。

**2.2 部署一台 coturn，`turnserver.conf` 最小配置（`static-auth-secret` 须与上面的 `TURN_STATIC_AUTH_SECRET` 完全一致）：**

```conf
listening-port=3478
tls-listening-port=443
fingerprint
use-auth-secret
static-auth-secret=<与 TURN_STATIC_AUTH_SECRET 相同的值>
realm=9527127.xyz
# turns:443 的证书（建议复用主域/租户域证书；Let's Encrypt 亦可）
cert=/etc/letsencrypt/live/turn.9527127.xyz/fullchain.pem
pkey=/etc/letsencrypt/live/turn.9527127.xyz/privkey.pem
# 公网 IP（云主机有 NAT 时填外网 IP）
# external-ip=<PUBLIC_IP>
no-cli
no-multicast-peers
# 收敛攻击面：仅放行中继端口范围
min-port=49152
max-port=65535
```

要点：**`turns:` over TCP/443** 是穿透移动蜂窝/企业防火墙的关键，证书与 `TURN_HOST` 域名须匹配。
后端按 coturn REST 方案下发凭证：`username=<过期时间戳>`，`credential=base64(HMAC-SHA1(secret, username))`，无需在 coturn 建静态用户。

### ⏳ 3.（决策）推送 wind 源码

- 现状：本地 `develop` 领先 `origin/develop` **18 个 commit**，其中**夹着并行开发的「自定义主题编辑器」WIP**（`feat(theme): …` 一系列，非本轮任务、未经我验证）。
- 因此 `git push` 会**连同主题 WIP 一起推上去**。要不要推、怎么拆（如只挑本轮修复 cherry-pick 到独立分支），由你决定——我没有擅自推 wind。
- 本轮修复对应的 wind commit 见下表，可据此筛选。

---

## 二、验证清单（需真机 / GUI，只有你能做）

> 我能做 svelte-check / vitest（已过），但下列是运行态/视觉行为，需你在应用里确认。

| 问题 | 验证动作 | 预期 |
|---|---|---|
| Bug 1 diff 模式切换 | 打开任一 diff，点「并排/行内」按钮 | 右侧 diff 即时重排，连点无 stale |
| Bug 2 光标行末对齐 | 冷启动开含长行文件，光标移到行末 | 不再视觉左偏（`document.fonts.check('16px "JetBrains Mono"')` 为 true 后看齐） |
| **diff 卡住回归修复** | 先看一个 diff，再开其它普通文件 | 展示区不卡住、正常显示新文件（**重点回归**） |
| Bug 3 web-remote 工作区 | 真实 cloud-controller / LAN web-remote 会话连接 | 工作区 tab 非空、资源管理器显示真实名（非 id）、终端铺满 |
| Bug 4A 桌面浏览器登录态 | 部署后，桌面浏览器开 `https://{device}-{user}.9527127.xyz` | Network 里 `/auth/session` 响应含 `Access-Control-Allow-Credentials: true`、200，不再卡 `remoteGateErrTenantLoginStuck` |
| Bug 4B 移动端 TOTP | **真机蜂窝**（coturn 部署后）开租户子域输 TOTP | `chrome://webrtc-internals` 看 candidate pair 落到 `relay`；TOTP 不再「网络错误」 |
| 文件搜索（搜不到） | Ctrl+Shift+P 输关键字（已打开若干工作区） | 能搜到当前会话各 pane CWD 下的文件 |
| 文件搜索（主题） | 切换主题后看 Ctrl+Shift+P 搜索框 | 配色随主题，不再硬编码暗色 |

---

## 三、代码 / 推送状态

### ridge-cloud（已推送 `origin/develop`）
| commit | 内容 |
|---|---|
| `4e43e96` | fix(cors): 补 `allow_credentials(true)`（Bug 4A） |
| `7f2429d` | feat(ice): TURN relay 时效凭证（Bug 4B；需 coturn 才生效） |
| `a00fbd2` | docs(env): `.env.example` 补 TURN 变量说明 |

### wind（本地 `develop`，**未推送**；夹有并行主题 WIP，见待办 3）
| commit | 内容 |
|---|---|
| `bd5199b` | fix(editor): diff 模式切换点击无反应（Bug 1） |
| `338ad21` | fix(editor): 编辑器/diff 光标行末左偏（Bug 2） |
| `08e5701` | fix(web-remote): 连接后工作区丢失/名字变 id/tab 空/终端不铺满（Bug 3） |
| `e9ecad6` | fix(cloud): TOTP 校验弱网首帧重发（Bug 4B 客户端侧） |
| `17ce48f` | fix(search): QuickOpen 改用 paneCwdStore（修搜不到文件） |
| `4ec889a` | style(search): QuickOpen 搜索框主题变量对齐 |
| `9d4cfe1` | fix(editor): diffEditor 回退普通 let（修 diff 后切文件卡住） |

> 另：工作树有会话开始时即存在的 `M src-tauri/Cargo.toml`（非本轮改动，未触碰）。

---

## 四、我已推进（非 user-only，已完成）

- ridge-cloud 两修复因远端并行工作冲突，已 rebase 到最新 `origin/develop` 并解决三方合并，91 测试通过后推送。
- 补充 `.env.example` 的 TURN 变量文档并推送。
- 本交接文档落地。
