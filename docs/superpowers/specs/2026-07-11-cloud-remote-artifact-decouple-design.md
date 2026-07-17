# Remote 产物部署与 ridge-cloud 部署解耦 设计稿

> 日期：2026-07-11 · 状态：**代码已落地**（2026-07-17 核实：ridge-cloud `api/remote_artifacts.rs` upload/rollback/activate/prune + `router.rs:105-106` 二端点 + `config.rs` remote_artifacts_root + `Dockerfile:49` 已删 `COPY desktop-app`/`COPY mobile-app`；wind `scripts/publish-remote-cloud.mjs` 就位）。**余 §8 dokku ops 割接（挂卷 + config:set + 部署 + 首次 publish）须主机权限执行/确认**——代码不可确证生产是否已割接；割接验证后方可删 wind 旧 `scripts/sync-cloud-desktop-app.mjs` + `package.json` `sync:cloud-controller`。
> 关联：`2026-06-13-web-remote-perf-and-stale-bundle-design.md`（§待部署）、`2026-07-02-rdg-remote-unify-and-fixes-design.md`（§P1 步骤8 ua SSOT）、`docs/contracts/ridge-cloud-protocol.md`（§10 静态托管）
> 跨仓库：`C:\code\wind`（发布端）+ `C:\code\ridge-cloud`（接收端）

## 1. 背景与问题

Remote 前端有两份产物：桌面浏览器版 `web-remote-dist/`（`pnpm build:desktop-web`）与手机版 `static/remote/`（`pnpm build:remote`），二者在 wind 里都被 gitignore（本地构建产物）。

当前上云链路是**构建期检入式强耦合**：

```
wind 构建产物 ──(sync-cloud-desktop-app.mjs / 手工拷贝)──▶ ridge-cloud/desktop-app、mobile-app
                                                        ├─ git 检入（desktop-app 218 文件 + mobile-app 28 文件）
                                                        ├─ Dockerfile COPY desktop-app / COPY mobile-app
                                                        └─ git push dokku main → 整个 Rust 后端重新构建 + 部署
```

**痛点**：
1. 更新 Remote 前端必须走「wind 重建 → 拷进 ridge-cloud → 在 ridge-cloud 提交 → `git push dokku main` 重部署整个后端」，Remote 与后端部署强绑定，没有独立通道。
2. `mobile-app` 甚至没有同步脚本，靠手工拷贝，极易漏更新。
3. 结果：云端产物长期过期（同步只到 wind 0.0.13，部署分支 `main` 更旧到 06-25，0.0.15 及 07-04/07-06 远控修复从未上云）。
4. 前端产物（~246 文件、含数 MB bundle）检入 ridge-cloud 仓库、烘进镜像，污染仓库与镜像体积。

**目标**：在 wind 侧一条命令把两端 Remote 产物直接发布到云端、**即时生效**，**完全不触发 ridge-cloud 后端重部署**，ridge-cloud 仓库不再检入前端产物。

## 2. 关键前提（决定方案成立）

代码抽验确认两点，使「热替换、零重部署」天然成立：

1. **`static_host::spa_handler` 每请求直接读盘**（`ridge-cloud/src/static_host.rs:42-152`：`build_dir.join(rel)` → `candidate.is_file()` → `tokio::fs::read`），**无任何内存预载/缓存文件字节**。故磁盘上的产物一旦原子换掉，运行中的后端**下一请求即服务新版，无需重启/重载**。目录缺失时优雅降级 503（`:44-54`）。
2. **产物目录可用环境变量覆盖**（`ridge-cloud/src/config.rs:157-161`：`DESKTOP_APP_DIR` 缺省 `desktop-app`、`MOBILE_APP_DIR` 缺省 `mobile-app`，均 `optional_var` 读 env）。故可把两目录指到镜像外的**持久卷**。
3. 缓存语义天然友好（`static_host.rs:30-36,124-132`）：`_app/immutable/*` 内容指纹入文件名、长缓存；`index.html`/`sw.js`/`*.webmanifest` 一律 `no-cache`，发布即生效。

## 3. 架构（三块，接口清晰）

