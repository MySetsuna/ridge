# Dev 全链路 TLS（wss）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 dev 默认全链路 TLS——cloud 起 https + wss、客户端对回环 cloud 也走 https/wss、用 mkcert 本地 CA 签 `*.localhost` 通配证书让 WebView2 信任，打通 dev 登录授权与远控信令子域。

**Architecture:** 证书用 mkcert（一次性 `mkcert -install` 装本地受信 CA）。cloud `tls.rs` 优先读 env 指定的 BYO 证书（`RIDGE_CLOUD_TLS_CERT/KEY`），缺失则回退现有 rcgen 自签。客户端 `cloudHttpScheme/cloudWsScheme` 默认对回环也返回 TLS scheme，留 `RIDGE_CLOUD_DEV_PLAINTEXT` 逃生开关。vite webview 维持 http（无 mixed-content 问题，localhost 属 secure context）。

**Tech Stack:** wind（SvelteKit + vitest + vite define）、ridge-cloud（Rust + Axum + rcgen + rustls）、mkcert、Node ESM 脚本。

**设计依据：** `docs/superpowers/specs/2026-06-10-dev-wss-tls-design.md`

**仓库与前置状态：**
- 改动横跨两仓库：`wind`（`C:\code\wind`）与 `ridge-cloud`（`C:\code\ridge-cloud`）。各 commit 在对应仓库执行（用 `git -C <repo>`）。
- ridge-cloud 工作区现状（本次会话已改、未提交）：`src/router.rs` 已加 CORS 回环 http 源放行分支 + 2 测试（全绿）；`scripts/dev.sh` 上一轮改成「默认 http」——Task 0 先提交 CORS，Task 5 把 dev.sh 回调为「默认 https/wss」。
- 所有 commit message 结尾追加 trailer：`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。

---

### Task 0: 提交已落地的 CORS 修复（ridge-cloud）

**Files:**
- Commit only: `C:\code\ridge-cloud\src\router.rs`（CORS 回环源放行分支 + helper + 2 测试，已实现并通过）

- [ ] **Step 1: 复核测试仍绿**

Run: `cargo test --manifest-path C:\code\ridge-cloud\Cargo.toml --lib cors_`
Expected: `6 passed; 0 failed`

- [ ] **Step 2: 仅提交 router.rs**

```powershell
git -C C:\code\ridge-cloud add src/router.rs
git -C C:\code\ridge-cloud status --short
# 确认 staged 只有 src/router.rs（dev.sh 不要带上，留给 Task 5）
git -C C:\code\ridge-cloud commit -m @'
fix(dev): CORS 放行 dev 本地回环 http 源

dev webview 页面源是 http://127.0.0.1:5173，对本地 cloud 发请求时被
生产 https-only 白名单拒掉。新增严格双重 gate 分支：仅当 cloud base 与
origin 均为回环时放行 http 源，生产 H-5 防降级不受影响。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 1: 客户端 scheme 默认走 TLS（wind）

**Files:**
- Modify: `C:\code\wind\src\lib\remote\cloud\apiClient.ts:51-59`
- Test: `C:\code\wind\src\lib\remote\cloud\apiClient.test.ts:40-52`

- [ ] **Step 1: 改测试为「默认 TLS + 逃生明文」（RED）**

把 `apiClient.test.ts` 第 40-52 行的整个 `describe('cloudHttpScheme / cloudWsScheme', ...)` 块替换为：

