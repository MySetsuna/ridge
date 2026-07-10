# Remote 产物部署解耦 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 wind 侧一条 `pnpm publish:remote-cloud` 把桌面/手机 Remote 产物直接发布到 ridge-cloud 持久卷、即时生效，零后端重部署。

**Architecture:** ridge-cloud 新增鉴权上传端点 `POST /api/v1/remote-artifacts`，收自描述 bundle → 校验 → 落卷 `releases/<ver>` → 原子换 `desktop-app`/`mobile-app` current 指针（`static_host` 每请求读盘，换即生效）；产物目录改由 env 指向卷。wind 侧新增 `scripts/publish-remote-cloud.mjs` 构建+打包+上传。

**Tech Stack:** ridge-cloud=Rust/axum/tokio/serde_json/subtle（零新依赖）；wind=Node ESM 脚本 + vitest。跨仓：设计稿在 wind，代码分落两仓。

## Global Constraints

- ridge-cloud **零新增 cargo 依赖**：bundle 用 `serde_json`，token 恒等比较用 `subtle::ConstantTimeEq`，IO 用 `tokio::fs`。
- Bundle 线格式（SSOT，两端一致）：`[u32 BE header_len][JSON 头][文件体拼接]`；JSON 头 `{manifest:{version,gitSha,builtAt}, files:[{path,size}]}`；path 相对、正斜杠、无 `..`/绝对。
- 生产 = Linux/Dokku；本地 dev = Windows。symlink 换手 `#[cfg(unix)]`，非 unix 回退双 rename；symlink 单测 `#[cfg(unix)]`。
- 端点不进 `/api/v1/admin`（AdminAuth JWT 组）；用专用 `RIDGE_ARTIFACT_TOKEN`。未配 token → 端点 503。
- 卷根 env `REMOTE_ARTIFACTS_ROOT` 缺省 `/data/remote-apps`；body 上限 env `REMOTE_ARTIFACT_MAX_BYTES` 缺省 64 MiB；保留 release 数 N=3。
- 每任务 TDD：先写失败测试 → 跑挂 → 最小实现 → 跑绿 → 提交。ridge-cloud 独立 target 可自跑 `cargo test`；wind 用 `pnpm exec vitest run`。

---

## File Structure

**ridge-cloud（C:\code\ridge-cloud）**
- Create `src/api/remote_artifacts.rs` — bundle 解析纯函数 + activate/rollback/prune + upload/rollback handler。
- Modify `src/config.rs` — 加 3 个配置项 + 解析。
- Modify `src/api/mod.rs` — `pub mod remote_artifacts;`
- Modify `src/router.rs` — 装配两路由 + 单独 body limit。
- Modify `Dockerfile` — 删 `COPY desktop-app` / `COPY mobile-app`。
- Modify `.gitignore` — 加 `desktop-app/` `mobile-app/`；`git rm -r --cached desktop-app mobile-app`。

**wind（C:\code\wind）**
- Create `scripts/publish-remote-cloud.mjs` — CLI 入口。
- Create `scripts/lib/remoteArtifactBundle.mjs` — 纯函数 `buildManifest/collectFiles/packBundle/resolveConfig`。
- Create `scripts/lib/remoteArtifactBundle.test.ts` — vitest 纯函数单测。
- Modify `package.json` — 加 `publish:remote-cloud` 脚本。

---

## Task 1: ridge-cloud config 三配置项

**Files:**
- Modify: `C:\code\ridge-cloud\src\config.rs`

**Interfaces:**
- Produces: `AppConfig.remote_artifact_token: Option<String>`、`remote_artifacts_root: String`、`remote_artifact_max_bytes: usize`。

- [ ] **Step 1: 写失败测试**（config.rs `#[cfg(test)]` 内）
```rust
#[test]
fn remote_artifact_config_defaults_and_env() {
    // 未设 → token None、root 默认、max 默认 64 MiB。
    std::env::remove_var("RIDGE_ARTIFACT_TOKEN");
    std::env::remove_var("REMOTE_ARTIFACTS_ROOT");
    std::env::remove_var("REMOTE_ARTIFACT_MAX_BYTES");
    let c = AppConfig::from_env_for_test();
    assert_eq!(c.remote_artifact_token, None);
    assert_eq!(c.remote_artifacts_root, "/data/remote-apps");
    assert_eq!(c.remote_artifact_max_bytes, 64 * 1024 * 1024);
}
```
> 注：若 `AppConfig` 无 `from_env_for_test`，改为对已有构造入口断言；先读 config.rs 现有 test 模式对齐（勿新造不一致的构造）。

