# 官方公网加速 · 实机验证清单

> 配套契约：`public-remote-e2e-contract.md`。云端已部署上线（ridge-cloud `a328252`），照本清单把**桌面端 + 浏览器 controller + 免费签到 + 管理端 + ridge-cli** 端到端走一遍。
> base 域 = `9527127.xyz`。测试账号 `jxk2yk@gmail.com`（已 premium + is_admin）。
> 语言约定：散文中文，命令/标识/域名保留英文原文。

---

## 0. 云端冒烟（已上线，可随时复测，不依赖桌面端）

```bash
# 登录授权端点（应回 {request_code, poll_token, authorize_url, expires_in:600, interval:2}）
curl -sS -X POST https://9527127.xyz/api/v1/auth/request \
  -H 'content-type: application/json' -d '{"client":"desktop"}'

# 管理端 SPA / 授权页（应 200）
curl -sI https://admin.9527127.xyz/ | head -1
curl -sI https://9527127.xyz/authorize | head -1
```

- [ ] 三条都正常。
- [ ] ⚠️ **通配 TLS（最可能的坑）**：浏览器开 `https://test-tester.9527127.xyz/`（语法合法的假租户名，路由不校验设备是否真实存在，会返回 controller SPA）。若**证书不匹配/告警** → 租户子域控制端打不开，需为 `*.9527127.xyz` 签**泛域证书**（Dokku：`dokku letsencrypt` 对 wildcard 需 DNS-01）。

---

## 1. 构建桌面端

```bash
npm run tauri build      # 或重启 dev：npm run tauri dev
```

- 本机 rebuild **不影响当前会话**。
- 含本批改动：`ridge://` scheme、`loginViaBrowser`、云端 TOTP、最小化按钮（原深根）、打开公网远控按钮。

---

## 2. 桌面端「官方公网加速」tab + 浏览器授权登录

- [ ] 远控面板有「局域网 / 官方公网加速」两个 tab；两 tab 都能看到「**最小化·后台保活**」按钮（文案已去"深根"）。
- [ ] 切「官方公网加速」。未登录 → 显示登录入口；点「**登录（浏览器授权）**」→ 默认浏览器打开 `…/authorize?code=…&client=desktop`。
- [ ] 浏览器侧：未登录 ridge-cloud → 先登录（登录后自动回到 authorize 页）；注册新账号 → 看到「**用户名将作为 remote 子域前缀**」「**升级才可任意公网远控**」提示 + 按语言显示**爱发电(zh)** / 海外订阅(en)。
- [ ] authorize 页点「**批准**」→ 浏览器尝试 `ridge://auth/focus` 把桌面端拉回前台；桌面端轮询拿到 user JWT，面板转为**已登录态**。

---

## 3. 设备激活 + 连接 + 打开子域

- [ ] 已登录后出现**设备名输入框** + 激活按钮。输入设备名（3-30 位小写字母/数字/连字符）→ 激活（device-code 流，需 premium + 已设用户名）。
- [ ] 专属子域卡片显示 `{device}-{username}.9527127.xyz` + 「**在浏览器打开公网远控**」按钮。
- [ ] 点「连接」→ 状态 `connecting → handshaking → connected`；连上后卡片出现 **TOTP 6 位 code + 绑定 QR**（复用 LAN 布局）。

---

## 4. 浏览器 controller 端（真正控制）

- [ ] 点「在浏览器打开公网远控」（或手动开子域）→ 浏览器加载 controller SPA。
- [ ] 连上后弹 **TOTP 输入**；输入桌面端卡片显示的 6 位 code → 通过后才放行控制；输错 → 拒绝 + 可重试。
- [ ] 验证可控：终端输入回显、resize、文件树展开、搜索命中。
- [ ] 桌面端点「最小化·后台保活」→ 窗口隐藏到托盘，浏览器侧远控**不断**；从托盘恢复正常。

---

## 5. 免费每日签到（free 用户路径）