```ts
describe('cloudHttpScheme / cloudWsScheme', () => {
  it('returns TLS schemes for loopback bases by default (dev TLS)', () => {
    expect(cloudHttpScheme('localhost:5050')).toBe('https');
    expect(cloudWsScheme('localhost:5050')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.localhost:5050')).toBe('wss');
  });

  it('returns TLS schemes for public bases', () => {
    expect(cloudHttpScheme('9527127.xyz')).toBe('https');
    expect(cloudWsScheme('9527127.xyz')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.9527127.xyz')).toBe('wss');
  });

  it('downgrades loopback bases to plaintext when plaintext flag set (escape hatch)', () => {
    expect(cloudHttpScheme('localhost:5050', true)).toBe('http');
    expect(cloudWsScheme('localhost:5050', true)).toBe('ws');
  });

  it('keeps public bases on TLS even with plaintext flag (never downgrade prod)', () => {
    expect(cloudHttpScheme('9527127.xyz', true)).toBe('https');
    expect(cloudWsScheme('9527127.xyz', true)).toBe('wss');
  });
});
```

- [ ] **Step 2: 跑测试看失败（RED）**

Run: `pnpm vitest run src/lib/remote/cloud/apiClient.test.ts`
Expected: FAIL —「returns TLS schemes for loopback bases by default」断言 `cloudWsScheme('localhost:5050')` 现返回 `'ws'`，期望 `'wss'`。

- [ ] **Step 3: 改实现（GREEN）**

把 `apiClient.ts` 第 51-59 行（两个 scheme 函数）替换为：

```ts
/** 构建期逃生开关：dev 默认全链路 TLS；置 RIDGE_CLOUD_DEV_PLAINTEXT=1 时回环 cloud
 *  回退明文 http/ws（mkcert 故障时临时调试用）。经 vite define 注入（见 vite.config.js）。 */
const DEV_PLAINTEXT = (import.meta.env.RIDGE_CLOUD_DEV_PLAINTEXT as string | undefined) === '1';

/** 某 cloud base 域应使用的 HTTP scheme。仅「回环 + 逃生明文」→ http，否则 https。 */
export function cloudHttpScheme(domain: string, plaintext: boolean = DEV_PLAINTEXT): 'http' | 'https' {
  return isInsecureCloudDomain(domain) && plaintext ? 'http' : 'https';
}

/** 某 cloud base 域应使用的 WebSocket scheme。仅「回环 + 逃生明文」→ ws，否则 wss。 */
export function cloudWsScheme(domain: string, plaintext: boolean = DEV_PLAINTEXT): 'ws' | 'wss' {
  return isInsecureCloudDomain(domain) && plaintext ? 'ws' : 'wss';
}
```

注意：保留上方 `isInsecureCloudDomain`（line 33）与其上注释不动；它仍是判「是否回环」的纯函数，此处复用。`API_BASE`（line 62）经 `cloudHttpScheme(BASE_DOMAIN)` 自动变为 dev `https://localhost:5001/api/v1`，无需另改。调用方 `ridgeCloudProvider.ts:453` / `controllerCloudProvider.ts:336` 只传一个参数，默认 `DEV_PLAINTEXT`，不受影响。

- [ ] **Step 4: 跑测试看通过（GREEN）**

Run: `pnpm vitest run src/lib/remote/cloud/apiClient.test.ts`
Expected: PASS（含 `isInsecureCloudDomain` 既有用例）。

- [ ] **Step 5: 提交**

```powershell
git -C C:\code\wind add src/lib/remote/cloud/apiClient.ts src/lib/remote/cloud/apiClient.test.ts
git -C C:\code\wind commit -m @'
feat(dev-tls): 客户端回环 cloud 默认走 https/wss

cloudHttpScheme/cloudWsScheme 默认对回环也返回 TLS scheme（dev 全链路
TLS），新增 plaintext 逃生参数（默认读 RIDGE_CLOUD_DEV_PLAINTEXT）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 2: vite.config 注入 RIDGE_CLOUD_DEV_PLAINTEXT define（wind）

**Files:**
- Modify: `C:\code\wind\vite.config.js:46`（`define` 块内）

- [ ] **Step 1: 加 define**

在 `vite.config.js` 的 `define` 块里，紧跟 `'import.meta.env.RIDGE_CLOUD_BASE_DOMAIN': ...` 那一行之后，新增：

```js
    // Dev TLS 逃生开关：dev 默认全链路 TLS（apiClient cloudHttpScheme/cloudWsScheme）；
    // 置 RIDGE_CLOUD_DEV_PLAINTEXT=1 时回环 cloud 回退明文 http/ws（mkcert 故障调试）。
    'import.meta.env.RIDGE_CLOUD_DEV_PLAINTEXT': JSON.stringify(process.env.RIDGE_CLOUD_DEV_PLAINTEXT || ''),