- [ ] **Step 2: 跑挂** `cargo test -p ridge-cloud config::` → FAIL（字段不存在）。
- [ ] **Step 3: 实现**：在 `AppConfig` 结构加 3 字段；在构造函数（`optional_var` 所在处，config.rs:157 附近）加：
```rust
let remote_artifact_token = optional_var("RIDGE_ARTIFACT_TOKEN");
let remote_artifacts_root =
    optional_var("REMOTE_ARTIFACTS_ROOT").unwrap_or_else(|| "/data/remote-apps".to_string());
let remote_artifact_max_bytes = optional_var("REMOTE_ARTIFACT_MAX_BYTES")
    .and_then(|s| s.parse::<usize>().ok())
    .unwrap_or(64 * 1024 * 1024);
```
并在结构体字面量里补上三字段。
- [ ] **Step 4: 跑绿** `cargo test -p ridge-cloud config::`。
- [ ] **Step 5: 提交** `git -C C:\code\ridge-cloud commit -am "feat(config): remote artifact token/root/max 配置项"`。

---

## Task 2: bundle 解析纯函数

**Files:**
- Create: `C:\code\ridge-cloud\src\api\remote_artifacts.rs`
- Modify: `C:\code\ridge-cloud\src\api\mod.rs`（加 `pub mod remote_artifacts;`）

**Interfaces:**
- Produces:
  - `struct BundleHeader { manifest: Manifest, files: Vec<FileEntry> }`、`struct Manifest{version,git_sha,built_at:String}`、`struct FileEntry{path:String,size:u64}`（serde）。
  - `fn parse_header(body: &[u8]) -> Result<(BundleHeader, usize /*body_offset*/), ArtifactError>`
  - `fn sanitize_rel(path: &str) -> Result<PathBuf, ArtifactError>`（拒 `..`/绝对/反斜杠/空）
  - `fn verify_token(configured: Option<&str>, presented: Option<&str>) -> bool`（subtle 恒等）
  - `enum ArtifactError`（`thiserror`）。

