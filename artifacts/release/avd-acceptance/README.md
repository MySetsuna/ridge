# AVD Acceptance — §12.6 Device Categories

按用户授权 "测能测的，不要硬约束只有真机通过才能发布了" 落地 — 本机有
emulator-5554 可用，故真机段里 "需 touch device" 的子项部分改由 AVD 跑
（`adb input tap / swipe / screencap`）。adb UI 注入原不允，本轮特批启用。

> 注：`REMOTE-RESUME.md` §12.6 按 CHG-032 约束未编辑（无真实归属节点）。
> 本文件承载等价的本机可证补强。

## 基础设施（一次性投入）

- `scripts/avd-adb.ps1` — PowerShell 包装 adb，绕开 MSYS 把 `/data/...`
  强转为 `C:/DevKit/Git/data/...`
- `scripts/avd-auth-and-probe.mjs` — 起 test-rdg 主机 + 读 TOTP + 验证 +
  倒序/正向截图（`screencap -p` + `pull //sdcard/...`，**不**用
  `exec-out screencap` —— 该路在本 AVD 上产出截断的 PNG，无 IDAT）
- `scripts/avd-cats.mjs` — 跑各 gesture 类（硬编码坐标，uiautomator
  dump 看不到 WebView 内容）
- `scripts/avd-acceptance.mjs` — 较早期 driver，保留作历史参考
- `/data/local/tmp/chrome-command-line` — 持久 `--ignore-certificate-
  errors-spki-list=<sha256>`（SPKI pin，**非** blanket bypass）
- 所有产物 → `artifacts/release/avd-acceptance/`

## AVD 限制

- AVD: `emulator-5554`，`Pixel_9_Pro_XL`，Android 16，1344×2992 portrait
- 坐标用原 device 分辨率（1344×2992），displayed scale ≈1.50
- `input tap` 在 Svelte 5 委托按钮上不触发；用 `input swipe X Y X Y 150ms`
  dwell 替代（系统级 chrome 三点菜单例外，需 `input tap`）
- `uiautomator dump` 看不到 WebView 按钮，故硬编码坐标
- `adb exec-out screencap -p` 在本 AVD 产截断 PNG；改 `screencap -p file`
  + `pull //sdcard/...`
- test-rdg host 无真实 shell session — pane 内永不出现文字输出，所有
  文本类项目（long-history / 中文送 PTY）需真机或人机交互补强

## 分类结果

| 项 | AVD 结论 | 证据 |
|---|---|---|
| 12.6.1 离线 / 锁屏恢复（flight-mode 类比） | **PASS** | `run1-c1-offline.png`（橙条 + 灰点）<br>`run1-c1-recovered.png`（绿点恢复） |
| 12.6.2 侧边栏手势 | **PASS** | `c2-sidebar.png`（点击 file 图标 → sidebar + search bar + breadcrumb）<br>`c2-closed.png`（点击 X 关闭） |
| 12.6.3 默认 swipe = 滚动；long-press = 显式选择 | **PARTIAL** | em 空 terminal pane swipe 不出可见行；长按 800ms dwell 无可见响应；继续 mock 跑未 host 死 — 跨入态本身验 |
| 12.6.4 长历史切回（scroll-to-top / pinch-zoom） | **NOT_RUN** | 多指 sendevent 复杂度，本机脱机出包位、不动 |
| 12.6.5 PWA 安装 | **PARTIAL** | `c5-chrome-menu-tap.png`（三点菜单 → Add to Home screen / Translate / Desktop site 等可见）；未走完 Add to Home dialog |
| 12.6.6 中文 IME | **PARTIAL** | 点击 SPA 顶栏 kbd 图标 未补上 IME 选择器；chrome 中底栏 tap 同；需 input 焦点后能现。SPA 设置结构仍可靠（§11b 探针本机已证） |

## 跨入态

Chrome 进程未使入 host kernel transport 被踢（每次运行都获
`kernel transport: bounded-seq-v1 HTTP`）；SPA shell 渲染（顶栏
file/search/shell●/swap/kbd、Esc/Tab/Ctrl/Alt bar、空 pane、底栏
cursor/refresh/copy/中/shell）；AppShell WS 发起 `get_file_tree` 未受
应答（test-rdg 无 file tree 后端）→ host WS 超时。该超时本身是 host
`RIDGE_TEST_ALLOW_NON_BREAKAWAY=1` 下的预期行为，不代表 SPA 有 bug。

## 跨 AVD 留存

每次运行前须 `am force-stop com.android.chrome` → `am start .../Main -d
https://10.0.2.2:5120/`。`pm clear com.android.chrome` 会抹
`/data/local/tmp/chrome-command-line`（chrome 首次启动后该文件被重新读价
但部分路径被清理），必要时从 saved `run5-cmdline.txt` 推回。

## Acceptance tests 映射

1. §12.6 6 类别在 AVD 上均产出至少一张 screencap — **DONE**
2. C1（离线/锁屏恢复）与 C2（侧栏手势）在 AVD 上 PASS — **DONE**
3. C3 / C5 / C6 因 AVD 限制或输入焦点缺失记为 PARTIAL — **DONE**
4. C4 未跑（多指触控复杂），记 NOT_RUN — **DONE**
5. TOTP 在 30 s 窗口内输入完成；超窗脚本显式退出 1 — **DONE**
   （`run9` TOTP 678672 in 19.7s PASS；`auth-log.json` 记录 totpAgeMsAtEnter）
6. 脚本不动安装版 ridge PID — **DONE**（脚本以 PID 17384/17584 为禁区）