```

- [ ] **Step 2: 验证默认行为不变（define 缺省为空 → DEV_PLAINTEXT=false）**

Run: `pnpm vitest run src/lib/remote/cloud/apiClient.test.ts`
Expected: PASS（仍全绿；未设 env 时回环默认 TLS）。

- [ ] **Step 3: 提交**

```powershell
git -C C:\code\wind add vite.config.js
git -C C:\code\wind commit -m @'
feat(dev-tls): vite 注入 RIDGE_CLOUD_DEV_PLAINTEXT define

让运行时 env 能控制客户端回环 cloud 明文逃生开关，默认空（=TLS）。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 3: cloud BYO 证书优先加载（ridge-cloud, Rust TDD）

**Files:**
- Modify: `C:\code\ridge-cloud\src\tls.rs`（新增 `CertSource` + `select_cert_source` + `resolve_dev_config` BYO 分支 + tests mod）

- [ ] **Step 1: 写失败测试（RED）**

在 `tls.rs` 末尾（`now_unix` 函数之后）追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byo_cert_source_when_both_env_paths_set() {
        let src = select_cert_source(Some("C:/c.pem".into()), Some("C:/k.pem".into()));
        assert!(matches!(src, CertSource::Byo { .. }));
    }

    #[test]
    fn generate_cert_source_when_either_env_missing() {
        assert!(matches!(select_cert_source(None, None), CertSource::Generate));
        assert!(matches!(select_cert_source(Some("c".into()), None), CertSource::Generate));
        assert!(matches!(select_cert_source(None, Some("k".into())), CertSource::Generate));
    }
}
```

- [ ] **Step 2: 跑测试看失败（RED）**

Run: `cargo test --manifest-path C:\code\ridge-cloud\Cargo.toml --lib tls::tests`
Expected: 编译失败 —— `cannot find type CertSource` / `cannot find function select_cert_source`。

- [ ] **Step 3: 实现（GREEN）**

在 `tls.rs` 的 `resolve_dev_config` 函数**之前**（约 line 23，紧邻其文档注释上方）插入：

```rust
/// dev TLS 证书来源：BYO（外部签发，如 mkcert，env 指定路径）或内置 rcgen 自签。
#[derive(Debug)]
enum CertSource {
    Byo { cert: PathBuf, key: PathBuf },
    Generate,
}

/// 选择证书来源：cert+key 两个 env 路径都给齐 → BYO；否则回退 rcgen 自签。
fn select_cert_source(cert_path: Option<String>, key_path: Option<String>) -> CertSource {
    match (cert_path, key_path) {
        (Some(c), Some(k)) => CertSource::Byo {
            cert: PathBuf::from(c),
            key: PathBuf::from(k),
        },
        _ => CertSource::Generate,
    }
}
```

再在 `resolve_dev_config` 函数体开头、`let _ = rustls::crypto::ring::default_provider()...` 之后、`let dir = tls_dir();` 之前插入 BYO 分支：

```rust
    // BYO 证书优先（mkcert 等，SAN 含 *.localhost）：env 给齐路径则直接加载。
    if let CertSource::Byo { cert, key } = select_cert_source(
        std::env::var("RIDGE_CLOUD_TLS_CERT").ok(),
        std::env::var("RIDGE_CLOUD_TLS_KEY").ok(),
    ) {
        match (std::fs::read(&cert), std::fs::read(&key)) {
            (Ok(c), Ok(k)) => return build_config(c, k).await,
            _ => tracing::warn!(?cert, ?key, "RIDGE_CLOUD_TLS_CERT/KEY 指定但读取失败，回退 rcgen 自签"),
        }
    }