- [ ] **Step 1: 写失败测试**（remote_artifacts.rs `#[cfg(test)]`）
```rust
#[test]
fn parse_header_reads_len_prefixed_json() {
    let header = r#"{"manifest":{"version":"1","gitSha":"a","builtAt":"t"},"files":[{"path":"desktop-app/index.html","size":3}]}"#;
    let mut body = (header.len() as u32).to_be_bytes().to_vec();
    body.extend_from_slice(header.as_bytes());
    body.extend_from_slice(b"abc");
    let (h, off) = parse_header(&body).unwrap();
    assert_eq!(h.files[0].size, 3);
    assert_eq!(&body[off..], b"abc");
}
#[test]
fn sanitize_rejects_traversal_and_absolute() {
    assert!(sanitize_rel("../x").is_err());
    assert!(sanitize_rel("/etc/x").is_err());
    assert!(sanitize_rel("a\\b").is_err());
    assert_eq!(sanitize_rel("desktop-app/index.html").unwrap(), PathBuf::from("desktop-app/index.html"));
}
#[test]
fn verify_token_constant_time_semantics() {
    assert!(!verify_token(None, Some("x")));          // 未配 → 拒
    assert!(!verify_token(Some("s"), None));           // 未带 → 拒
    assert!(!verify_token(Some("s"), Some("t")));      // 不等 → 拒
    assert!(verify_token(Some("s3cr3t"), Some("s3cr3t")));
}
```
- [ ] **Step 2: 跑挂** `cargo test -p ridge-cloud remote_artifacts::` → FAIL。
- [ ] **Step 3: 实现**（关键片段）
```rust
use std::path::{Path, PathBuf};
use serde::Deserialize;
use subtle::ConstantTimeEq;

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("bundle too short")] TooShort,
    #[error("bad header json: {0}")] BadHeader(String),
    #[error("size mismatch")] SizeMismatch,
    #[error("unsafe path: {0}")] UnsafePath(String),
    #[error("missing index.html in {0}")] MissingIndex(&'static str),
    #[error("io: {0}")] Io(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)] pub struct Manifest {
    pub version: String,
    #[serde(rename = "gitSha")] pub git_sha: String,
    #[serde(rename = "builtAt")] pub built_at: String,
}
#[derive(Debug, Deserialize)] pub struct FileEntry { pub path: String, pub size: u64 }
#[derive(Debug, Deserialize)] pub struct BundleHeader { pub manifest: Manifest, pub files: Vec<FileEntry> }

pub fn parse_header(body: &[u8]) -> Result<(BundleHeader, usize), ArtifactError> {
    if body.len() < 4 { return Err(ArtifactError::TooShort); }
    let n = u32::from_be_bytes([body[0],body[1],body[2],body[3]]) as usize;
    let start = 4; let end = start + n;
    if body.len() < end { return Err(ArtifactError::TooShort); }
    let header: BundleHeader = serde_json::from_slice(&body[start..end])
        .map_err(|e| ArtifactError::BadHeader(e.to_string()))?;
    let sum: u64 = header.files.iter().map(|f| f.size).sum();
    if end as u64 + sum != body.len() as u64 { return Err(ArtifactError::SizeMismatch); }
    Ok((header, end))
}

pub fn sanitize_rel(path: &str) -> Result<PathBuf, ArtifactError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return Err(ArtifactError::UnsafePath(path.to_string()));
    }
    let mut out = PathBuf::new();
    for seg in path.split('/') {
        match seg { "" | "." | ".." => return Err(ArtifactError::UnsafePath(path.to_string())), s => out.push(s) }
    }
    Ok(out)
}

pub fn verify_token(configured: Option<&str>, presented: Option<&str>) -> bool {
    match (configured, presented) {
        (Some(c), Some(p)) => c.as_bytes().ct_eq(p.as_bytes()).into(),
        _ => false,
    }
}
```
在 `src/api/mod.rs` 加 `pub mod remote_artifacts;`。
- [ ] **Step 4: 跑绿** `cargo test -p ridge-cloud remote_artifacts::`。
- [ ] **Step 5: 提交** `git -C C:\code\ridge-cloud commit -am "feat(remote-artifacts): bundle 头解析/路径穿越防护/token 恒等比较"`。

---

## Task 3: 落盘 + activate/rollback/prune

**Files:**
- Modify: `C:\code\ridge-cloud\src\api\remote_artifacts.rs`

**Interfaces:**
- Consumes: Task 2 的 `BundleHeader/FileEntry/sanitize_rel/ArtifactError`。
- Produces:
  - `async fn write_release(root:&Path, ver:&str, header:&BundleHeader, bodies:&[u8]) -> Result<PathBuf, ArtifactError>`（写 `.incoming-<ver>`，校验两 index.html，rename 成 `releases/<ver>`）
  - `fn activate(root:&Path, ver:&str) -> Result<(), ArtifactError>`（换 desktop-app/mobile-app current，平台分叉）
  - `fn prune(root:&Path, keep:usize) -> Result<Vec<String>, ArtifactError>`
  - `fn list_releases(root:&Path) -> Vec<String>`