```
[wind] pnpm publish:remote-cloud
  1. pnpm build:desktop-web + build:remote      → web-remote-dist/ + static/remote/
  2. 打成一个自描述 bundle（长度前缀框架，见 §5.1；无压缩/无 tar，两端零新依赖）
  3. HTTPS POST https://<cloud>/api/v1/remote-artifacts   (Authorization: Bearer $RIDGE_ARTIFACT_TOKEN)
                         │
                         ▼
[ridge-cloud] POST /api/v1/remote-artifacts (新增 handler，专用 token 门控)
  4. 校验 token → 大小上限 → 解析 bundle 头 → 逐文件落到卷 releases/.incoming-<ver>（路径穿越防护）
  5. 校验两个 app 目录都含 index.html（否则 400，不半换）
  6. rename .incoming-<ver> → releases/<ver>；原子换 desktop-app / mobile-app 两条 current 指针
  7. 清理超出保留数（N=3）的旧 release；返回 {version, activatedAt, kept:[...]}
                         │
                         ▼
[持久卷] /data/remote-apps（Dokku storage mount）
  releases/<ver>/{desktop-app,mobile-app}/…
  desktop-app -> releases/<active>/desktop-app   (symlink)
  mobile-app  -> releases/<active>/mobile-app
  env: DESKTOP_APP_DIR=/data/remote-apps/desktop-app  MOBILE_APP_DIR=/data/remote-apps/mobile-app
                         │
                         ▼
[ridge-cloud] static_host::spa_handler 每请求经 symlink 读到 <active> 版本（零改动）
```

三块的职责与接口：
- **发布器（wind）**：纯构建+打包+上传，不依赖 ridge-cloud 源码。接口=一个 tar.gz + Bearer。
- **上传端点（ridge-cloud）**：纯「收包→校验→原子上架」，不懂 Remote 内部结构（只认「两个含 index.html 的目录」）。接口=HTTP 契约（§5）。
- **持久卷 + symlink**：serve 的唯一真实来源；`static_host` 透明经 symlink 读，无需改。

## 4. 卷布局 & 原子换手

```
/data/remote-apps/
├── releases/
│   ├── 0.0.15+g7caff68/            ← 一次发布一个版本目录
│   │   ├── manifest.json           (version, gitSha, builtAt)
│   │   ├── desktop-app/  (index.html + _app/immutable/… + sw.js + manifest)
│   │   └── mobile-app/
│   ├── 0.0.16+gabc1234/
│   └── .incoming-<ver>/            ← 解压中临时目录（校验通过前）
├── desktop-app -> releases/0.0.16+gabc1234/desktop-app   (symlink)
└── mobile-app  -> releases/0.0.16+gabc1234/mobile-app
```

**原子性（`activate` 抽象，平台分叉）**：写到 `.incoming-<ver>` → 校验全过 → `fs::rename` 成 `releases/<ver>` → 换 `desktop-app`/`mobile-app` 两个 current 指针：
- **unix（生产 = Linux/Dokku）**：`current` 是 symlink；「建临时 symlink + `rename` 覆盖」，POSIX `rename` 原子、零窗口。
- **非 unix（Windows 本地开发/测试）**：symlink_dir 需特权 → 回退「`rename` 现目录到 `.prev` + `rename` release 目录到 current」（两次 rename 间有秒级窗口，`spa_handler` 缺目录时优雅 503），保证 Windows 也能编译+跑通 dev/e2e。

任一步失败**不动现有 current**，旧版继续服务。两个 app 顺序换，极短窗口内两端可能版本不一致——两端相互独立、无跨端引用，可接受。涉及 symlink 语义的单测 `#[cfg(unix)]`（本机 Windows 跳过，CI/生产 Linux 覆盖）；bundle 解析/穿越防护/token 比较/prune 等纯逻辑跨平台全测。

**版本命名**：`<pkgVersion>+g<shortSha>`（如 `0.0.15+g7caff68`），同 sha 重发覆盖同名目录。保留最近 `N=3` 个 release，多余按 mtime 清理。

**回滚**：把两条 symlink 指回上一个保留的 release。提供 `POST /api/v1/remote-artifacts/rollback`（Bearer）与 `pnpm publish:remote-cloud --rollback`。

**换手窗口取舍（诚实说明）**：换 symlink 的秒级窗口内，已加载**旧** `index.html` 的首屏用户若随后请求旧指纹 chunk，会因新 release 无该文件名而 404。相较现状「整机重部署有停机」反而更轻；保留旧 release 目录本身不消除该窗口（symlink 已指新版），但窗口仅秒级且并发首屏极少，可接受。返回用户因 `index.html` 为 `no-cache`、下次进入即取新壳。

## 5. 上传端点契约

### 5.1 Bundle 线格式（自描述、零依赖）

