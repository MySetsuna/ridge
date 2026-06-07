# 在 VPS 上跑 agent（无头 tmux 引擎）快速上手

> 语言约定：散文简体中文，所有命令/标志/字段/代码块保持英文原文（与代码库一致）。
> 本文基于 `packages/ridge-cli/`（`tmux` 子命令）、`packages/ridge-tmux/`（native 引擎）
> 与 `src-tauri/src/bin/tmux.rs`（`tmux` shim）实证撰写——只记录代码里实际存在的行为。

## 这是干什么的

把一台**无图形界面**的机器（VPS / 云主机）变成 agent 的工作台：让运行在这台机器上的
agent（Claude Code 等）通过 PATH 上的 `tmux` shim，创建 / `send-keys` / `capture` / `kill`
**无头 tmux 会话**——不需要桌面 app、不需要 controller、不需要 Tauri / WebView / GPU。

引擎（`rdg tmux`）背后是与桌面端**逐字节同源**的 `ridge-tmux` native 路由
（`ridge_tmux::http`，桌面 teammate server 与本子命令挂的是同一个 router）。

```
agent 进程 (Claude Code)
  └── 调 `tmux …`（PATH 里是 Ridge 的 tmux shim，src-tauri/src/bin/tmux.rs）
        └── HTTP /api/v1/tmux/*  （读 RIDGE_TEAMMATE_URL / RIDGE_TEAMMATE_TOKEN）
              └── rdg tmux  （监听 RIDGE_TMUX_BIND:RIDGE_TMUX_PORT）
                    └── ridge_tmux::http  ← 与桌面端同一个 native 引擎
```

与 `rdg remote`（WebRTC 远控，连出信令、等浏览器接入）是**两条独立的路**：本文只讲
`tmux` 引擎。两者可同机共存（各自一个 systemd 单元）。

---

## 步骤一：构建 ridge-cli

`tmux` 子命令**不依赖 webrtc**（只 `remote --daemon` 用）。所以 VPS 上可以用
`--no-default-features` 构建一个**精简的、不含 webrtc 重依赖树**的二进制：

```bash
# 在仓库根。agent-only 机器推荐去掉 rtc 特性（remote 子命令会变 stub，但 tmux 不受影响）。
cargo build --release -p ridge-cli --no-default-features

# 若同机还要用 remote 远控，则保留默认特性：
# cargo build --release -p ridge-cli

sudo cp target/release/rdg /usr/local/bin/rdg
```

> 没有 Rust 工具链的 VPS：在一台同架构的开发机构建后 `scp` 过去即可——`rdg` 是
> 静态性较强的单文件（reqwest 用 rustls，不依赖系统 OpenSSL）。

---

## 步骤二：放好 `tmux` shim

agent 调用的 `tmux` 必须是 Ridge 的 shim，而不是系统自带的 tmux。shim 源码在
`src-tauri/src/bin/tmux.rs`，构建：

```bash
# 在开发机（shim 属于 src-tauri）
pnpm build:teammate-shim
# 产物：构建出的 `tmux`（Windows 上 `tmux.exe`），scp 到 VPS
```

把 shim 放进一个**排在系统 tmux 之前**的目录（agent 进程的 PATH 里靠前），例如
`/usr/local/libexec/ridge/tmux`，再让 agent 的 PATH 指向该目录；或直接给 agent 配置里把
tmux 可执行路径指到这个 shim。验证 shim 生效：

```bash
tmux -V        # 走 shim 时会按 teammate 协议应答，而非系统 tmux 的版本串
```

> shim 自己的排障日志：设 `Ridge_TMUX_LOG=/path/to/dir`（可给目录或文件），默认落
> `$TMPDIR/tmux-shim.log`。

---

## 步骤三：用 systemd 常驻引擎

仓库 `packages/ridge-cli/` 下提供了现成的单元与配置示例：

- `ridge-tmux.service` —— 跑 `rdg tmux` 的服务单元
- `ridge-tmux.env.example` —— `--bind` / `--port` / `--token` 的环境配置示例

`--bind` / `--port` / `--token` 经 clap 的 env fallback 读取 `RIDGE_TMUX_*`
（优先级：**命令行参数 > 环境变量 > 内置默认**）。把 token 放进
`EnvironmentFile`（权限 0600）而不是命令行，能避免 token 出现在 `ps` / journald 里。

```bash
# 1) 专用账户 + 配置目录
sudo useradd --system --create-home --home-dir /var/lib/ridge --shell /bin/bash ridge
sudo install -d -m 0750 -o ridge -g ridge /etc/ridge

# 2) 落配置并设置强 token（必做）
sudo install -m 0600 -o ridge -g ridge \
    packages/ridge-cli/ridge-tmux.env.example /etc/ridge/tmux.env
# 把 RIDGE_TMUX_TOKEN 填成 `openssl rand -hex 32` 的输出：
sudoedit /etc/ridge/tmux.env

# 3) 装单元并启动
sudo cp packages/ridge-cli/ridge-tmux.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ridge-tmux

# 4) 看日志：启动行里有给 agent 抄的 RIDGE_TEAMMATE_URL / _TOKEN
journalctl -u ridge-tmux -f
```