```

（`PathBuf` 已在 line 3 `use std::path::{Path, PathBuf};` 导入，无需新增 use。）

- [ ] **Step 4: 跑测试看通过（GREEN）**

Run: `cargo test --manifest-path C:\code\ridge-cloud\Cargo.toml --lib tls::tests`
Expected: `2 passed; 0 failed`

- [ ] **Step 5: 确认全 lib 未回归**

Run: `cargo test --manifest-path C:\code\ridge-cloud\Cargo.toml --lib router::tests tls::tests`
Expected: 全部 PASS（router 7 + tls 2）。

- [ ] **Step 6: 提交**

```powershell
git -C C:\code\ridge-cloud add src/tls.rs
git -C C:\code\ridge-cloud commit -m @'
feat(dev-tls): tls.rs 支持 BYO 证书（RIDGE_CLOUD_TLS_CERT/KEY）

env 给齐 cert+key 路径则直接加载 mkcert 等外部受信证书（SAN 含
*.localhost），缺失回退现有 rcgen 自签。新增 select_cert_source 单测。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 4: mkcert 证书准备脚本（ridge-cloud）

**Files:**
- Create: `C:\code\ridge-cloud\scripts\setup-dev-tls.mjs`

- [ ] **Step 1: 写脚本**

新建 `ridge-cloud/scripts/setup-dev-tls.mjs`，内容：

```js
// 本地 dev TLS 证书准备：用 mkcert 建本地受信 CA + 签 *.localhost 通配证书，
// 供 ridge-cloud dev HTTPS 复用（cloud 读 RIDGE_CLOUD_TLS_CERT/KEY）。
// 幂等：CA 已装则 -install 跳过；证书覆盖重签（廉价）。
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';

function hasMkcert() {
  return spawnSync('mkcert', ['-version'], { stdio: 'ignore', shell: true }).status === 0;
}

if (!hasMkcert()) {
  console.error('[setup-dev-tls] 未找到 mkcert，请先安装：');
  console.error('  scoop install mkcert      # 或：winget install FiloSottile.mkcert');
  process.exit(1);
}

// 1. 安装本地 CA 到系统受信根（幂等；首次可能弹 UAC）。
console.log('[setup-dev-tls] mkcert -install（装本地 CA 到系统受信根）…');
const install = spawnSync('mkcert', ['-install'], { stdio: 'inherit', shell: true });
if (install.status !== 0) {
  console.error('[setup-dev-tls] mkcert -install 失败');
  process.exit(install.status ?? 1);
}

// 2. 证书输出目录（与 ridge-cloud tls.rs::tls_dir 对齐：%LOCALAPPDATA%\ridge\cloud-tls）。
const baseDir = process.env.LOCALAPPDATA || path.join(process.env.HOME || '.', '.local', 'share');
const outDir = path.join(baseDir, 'ridge', 'cloud-tls');
if (!existsSync(outDir)) mkdirSync(outDir, { recursive: true });
const certPath = path.join(outDir, 'cert.pem');
const keyPath = path.join(outDir, 'key.pem');

// 3. 签发含 *.localhost 通配 + 回环 IP 的叶子证书。
console.log('[setup-dev-tls] 生成通配证书 →', outDir);
const gen = spawnSync(
  'mkcert',
  ['-cert-file', certPath, '-key-file', keyPath, 'localhost', '*.localhost', '127.0.0.1', '::1'],
  { stdio: 'inherit', shell: true },
);
if (gen.status !== 0) {
  console.error('[setup-dev-tls] 证书生成失败');
  process.exit(gen.status ?? 1);
}

console.log('[setup-dev-tls] ✅ 完成。dev.sh 将 export：');
console.log(`  RIDGE_CLOUD_TLS_CERT=${certPath}`);
console.log(`  RIDGE_CLOUD_TLS_KEY=${keyPath}`);
```