```
[u32 BE header_len][header_len 字节 UTF-8 JSON 头][文件体按序拼接]
JSON 头 = {
  "manifest": { "version": "0.0.15", "gitSha": "7caff68", "builtAt": "2026-07-11T…Z" },
  "files":    [ { "path": "desktop-app/index.html", "size": 1234 }, … ]   // 相对路径，正斜杠
}
```
- 两端零新依赖：wind 用 Node `Buffer` 拼；ridge-cloud 用 `serde_json`（已有）解头 + 按 `size` 顺序切 body。
- 不压缩（SPA 资源多为已压缩二进制；发布低频，HTTPS 传几 MB 无碍），省掉 tar/flate2 依赖与 gzip 手写。

### `POST /api/v1/remote-artifacts`
- **鉴权**：`Authorization: Bearer <RIDGE_ARTIFACT_TOKEN>`（新增配置项，env `RIDGE_ARTIFACT_TOKEN`；缺省未配 → 端点整体 503 关闭，避免裸奔）。用 `subtle::ConstantTimeEq` **恒等时间比较** token。
- **Body**：`application/octet-stream`，§5.1 bundle 原始字节。
- **Body 上限**：单独放大到 `REMOTE_ARTIFACT_MAX_BYTES`（缺省 64 MiB；经 `DefaultBodyLimit` 覆盖该路由，不动全局 256 KiB）。
- **校验顺序**：token → 大小 → 头可解析且 `files[].size` 之和 + `4 + header_len` == body 总长 → 每个 `path` 穿越防护（拒 `..`/绝对路径/反斜杠，复用 `sanitize_path` 同源逻辑）→ 逐文件写入 `.incoming-<ver>` → `desktop-app/index.html` 与 `mobile-app/index.html` 均存在（否则 `400`，不换手）。
- **响应**：`200 {ok:true, data:{version, activatedAt, kept:[...]}}`；`401`（token 错）、`400`（结构非法）、`413`（超限）、`503`（未配 token / 卷不可写）。
- **挂载**：独立小 router 段，**不进** `/api/v1/admin`（那是 AdminAuth JWT 组），而是 `POST /api/v1/remote-artifacts` + 该路由 `DefaultBodyLimit` 覆盖 + 严格限流（`rate_limit_general` 已够；发布低频）。
- **审计**：`tracing::info!` 记 version/gitSha/来源 IP/耗时；`warn!` 记拒绝原因。

### `POST /api/v1/remote-artifacts/rollback`
- 同鉴权。Body 可空或 `{to:<version>}`（缺省=上一个 release）。把 symlink 指回目标 release，返回 `{version, activatedAt}`；目标不存在 → `404`。

## 6. wind 侧发布器 `scripts/publish-remote-cloud.mjs`

`package.json` 加 `"publish:remote-cloud": "node scripts/publish-remote-cloud.mjs"`（与既有 `sync:cloud-controller` 同风格）。

- **入参**（env / flag）：`RIDGE_CLOUD_ARTIFACT_URL`（如 `https://9527127.xyz/api/v1/remote-artifacts`）、`RIDGE_ARTIFACT_TOKEN`；flag `--no-build`（跳过构建用现有产物）、`--dry-run`（只打包不上传，落 `build/remote-artifact-<ver>.tar.gz`）、`--rollback [ver]`。
- **流程**：
  1. （除 `--no-build`）`pnpm build:desktop-web && pnpm build:remote`。
  2. 校验 `web-remote-dist/index.html` 与 `static/remote/index.html` 存在。
  3. 生成 `manifest.json`：`{version: package.json.version, gitSha: <git rev-parse --short HEAD>, builtAt: <ISO>}`。
  4. 遍历 `web-remote-dist/`（→`desktop-app/…`）与 `static/remote/`（→`mobile-app/…`）收集文件列表，按 §5.1 拼 bundle（`u32 头长 + JSON 头 + 文件体`），纯 Node `Buffer`，无新依赖。
  5. `fetch` POST（`--dry-run` 则落盘 `build/remote-artifact-<ver>.bundle` 并打印路径），打印服务端返回的 version/kept。
- **纯函数抽出**便于单测：`buildManifest()`、`collectFiles(dir, prefix)`、`packBundle(manifest, files)`、`resolveConfig(env,args)`。

## 7. ridge-cloud 一次性改动（本方案落地的**最后一次**"带 remote"的部署）

