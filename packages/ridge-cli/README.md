# ridge-cli

Ridge 的**无头远控 host**——把一台没有图形界面的 Linux / VPS 变成可被 Ridge 移动端
/ 浏览器安全控制的远程终端。E2EE（X25519 + ChaCha20-Poly1305）在 WebRTC DataChannel
之上再叠一层，信令复读机（relay）与 TURN 都看不到明文。

实现严格遵循 `docs/contracts/ridge-cloud-protocol.md`（SSOT）。

## 它做什么

- **设备码配对**（契约 §4.4）：无需在服务器上输密码，拿一个一次性配对码到浏览器激活。
- **端到端加密**（契约 §7）：X25519 握手 + HKDF-SHA256 派生 + ChaCha20-Poly1305，
  nonce 方向分离、counter 严格递增防重放。与浏览器侧 `@noble/*` 字节级一致。
- **PTY 桥**：拉起本地 shell（bash/zsh/…），把输出经 **16ms 攒批**合并后加密发往控制端。
- **远端文件能力**：ripgrep 级文本搜索 + 目录树（控制端按需请求）。
- **信令 WebSocket**：作为 host(answerer) 连 `wss://{device}-{username}.{base}/ws?token=&role=host`，
  处理 controller 的 offer、回 answer、交换 ICE。

## 安装与构建

```bash
# 在仓库内构建（standalone crate，不依赖 Tauri/webview）
cd packages/ridge-cli
cargo build --release
# 产物: target/release/rdg
```

> 默认启用 `rtc` 特性（真实 WebRTC）。受限 CI 上若 `webrtc` 依赖树不可用，可用
> `cargo build --no-default-features` 得到一个除 RTCPeerConnection 外全部可用的构建
> （设备码流 / E2EE / PTY / 攒批 / 信令均真实实现）。

## 用法

### 1. 配对（一次性）

```bash
rdg remote --enable
```

控制台会打印一个配对码，并引导你在已登录的浏览器打开 `https://{base}/activate` 输入。
绑定成功后，device JWT 持久化到 `~/.config/ridge/auth.json`（Linux 下权限 0600）。

### 2. 守护运行

```bash
rdg remote --daemon
# 可选：指定 shell / 工作目录
rdg remote --daemon --shell /bin/zsh --cwd /srv/app
```

也可一步到位：`rdg remote --enable --daemon`（配对成功后直接进入守护）。

### 环境变量

- `RUST_LOG`：日志级别，默认 `info`（如 `RUST_LOG=debug,ridge_cli=debug`）。
- `RIDGE_BASE_DOMAIN`：覆盖默认 Base zone `remo2ridge.duckdns.org`（自托管 / 测试用）。

## systemd 安装

见 `ridge-cli.service` 顶部注释。系统级摘要：

```bash
sudo cp target/release/rdg /usr/local/bin/rdg
sudo useradd --system --create-home --home-dir /var/lib/ridge --shell /usr/sbin/nologin ridge
sudo -u ridge -H /usr/local/bin/rdg remote --enable     # 完成一次配对
sudo cp ridge-cli.service /etc/systemd/system/ridge-cli.service
sudo systemctl daemon-reload
sudo systemctl enable --now ridge-cli
journalctl -u ridge-cli -f
```

服务以低优先级（`Nice=10`、`CPUWeight=20`）后台运行，崩溃自动退避重启，并启用了
`NoNewPrivileges` / `ProtectSystem=strict` 等沙箱加固。

## 模块一览

| 模块 | 职责 |
|---|---|
| `device_flow` | 设备码流（§4.4）+ 极客风配对码打印 |
| `config` | 凭据持久化（`auth.json`）、域名 / URL 拼接（§1/§3） |
| `e2ee` | X25519 + HKDF + ChaCha20-Poly1305（§7），含往返/方向/重放单测 |
| `batching` | 16ms 攒批缓冲，含合并单测 |
| `signaling` | 信令 WS 客户端（§5） |
| `rtc` | host answerer（`webrtc` crate；`--no-default-features` 时为 stub） |
| `pty` | `portable-pty` 拉起本地 shell（解耦 Tauri AppState） |
| `fs_reuse` | ripgrep 级搜索 + 目录树（与桌面端同引擎 `ignore`/`glob`/`regex`） |
| `session` | 把信令 → WebRTC → E2EE → PTY 串成远控通路 |
| `daemon` | 守护主循环 + 断线重连 |

## 测试

```bash
cargo test --lib        # 纯逻辑单测（E2EE、攒批、协议、信令解析、fs）
cargo fmt
```