- [ ] **Step 1: 写失败测试**（跨平台部分 + `#[cfg(unix)]` symlink 部分）
```rust
#[tokio::test]
async fn write_release_rejects_missing_index() {
    let root = unique_root("noidx");
    let header = BundleHeader{ manifest: m(), files: vec![fe("desktop-app/x.js", 1)] }; // 无 index.html
    let bodies = b"y";
    let err = write_release(&root, "1", &header, bodies).await.unwrap_err();
    assert!(matches!(err, ArtifactError::MissingIndex(_)));
    std::fs::remove_dir_all(&root).ok();
}
#[test]
fn prune_keeps_latest_n() {
    let root = unique_root("prune");
    for v in ["0.0.1","0.0.2","0.0.3","0.0.4"] { std::fs::create_dir_all(root.join("releases").join(v)).unwrap(); }
    let removed = prune(&root, 3).unwrap();
    assert_eq!(list_releases(&root).len(), 3);
    assert!(removed.contains(&"0.0.1".to_string())); // 最旧被清
    std::fs::remove_dir_all(&root).ok();
}
#[cfg(unix)]
#[tokio::test]
async fn activate_swaps_current_and_serves_new() {
    let root = unique_root("act");
    // 造 releases/1/{desktop-app,mobile-app}/index.html
    for app in ["desktop-app","mobile-app"] {
        let d = root.join("releases").join("1").join(app);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("index.html"), b"v1").unwrap();
    }
    activate(&root, "1").unwrap();
    // current 指针读到 v1
    let served = std::fs::read(root.join("desktop-app").join("index.html")).unwrap();
    assert_eq!(served, b"v1");
    std::fs::remove_dir_all(&root).ok();
}
```
> `prune` 用「版本目录 mtime 排序」而非语义化版本比较（避免引 semver；发布单调递增，mtime 足够，同 sha 覆盖不新增）。
- [ ] **Step 2: 跑挂** → FAIL。
- [ ] **Step 3: 实现**（关键片段）
```rust
use tokio::fs as afs;

pub async fn write_release(root:&Path, ver:&str, header:&BundleHeader, bodies:&[u8]) -> Result<PathBuf, ArtifactError> {
    let incoming = root.join(format!(".incoming-{ver}"));
    let _ = afs::remove_dir_all(&incoming).await;
    let mut off = 0usize;
    for f in &header.files {
        let rel = sanitize_rel(&f.path)?;
        let dst = incoming.join(&rel);
        if let Some(p) = dst.parent() { afs::create_dir_all(p).await?; }
        let end = off + f.size as usize;
        afs::write(&dst, &bodies[off..end]).await?;
        off = end;
    }
    if !incoming.join("desktop-app").join("index.html").exists() { return Err(ArtifactError::MissingIndex("desktop-app")); }
    if !incoming.join("mobile-app").join("index.html").exists() { return Err(ArtifactError::MissingIndex("mobile-app")); }
    let release = root.join("releases").join(ver);
    afs::create_dir_all(root.join("releases")).await?;
    let _ = afs::remove_dir_all(&release).await;
    afs::rename(&incoming, &release).await?;
    Ok(release)
}

fn point_current(root:&Path, app:&str, ver:&str) -> Result<(), ArtifactError> {
    let link = root.join(app);
    let target = root.join("releases").join(ver).join(app);
    #[cfg(unix)] {
        let tmp = root.join(format!(".{app}.swap"));
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(&target, &tmp)?;
        std::fs::rename(&tmp, &link)?; // 原子替换
    }
    #[cfg(not(unix))] {
        let prev = root.join(format!("{app}.prev"));
        let _ = std::fs::remove_dir_all(&prev);
        if link.exists() { std::fs::rename(&link, &prev)?; }
        // dev 回退：复制 release 到 current（非原子；仅本地开发用）
        copy_dir_all(&target, &link)?;
    }
    Ok(())
}
pub fn activate(root:&Path, ver:&str) -> Result<(), ArtifactError> {
    point_current(root, "desktop-app", ver)?;
    point_current(root, "mobile-app", ver)?;
    Ok(())
}

pub fn list_releases(root:&Path) -> Vec<String> {
    let mut v: Vec<(std::time::SystemTime,String)> = std::fs::read_dir(root.join("releases")).into_iter().flatten()
        .flatten().filter(|e| e.path().is_dir())
        .filter_map(|e| { let n=e.file_name().to_string_lossy().to_string();
            let t=e.metadata().and_then(|m|m.modified()).ok()?; Some((t,n)) }).collect();
    v.sort_by(|a,b| a.0.cmp(&b.0));
    v.into_iter().map(|(_,n)| n).collect()
}
pub fn prune(root:&Path, keep:usize) -> Result<Vec<String>, ArtifactError> {
    let all = list_releases(root);
    if all.len() <= keep { return Ok(vec![]); }
    let remove: Vec<String> = all[..all.len()-keep].to_vec();
    for v in &remove { let _ = std::fs::remove_dir_all(root.join("releases").join(v)); }
    Ok(remove)
}
```
外加 `copy_dir_all`（`#[cfg(not(unix))]`）与测试助手 `unique_root/m/fe`（临时目录，仿 static_host.rs 的 `unique_build_dir`）。
- [ ] **Step 4: 跑绿** `cargo test -p ridge-cloud remote_artifacts::`（Windows 上 symlink 测试自动跳过）。
- [ ] **Step 5: 提交** `git -C C:\code\ridge-cloud commit -am "feat(remote-artifacts): 落盘校验 + activate/rollback/prune（平台分叉换手）"`。