`ridge-tmux.env.example` 里的默认值：`RIDGE_TMUX_BIND=127.0.0.1`、`RIDGE_TMUX_PORT=47615`、
`RIDGE_TMUX_TOKEN=`（空 = 每次随机生成并打印；固定下来才方便常驻）。

> 用户级（无 root）安装：把 env 文件放 `~/.config/ridge/tmux.env`、改单元里的
> `EnvironmentFile=` 指向它、删掉 `User=`/`Group=` 两行，用 `systemctl --user` 启用，
> 再 `loginctl enable-linger "$USER"` 让它登出后存活。详见 `ridge-tmux.service` 头部注释。

---

## 步骤四：把引擎坐标注入 agent

shim 经 `RIDGE_TEAMMATE_URL` / `RIDGE_TEAMMATE_TOKEN` 找引擎。固定了端口和 token 后，
直接在 agent 进程的环境里导出（值 = 引擎的 bind/port 与 `RIDGE_TMUX_TOKEN`）：

```bash
export RIDGE_TEAMMATE_URL=http://127.0.0.1:47615
export RIDGE_TEAMMATE_TOKEN=<与 /etc/ridge/tmux.env 里 RIDGE_TMUX_TOKEN 相同>
```

没固定 token（留空让引擎随机生成）时，从 `journalctl -u ridge-tmux` 的启动行里抄那两个
`export …`。

验证端到端：

```bash
tmux new-session -L work -d 'echo hello; sleep 600'
tmux capture-pane -t work -p          # 应能看到 hello
tmux kill-session -t work
```

---

## 鉴权与安全边界

- **token 必带**：每个 `/api/v1/tmux/*` 请求须带 `x-ridge-token: <TOKEN>` 或
  `Authorization: Bearer <TOKEN>`，否则 401。shim 会自动带上 `RIDGE_TEAMMATE_TOKEN`。
- **默认只听回环**：`RIDGE_TMUX_BIND=127.0.0.1`。agent 与引擎同机时**保持默认**。
- **无路径沙箱**：引擎对会话内子进程不做路径限制——拉起的 shell 拥有运行 `rdg`
  的用户的全部权限。这正是 agent 能干活的前提，但也意味着：
  - **不要** `--bind 0.0.0.0` 暴露到公网，除非你已用 firewall / 反向代理收口，且设了强 token。
  - 给引擎用一个**专用低权账户**（上面的 `ridge`），别用 root。
- `ridge-tmux.service` 已做适度加固（`NoNewPrivileges`、`PrivateTmp`、`ProtectSystem=full`、
  只读 `/usr` `/etc`），但刻意比 `ridge-cli.service`（remote 守护）宽松——因为 agent 要真
  编译 / 读写仓库 / 联网装包。需要更紧或更松按注释自行调。

---

## 与桌面端的差异

headless host 没有可见工作区，故 GUI 专属的 `summon`（把会话领养进可见分屏）不暴露；其余
native 路由（`new-session` / `has-session` / `resolve` / `list-sessions` / `list-panes` /
`capture-pane` / `list-windows` / `display-message` / `split-window` / `send-keys` /
`select` / `kill` / `list-all-sessions`）行为与桌面端一致。完整命令参考见
[`ridge-cli-usage.md`](./ridge-cli-usage.md) 的 `rdg tmux` 一节。

---

## 故障排查

| 现象 | 排查 |
|------|------|
| agent 的 `tmux` 走了系统 tmux | shim 没排在 PATH 前面；`command -v tmux` 确认指向 shim |
| 所有请求 401 | `RIDGE_TEAMMATE_TOKEN` 与引擎 `RIDGE_TMUX_TOKEN` 不一致；或引擎随机生成了新 token（看 journald）|
| 连不上引擎 | `RIDGE_TEAMMATE_URL` 端口与 `RIDGE_TMUX_PORT` 不符；或引擎用了默认 0（随机端口），改成固定端口 |
| 引擎起不来 | `journalctl -u ridge-tmux -e`；端口被占 → 换 `RIDGE_TMUX_PORT` |
| shim 行为可疑 | 设 `Ridge_TMUX_LOG=/var/log/ridge` 看 shim 日志 |
| 想跨机调试但又怕暴露 | 别动 `RIDGE_TMUX_BIND`，改用 SSH 端口转发：`ssh -L 47615:127.0.0.1:47615 vps` |