1. **新增** `src/api/remote_artifacts.rs`：`upload` / `rollback` handler + 纯函数（token 恒等比较、tar 安全解压、原子 symlink 换手、release 清理、版本解析）。
2. **router.rs**：装配 `POST /api/v1/remote-artifacts` + `/rollback`（独立 body limit 段）。
3. **config.rs**：加 `remote_artifact_token: Option<String>`（env `RIDGE_ARTIFACT_TOKEN`）、`remote_artifacts_root: String`（env `REMOTE_ARTIFACTS_ROOT`，缺省 `/data/remote-apps`）、`remote_artifact_max_bytes`（缺省 64 MiB）。`desktop_app_dir`/`mobile_app_dir` 无需改（部署时用 env 指到卷 symlink）。
4. **Dockerfile**：删 `COPY desktop-app` / `COPY mobile-app`（`Dockerfile:44-55` 区域）。`web/build`、`admin-app/build` 保留（云端自有，不在本方案范围）。
5. **仓库清理**：`git rm -r desktop-app mobile-app`（移除检入的 218+28 文件），`.gitignore` 加 `desktop-app/`、`mobile-app/`。
6. **依赖**：**零新增**。bundle 解析用已有 `serde_json`；token 恒等比较用已有 `subtle`；文件 IO 用 `tokio::fs`。不引 tar/flate2。

## 8. 割接 / 运维手册（用户执行，需主机权限）

> 这些是**一次性 ops 动作**，我准备好代码后交付给你按序执行（我无法代跑 dokku/SSH）。

1. 在 Dokku 主机：`dokku storage:ensure-directory ridge-cloud-remote-apps`（或手建 `/var/lib/dokku/data/storage/ridge-cloud/remote-apps`）。
2. `dokku storage:mount ridge-cloud /var/lib/dokku/data/storage/ridge-cloud/remote-apps:/data/remote-apps`。
3. `dokku config:set ridge-cloud RIDGE_ARTIFACT_TOKEN=<强随机> DESKTOP_APP_DIR=/data/remote-apps/desktop-app MOBILE_APP_DIR=/data/remote-apps/mobile-app`。
4. 部署本方案版 ridge-cloud（`git push dokku main`）。此刻卷空 → `spa_handler` 短暂 503 降级（合理）。
5. 在 wind：`RIDGE_CLOUD_ARTIFACT_URL=… RIDGE_ARTIFACT_TOKEN=… pnpm publish:remote-cloud`，填充当前 0.0.15 产物。校验租户域桌面 SPA 与手机 SPA 下发新版。
6. 自此更新 Remote 只需第 5 步一条命令，**永不再** `git push dokku main`。

## 9. 失败处理 / 安全

- **原子上架**：解压+校验全过才换 symlink；失败留 `.incoming-<ver>` 供排查（下次同版覆盖），旧版持续服务。
- **穿越防护**：tar entry 逐个 `sanitize`，拒 `..`/绝对路径/symlink entry。
- **鉴权**：token 恒等时间比较；未配 token → 端点 503（不裸奔）；TLS 已全局强制。
- **卷不可写**：写失败 → 500/503 + `error!`，不影响 serve。
- **DoS**：body 上限 + 发布低频 + 现有限流。

## 10. 测试

- **ridge-cloud（`cargo test`，独立 target 可自跑）**：
  - `verify_token`：恒等比较、未配 token 拒绝。
  - `safe_extract`：拒 `../`/绝对路径 entry；正常 entry 落对位置。
  - `activate`：临时目录建 `.incoming` → 换 symlink → 断言 `spa_handler` 读到新版 `index.html`；缺 `index.html` 时**不换**、旧版仍在。
  - `rollback`：换到上一个 release；目标缺失 404。
  - `prune`：保留 N、清理旧。
- **wind 发布器（vitest）**：`buildManifest`/`resolveConfig`/tar 打包纯函数单测；`--dry-run` 产出 tar.gz 结构正确（顶层 manifest + 两目录）。
- **e2e（可选，手动）**：本地起 ridge-cloud + 临时卷目录 → `pnpm publish:remote-cloud` 打到 localhost → curl 租户域断言新版。

## 11. YAGNI（明确不做）

- 不引对象存储 / CDN（单机 Dokku 够用）。
- 不做多环境 / 灰度 / 蓝绿（单生产）。
- 不做发布管理 Web UI（命令行足矣）。
- 不动 `web/build`、`admin-app/build`（云端自有产物，非本方案范围）。
- ua SSOT 发 git crate（task#4）是独立小任务，不并入本稿。

## 12. 交付边界

- **本会话可完成（代码）**：ridge-cloud upload/rollback handler + config + router + Dockerfile + 仓库清理 + 测试；wind 发布器 + 测试。分仓分 commit。
- **交付用户执行（ops）**：§8 割接手册（dokku mount / config:set / 部署 / 首次 publish）。这些需主机权限，我出手册不代跑。

## 13. 任务映射

对应 backlog `#2`（本稿）。相关但独立：`#4`（ua git crate）、`#5`（P4 部署）、`#6`（coturn）——不并入。