- [ ] **Step 2: 手动验证（需已 `scoop install mkcert`）**

Run（PowerShell 或 git-bash，cwd=ridge-cloud）: `node scripts/setup-dev-tls.mjs`
Expected: 打印 `mkcert -install` 输出 + `✅ 完成`，且 `%LOCALAPPDATA%\ridge\cloud-tls\` 下出现 `cert.pem` / `key.pem`。

验证证书含通配 SAN：
Run: `mkcert -version`（确认装好）；用浏览器在 Task 6 实测 `https://x.localhost:5001`。

- [ ] **Step 3: 提交**

```powershell
git -C C:\code\ridge-cloud add scripts/setup-dev-tls.mjs
git -C C:\code\ridge-cloud commit -m @'
feat(dev-tls): 新增 setup-dev-tls.mjs（mkcert 备证书）

幂等准备本地受信 CA + *.localhost 通配证书到 %LOCALAPPDATA%\ridge\
cloud-tls，供 cloud 经 RIDGE_CLOUD_TLS_CERT/KEY 读取。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 5: dev.sh 默认 https/wss + 调 setup-dev-tls（ridge-cloud，回调上一轮）

**Files:**
- Modify: `C:\code\ridge-cloud\scripts\dev.sh`（line 6 注释、line 16-18 banner、line 77-82 env 区）

- [ ] **Step 1: 改 env 默认值 + 接入 setup-dev-tls**

把 `dev.sh` 当前的 env 区块（上一轮的「默认 http」版）：

```bash
# 默认明文 http：wind 客户端对 localhost 走 http（见 apiClient.ts isInsecureCloudDomain，
# 契约「本地回环走明文 http/ws」）。需要测 TLS/wss 时显式 RIDGE_CLOUD_DEV_HTTPS=1 启动。
export RIDGE_CLOUD_DEV_HTTPS="${RIDGE_CLOUD_DEV_HTTPS:-0}"
SCHEME=$([[ "$RIDGE_CLOUD_DEV_HTTPS" == "1" ]] && echo https || echo http)

echo "✅ 环境配置: API=$SCHEME://localhost:$PORT, Admin=$ADMIN_DEV_PORT"
```

替换为：

```bash
# 默认全链路 TLS（https/wss），与 wind 客户端 dev 默认 TLS 对齐。先用 mkcert 备好
# 受信证书（含 *.localhost 通配），cloud 经 RIDGE_CLOUD_TLS_CERT/KEY 读取。
# 临时回明文调试：RIDGE_CLOUD_DEV_HTTPS=0 ./scripts/dev.sh
export RIDGE_CLOUD_DEV_HTTPS="${RIDGE_CLOUD_DEV_HTTPS:-1}"
SCHEME=$([[ "$RIDGE_CLOUD_DEV_HTTPS" == "1" ]] && echo https || echo http)

if [[ "$RIDGE_CLOUD_DEV_HTTPS" == "1" ]]; then
    echo "🔐 准备 dev TLS 证书 (mkcert)…"
    node "$ROOT_DIR/scripts/setup-dev-tls.mjs"
    TLS_DIR="${LOCALAPPDATA:-$HOME/.local/share}/ridge/cloud-tls"
    export RIDGE_CLOUD_TLS_CERT="$TLS_DIR/cert.pem"
    export RIDGE_CLOUD_TLS_KEY="$TLS_DIR/key.pem"
fi

echo "✅ 环境配置: API=$SCHEME://localhost:$PORT, Admin=$ADMIN_DEV_PORT"
```

- [ ] **Step 2: 改顶部 banner（line 16-18）**

把：

```bash
echo "  API:       http://localhost:5001   (默认明文 http，与 wind 客户端对齐)"
echo "  Admin:     http://localhost:5002"
echo "  WebSocket: ws://localhost:5001/ws"
```

替换为：

```bash
echo "  API:       https://localhost:5001   (默认 TLS/mkcert；RIDGE_CLOUD_DEV_HTTPS=0 回明文)"
echo "  Admin:     http://localhost:5002"
echo "  WebSocket: wss://localhost:5001/ws"
```

- [ ] **Step 3: 改 line 6 注释**

把：`#   - 启动 ridge-cloud API (http://localhost:5001) + Admin (5002)`
替换为：`#   - 启动 ridge-cloud API (https://localhost:5001, mkcert TLS) + Admin (5002)`