---

## Task 4: upload/rollback handler + 路由

**Files:**
- Modify: `C:\code\ridge-cloud\src\api\remote_artifacts.rs`（加 handler）
- Modify: `C:\code\ridge-cloud\src\router.rs`

**Interfaces:**
- Consumes: Task 1 config、Task 2/3 纯函数。
- Produces: `async fn upload(State, HeaderMap, Bytes) -> Response`、`async fn rollback(...)`。

- [ ] **Step 1: 写失败测试**（用 axum 直接调 handler 或 `tower::ServiceExt::oneshot`；仿现有 handler 测试风格——先读 router.rs/现有 api 测试对齐）
```rust
#[tokio::test]
async fn upload_rejects_bad_token() { /* 构造 State(token=Some("s")) + Bearer "x" → 401 */ }
#[tokio::test]
async fn upload_activates_and_serves() { /* 好 bundle + 对 token → 200，随后 spa_handler 读到新 index.html（unix） */ }
```
- [ ] **Step 2: 跑挂** → FAIL。
- [ ] **Step 3: 实现** handler：抽 Bearer → `verify_token`（config.remote_artifact_token）→ 未配 503 → `parse_header` → `write_release(root, ver, header, &body[off..])` → `activate` → `prune(root, 3)` → `Ok(Json{version,activatedAt,kept})`。rollback：读 `list_releases`，取上一个（或 `{to}`），`activate`。router.rs 装配：
```rust
let artifacts = Router::new()
    .route("/remote-artifacts", post(remote_artifacts::upload))
    .route("/remote-artifacts/rollback", post(remote_artifacts::rollback))
    .layer(DefaultBodyLimit::max(state.config.remote_artifact_max_bytes));
// 并入 /api/v1 nest（与其它 api 同级），套 rate_limit_general、不套 AdminAuth。
```
- [ ] **Step 4: 跑绿** `cargo test -p ridge-cloud remote_artifacts::` + `cargo check -p ridge-cloud`。
- [ ] **Step 5: 提交** `git -C C:\code\ridge-cloud commit -am "feat(remote-artifacts): upload/rollback handler + /api/v1 路由 + body limit"`。

---

## Task 5: Dockerfile 去 COPY + 仓库清理

**Files:**
- Modify: `C:\code\ridge-cloud\Dockerfile`
- Modify: `C:\code\ridge-cloud\.gitignore`

- [ ] **Step 1: 改 Dockerfile**：删 `COPY desktop-app`/`COPY mobile-app` 两行（保留 `web/build`、`admin-app/build`）。
- [ ] **Step 2: .gitignore 加** `desktop-app/`、`mobile-app/`。
- [ ] **Step 3: 移除检入产物** `git -C C:\code\ridge-cloud rm -r --cached desktop-app mobile-app`（保留工作区文件，仅脱离跟踪）。
- [ ] **Step 4: 验证** `git -C C:\code\ridge-cloud status` 显示两目录已 untracked；`cargo check -p ridge-cloud` 仍绿。
- [ ] **Step 5: 提交** `git -C C:\code\ridge-cloud commit -m "chore(deploy): 产物改持久卷托管——去 Dockerfile COPY + 脱管 desktop-app/mobile-app"`。

---

## Task 6: wind 发布器纯函数 + vitest

**Files:**
- Create: `C:\code\wind\scripts\lib\remoteArtifactBundle.mjs`
- Create: `C:\code\wind\scripts\lib\remoteArtifactBundle.test.ts`

**Interfaces:**
- Produces（ESM）：
  - `collectFiles(dir, prefix) -> [{path, abs}]`（递归，正斜杠，prefix 前缀）
  - `packBundle(manifest, files) -> Buffer`（读每文件字节，按 §5.1 拼）
  - `buildManifest({version, gitSha, builtAt}) -> object`
  - `resolveConfig(env, argv) -> {url, token, build, dryRun, rollback}`