- [ ] 另注册/用一个 **free** 账号登录，升级处显示「**每日签到 · 免费 2 小时公网远控**」。
- [ ] 点签到 → 提示「已授予至 MM-DD HH:mm」；再点 → 「今日已签到」。
- [ ] 签到后该 free 账号 2h 内可公网远控；**到期后** WS 门控按 DB 有效期实时拒绝（不是等 JWT 过期）。

---

## 6. 管理端（admin.9527127.xyz）

- [ ] 开 `https://admin.9527127.xyz/`，用 `jxk2yk@gmail.com` 登录（已 is_admin）。非管理员账号 → 「无管理员权限」，不建立会话。
- [ ] **Users**：搜索；看 plan / premiumActive / 到期 / 设备数；对某用户 **grant**（月/年/买断）/ **revoke**。
- [ ] **Sessions**：当前在线 host（device/username/online/lastSeen），~10s 自动刷新。（精确"连接时长"待 rooms 暴露 join 时刻，当前展示 lastSeen。）
- [ ] **Tiers**：改某档**价格/时长/上下架** → 不改代码即生效，下次发卡/兑换按新值。
- [ ] **Keys**：选档位 + 数量发卡 → 显示卡密可复制。拿一张到普通账号面板 `/activate` 兑换 → 按档位授时长（月/年=到期，买断=永久）。

---

## 7. ridge-cli host（可选，VPS 无头）

- [ ] VPS 上 rebuild + 部署**新** `ridge-cli`（新 mux+JSON-RPC 协议；旧协议 daemon 与新 controller 不兼容）。
- [ ] `ridge-cli remote --enable`（配对，浏览器 `/activate` 输码）→ `ridge-cli remote --daemon --root /srv/<项目>`。
- [ ] **⚠️ 云端握手端到端（FIX-1c，必须实跑——静态/单测覆盖不到真实 WebRTC）**：浏览器 controller 打开 cli 设备子域后，**WebRTC offer→answer→ICE→DataChannel 打开→E2EE 握手能完整跑通、controller 真正连上无头 host**。这条曾被两个 blocker 卡死：①**cid 缺失**（FIX-1，relay 丢弃无 cid 的 answer）②**握手时序死锁**（FIX-1c，旧 `session.rs::run()` 在主循环前阻塞 `await` 对端握手帧，而驱动 DataChannel 打开的 offer 只在循环内转发 → 握手永远超时）。两者已修；**这是浏览器能连上无头 host 的必要条件**，务必在真链路确认握手不再 15s/30s 超时空转。
- [ ] **e2ee-pubkey 旁路（FIX-2）**：cli 与浏览器 controller 间 B3 防 relay-MITM 旁路**不再静默退化为 relay-trust**——cli daemon 日志应出现 `E2EE 公钥绑定判定 mode=Enforced`（双方都发 e2ee-pubkey 时）；旧 controller 则 `mode=RelayTrust`（3s 宽限回落，不回归）。
- [ ] 同一**浏览器 controller** 打开该 cli 设备子域 → 输 cli **TUI 打印的 TOTP** → 控制其终端 + 文件搜索/树；git/workspace/theme/IDE 面板因 `$/hello` 能力协商**置灰**（cli 只 advertise `pane/fs/search`）。

---

## 8. 回归 + 排查要点

- [ ] **回归**：LAN 局域网远控、最小化按钮（双 tab）不受影响。
- 排查：
  1. 子域打不开 → 先查**通配 DNS**（`*.9527127.xyz` 是否解析到 Oracle `150.230.171.175`，DuckDNS 默认跟随）+ **通配 TLS 证书**（见 §0）。
  2. 登录后仍显示 free / 非 premium → **重新登录**刷新 JWT（plan 在签发时定）。
  3. 到期仍可用 → 确认线上是 `a328252`（WS 门控读 DB 有效期）：`ssh oracle "dokku git:report ridge-cloud | grep sha"`。
  4. 授权页批准后桌面端没回前台 → `ridge://` 唤起失败也无妨，桌面端**轮询**仍会拿到 token（deep-link 仅加速回前台）。