- [ ] **Step 4: 语法校验**

Run: `bash -n C:/code/ridge-cloud/scripts/dev.sh; echo $?`
Expected: 输出 `0`（无语法错误）。

- [ ] **Step 5: 提交**

```powershell
git -C C:\code\ridge-cloud add scripts/dev.sh
git -C C:\code\ridge-cloud commit -m @'
feat(dev-tls): dev.sh 默认 https/wss 并接入 mkcert 证书

回调上一轮的明文默认；启动前跑 setup-dev-tls.mjs 备证书并 export
RIDGE_CLOUD_TLS_CERT/KEY；保留 RIDGE_CLOUD_DEV_HTTPS=0 回明文逃生。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
'@
```

---

### Task 6: 手动 e2e 验收

**Files:** 无（端到端验证）

- [ ] **Step 1: 装 mkcert（若未装）**

Run: `scoop install mkcert`
Expected: mkcert 进 PATH（`mkcert -version` 可用）。

- [ ] **Step 2: 起本地 cloud（git-bash，cwd=ridge-cloud）**

Run: `./scripts/dev.sh`
Expected: banner 显示 `https://localhost:5001` / `wss://…`；`setup-dev-tls` 打印 `✅ 完成`；日志出现「Dev 模式：启用 HTTPS」。

- [ ] **Step 3: 证书受信验证**

浏览器访问 `https://localhost:5001` 与 `https://demo-alice.localhost:5001`。
Expected: 两者均**无证书警告**（mkcert CA 已受信，`*.localhost` 通配命中）。

- [ ] **Step 4: 起 wind 并登录**

Run（cwd=wind）: `pnpm tauri dev`
点击登录 → 浏览器打开授权页 → 批准。
Expected: 登录成功，无「网络错误」。DevTools Network 显示 `https://localhost:5001/api/v1/auth/request` 200，无 `ERR_CERT_*` / CORS 报错。

- [ ] **Step 5: 远控信令验证**

发起一次云端远控连接。
Expected: 信令 `wss://{device}-{username}.localhost:5001/ws` 连接建立成功（DevTools → Network → WS 101 Switching Protocols）。

- [ ] **Step 6: 逃生开关回归（可选）**

Run（cloud）: `RIDGE_CLOUD_DEV_HTTPS=0 ./scripts/dev.sh`；Run（wind）: `RIDGE_CLOUD_DEV_PLAINTEXT=1 pnpm tauri dev`
Expected: 两侧成对回明文（http/ws），登录仍可走通（验证逃生通道未失效）。

---

## Self-Review

- **Spec coverage：** A 证书/mkcert→T4；B cloud(tls.rs→T3, dev.sh→T5, CORS 不改→T0 提交既有改动)；C 客户端(scheme→T1, vite define→T2)；D 数据流→T6；E 测试→T1/T3 TDD + T6 e2e。全覆盖。
- **逃生开关一致性：** 客户端 `RIDGE_CLOUD_DEV_PLAINTEXT`（T1/T2）与 cloud `RIDGE_CLOUD_DEV_HTTPS=0`（T5）成对，T6 Step 6 验证。
- **类型/签名一致：** `CertSource`/`select_cert_source`（T3）；`cloudHttpScheme/cloudWsScheme(domain, plaintext=DEV_PLAINTEXT)`（T1）跨 T2/T6 一致；env 名 `RIDGE_CLOUD_TLS_CERT/KEY` 在 T3/T4/T5 统一。