- [ ] **Step 1: 写失败测试**
```ts
import { describe, it, expect } from 'vitest';
import { packBundle, buildManifest, resolveConfig } from './remoteArtifactBundle.mjs';

it('packBundle frames u32 len + json header + bodies', () => {
  const buf = packBundle(buildManifest({version:'1',gitSha:'a',builtAt:'t'}),
    [{path:'desktop-app/index.html', bytes: Buffer.from('abc')}]);
  const n = buf.readUInt32BE(0);
  const header = JSON.parse(buf.subarray(4, 4+n).toString('utf8'));
  expect(header.files[0]).toEqual({path:'desktop-app/index.html', size:3});
  expect(buf.subarray(4+n).toString()).toBe('abc');
});
it('resolveConfig reads env + flags', () => {
  const c = resolveConfig({RIDGE_CLOUD_ARTIFACT_URL:'u', RIDGE_ARTIFACT_TOKEN:'t'}, ['--dry-run']);
  expect(c).toMatchObject({url:'u', token:'t', dryRun:true, build:true});
});
```
- [ ] **Step 2: 跑挂** `pnpm exec vitest run scripts/lib/remoteArtifactBundle.test.ts` → FAIL。
- [ ] **Step 3: 实现** `remoteArtifactBundle.mjs`：`packBundle` 用 `Buffer.concat([u32be(headerLen), headerBuf, ...bodies])`；`buildManifest` 组 `{manifest, files}` 时 `files` 用 `{path,size}`；`collectFiles` 递归 `fs.readdirSync` 拼正斜杠 path；`resolveConfig` 读 env + 扫 argv flags。
- [ ] **Step 4: 跑绿** vitest 绿。
- [ ] **Step 5: 提交** `git -C C:\code\wind commit -am "feat(publish): remote 产物 bundle 打包纯函数 + 单测"`。

---

## Task 7: wind 发布 CLI + package.json

**Files:**
- Create: `C:\code\wind\scripts\publish-remote-cloud.mjs`
- Modify: `C:\code\wind\package.json`

**Interfaces:**
- Consumes: Task 6 纯函数。

- [ ] **Step 1: 实现 CLI**：`resolveConfig(process.env, process.argv.slice(2))` → 除 `--no-build` 外 `execSync('pnpm build:desktop-web && pnpm build:remote')` → 校验两 index.html → `collectFiles('web-remote-dist','desktop-app') + collectFiles('static/remote','mobile-app')` → `packBundle` → `--dry-run` 落 `build/remote-artifact-<ver>.bundle` 否则 `fetch(url,{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/octet-stream'},body})` → 打印返回。`--rollback` 走 `/rollback`。gitSha=`execSync('git rev-parse --short HEAD')`。
- [ ] **Step 2: package.json 加** `"publish:remote-cloud": "node scripts/publish-remote-cloud.mjs"`。
- [ ] **Step 3: 验证** `pnpm publish:remote-cloud --dry-run --no-build`（若产物已在）产出 `.bundle` 且大小合理；或至少 `node -c` 语法检查 + `resolveConfig` 已单测。
- [ ] **Step 4: 提交** `git -C C:\code\wind commit -am "feat(publish): publish-remote-cloud CLI + pnpm 脚本"`。

---

## 交付后（用户执行的 ops，见设计稿 §8）

代码全绿后，输出割接手册给用户：`dokku storage:mount` + `dokku config:set RIDGE_ARTIFACT_TOKEN/DESKTOP_APP_DIR/MOBILE_APP_DIR` + 部署本方案版 + 首次 `pnpm publish:remote-cloud`。这些需主机权限，我出手册不代跑。

## Self-Review

- **Spec 覆盖**：§3 架构→Task1-7；§4 换手→Task3；§5 契约/bundle→Task2/4；§6 发布器→Task6/7；§7 一次性改动→Task1/4/5；§8 ops→交付手册；§10 测试→各任务 TDD。✅
- **Placeholder**：Task1 Step1 注明「按 config.rs 现有 test 模式对齐」，Task4 Step1 注明「按现有 handler 测试风格对齐」——这两处需执行时先读现有代码定构造/测试入口（非占位，是显式的对齐动作）。
- **类型一致**：`BundleHeader/FileEntry/Manifest`、`parse_header/sanitize_rel/verify_token/write_release/activate/prune`、Node `packBundle/buildManifest/collectFiles/resolveConfig` 跨任务一致。bundle 格式两端一致（u32 BE + JSON + bodies）。✅
