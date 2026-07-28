# Ridge 发布 Runbook

## 目录

1. [默认拓扑](#默认拓扑)
2. [预检与同步](#预检与同步)
3. [Windows release 构建](#windows-release-构建)
4. [Dokku 发布 ridge-cloud](#dokku-发布-ridge-cloud)
5. [分离式 remote 产物](#分离式-remote-产物)
6. [故障恢复索引](#故障恢复索引)
7. [最终验收](#最终验收)

## 默认拓扑

先发现实际配置；只有与现场一致时才使用这些默认值。

| 项目 | 默认值 |
|---|---|
| 桌面仓库 | `C:\code\wind`，分支 `main` |
| 云端仓库 | `C:\code\ridge-cloud`，分支 `main` |
| 云部署工作流 | `MySetsuna/ridge-cloud` 的 `deploy-dokku.yml` |
| Dokku 应用 | `ridge-cloud` |
| 健康接口 | `https://9527127.xyz/api/v1/health` |
| artifact API | `https://9527127.xyz/api/v1/remote-artifacts` |
| Rust 工具链 | `C:\DevKit\Rust\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin` |
| Cargo 缓存 | `C:\DevKit\Rust\.cargo` |
| 生产 appuser | UID/GID `10001:10001` |

不要保存历史 SHA、artifact token 或 storage id；每次现场读取。

## 预检与同步

分别在两个仓库执行：

```powershell
git status --short --branch
git branch --show-current
git rev-parse HEAD
git remote -v
```

记录既有脏文件。它们属于用户，不执行批量清理、stash、reset 或 checkout。然后：

```powershell
git fetch origin
git pull --ff-only origin main
```

若 pull 报告“未跟踪文件将被覆盖”，先确认精确文件，再用带时间戳的相邻备份名保留，例如：

```powershell
Move-Item -LiteralPath 'AGENTS.md' -Destination 'AGENTS.md.pre-sync-20260720'
git pull --ff-only origin main
```

不要覆盖已有备份名。同步后核对原有脏文件和备份仍存在。

## Windows release 构建

### 绑定确定的工具链

```powershell
$ridgeToolchainBin = 'C:\DevKit\Rust\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin'
$env:Path = "$ridgeToolchainBin;$env:Path"
$env:CARGO = "$ridgeToolchainBin\cargo.exe"
$env:RUSTC = "$ridgeToolchainBin\rustc.exe"
$env:CARGO_HOME = 'C:\DevKit\Rust\.cargo'
$env:CI = 'true'

& $env:RUSTC --version
& $env:CARGO --version
& $env:CARGO metadata --offline --locked --manifest-path 'packages/ridge-term/Cargo.toml'
```

只有 metadata 离线成功后才设置 `$env:CARGO_NET_OFFLINE = 'true'`。失败时先检查 `CARGO_HOME` 是否误落到空的用户缓存，不要立即重装 Rust 或切换镜像。

### 正常构建

```powershell
pnpm install --frozen-lockfile
$env:CARGO_NET_OFFLINE = 'true'
pnpm tauri:build
```

无 TTY 时必须保留 `CI=true`。`tauri:build` 会运行完整前端、Rust 和 bundle 链路。

### 内存压力恢复

若并行 release 在 Vite/Rust 高峰后以 rustc exit code 1 退出且没有编译诊断，先检查内存和残留进程。使用单线程完成主 Rust 产物：

```powershell
& $env:CARGO build --release --manifest-path 'src-tauri/Cargo.toml' -j 1
```

若该命令成功，不再重复完整 `pnpm tauri:build`，直接打 NSIS bundle 并重命名：

```powershell
.\node_modules\.bin\tauri.cmd bundle --bundles nsis --ci
node scripts/post-build-rename.mjs
```

`tauri bundle` 仍会执行 `beforeBundleCommand`，构建 `tmux` 与 `ridge-cli`。a11y 或 dead-code warning 不等于失败，以退出码和产物为准。

### 产物检查

```powershell
$ridgeInstaller = Get-ChildItem -LiteralPath 'release' -Filter 'ridge_*_x64-setup.exe' |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
$ridgeInstaller | Select-Object FullName,Length,LastWriteTime
Get-FileHash -LiteralPath $ridgeInstaller.FullName -Algorithm SHA256
git status --short
```

若 `src-tauri/Cargo.toml` 显示修改但 `git diff -- src-tauri/Cargo.toml` 为空，通常是换行归一化。先确认内容 diff 为空，再执行：

```powershell
git add --renormalize -- src-tauri/Cargo.toml
git diff --cached --quiet
git status --short
```

若产生 staged diff，停止并检查，不要提交或重置。

## 镜像发布 ridge-cloud

在 `C:\code\ridge-cloud`：

```powershell
git fetch origin
git pull --ff-only origin main
$ridgeCloudTarget = git rev-parse main
gh workflow run deploy-dokku.yml --repo MySetsuna/ridge-cloud --ref main -f "ref=$ridgeCloudTarget"
```

工作流在 GitHub runner 用 BuildKit/GHA cache 构建 `linux/amd64` 运行时镜像，冒烟后由
`docker image save | ssh ... git:load-image` 直传 Dokku。生产机只构建一层 `FROM`，
不再重编 Rust。镜像 `/app/CHECKS` 必须含 `/api/v1/health`，否则镜像部署会退化为
仅检查进程存活。

```powershell
$ridgeCloudRun = gh run list --repo MySetsuna/ridge-cloud --workflow deploy-dokku.yml `
  --event workflow_dispatch --limit 1 --json databaseId,headSha,status,url |
  ConvertFrom-Json
if ($ridgeCloudRun.headSha -ne $ridgeCloudTarget) { throw '部署 run 源 SHA 不符' }
gh run watch $ridgeCloudRun.databaseId --repo MySetsuna/ridge-cloud --exit-status
```

首次运行须预热 Rust 构建层，仍可能较久；后续应主要复用依赖层。若镜像直传步骤超过
15 分钟或整 job 超过 45 分钟，工作流自动终止，勿无限等待。

### 中断与 deploy lock

取消 Action 后若 `git:load-image` 已进入 Dokku，重试可能返回 deploy lock：

1. 检查 Action、Dokku build record 与应用进程，确认生产当前版本。
2. 确认锁是残留锁，不是另一个仍在运行的发布。
3. 获授权后执行：

```powershell
ssh dokku@oracle apps:unlock ridge-cloud
```

4. 重新触发精确 SHA 的工作流。

### 云端验收

```powershell
Invoke-WebRequest -UseBasicParsing 'https://9527127.xyz/api/v1/health' |
  Select-Object StatusCode,Content
```

Action 的 `Resolve source revision` 与镜像
`org.opencontainers.image.revision` 必须等于 `$ridgeCloudTarget`；健康接口必须返回 HTTP 200。

## 分离式 remote 产物

artifact API 必须先随 ridge-cloud 成功上线。安全读取 token，不把赋值表达式单独回显：

```powershell
$ridgeArtifactToken = (& ssh dokku@oracle config:get ridge-cloud RIDGE_ARTIFACT_TOKEN).Trim()
if ([string]::IsNullOrWhiteSpace($ridgeArtifactToken)) { throw 'RIDGE_ARTIFACT_TOKEN 为空' }
$env:RIDGE_ARTIFACT_TOKEN = $ridgeArtifactToken
$env:RIDGE_CLOUD_ARTIFACT_URL = 'https://9527127.xyz/api/v1/remote-artifacts'
$env:CI = 'true'
pnpm publish:remote-cloud
```

不要使用 `pnpm publish:remote-cloud -- --no-build`；正确重试语法是：

```powershell
pnpm publish:remote-cloud --no-build
```

### HTTP 413

后端支持大包不代表 Nginx 已放行。先只读检查：

```powershell
ssh dokku@oracle nginx:report ridge-cloud
```

确认确为默认 1 MiB 限制并获得授权后：

```powershell
ssh dokku@oracle nginx:set ridge-cloud client-max-body-size 64m
ssh dokku@oracle proxy:build-config ridge-cloud
ssh dokku@oracle nginx:report ridge-cloud
```

只执行 `nginx:set` 不会让当前代理配置立即生效；必须重建配置后再用 `--no-build` 上传。

### HTTP 500“产物落盘失败”

检查应用日志和存储映射：

```powershell
ssh dokku@oracle logs ridge-cloud --num 100
ssh dokku@oracle storage:report ridge-cloud
```

若日志为 `Permission denied (os error 13)`，从 storage report 获取实际 storage id。确认运行用户仍为 `10001:10001` 并获得授权后：

```powershell
$ridgeStorageId = '从 storage:report 复制的实际 ID'
ssh dokku@oracle storage:exec $ridgeStorageId -- chown -R 10001:10001 /data
pnpm publish:remote-cloud --no-build
```

`storage:exec` 中卷挂载点是 `/data`，而生产容器使用 `/data/remote-apps`。生产绝对符号链接在临时容器里可能看似断裂；验证实体目录：

```powershell
$ridgeRemoteVersion = '上传响应中的激活版本'
ssh dokku@oracle storage:exec $ridgeStorageId -- find "/data/releases/$ridgeRemoteVersion" -maxdepth 2 -type f -name index.html -print
```

### 清除秘密

无论成功失败，结束当前上传尝试后执行：

```powershell
Remove-Item Env:RIDGE_ARTIFACT_TOKEN -ErrorAction SilentlyContinue
$ridgeArtifactToken = $null
```

## 故障恢复索引

| 症状 | 优先根因 | 恢复 |
|---|---|---|
| pull 被未跟踪文件阻止 | 上游新增同名 tracked 文件 | 精确备份本地文件，再 `pull --ff-only` |
| Rustup 卡住或 manifest 缺失 | 调错工具链/用户缓存为空 | 显式设置 DevKit `CARGO`、`RUSTC`、`CARGO_HOME` |
| cargo metadata 卡住 | 尝试访问网络或错误缓存 | 验证 DevKit 缓存，使用 `--offline --locked` |
| rustc exit 1 且无诊断 | 构建内存压力 | 清点残留进程，`cargo build ... -j 1` |
| CI 镜像构建仍慢 | GHA cache 首次预热或层失效 | 查 Buildx cache 命中；勿回退生产机源码构建 |
| 镜像直传超时 | SSH 中断或运行时镜像异常膨胀 | 查 Action 首因、镜像大小与 Dokku build record |
| Dokku deploy lock | 中断留下残留锁 | 先确认无活跃部署，授权后 `apps:unlock` |
| 上传 HTTP 413 | Nginx 1 MiB 限制 | `nginx:set 64m` 后 `proxy:build-config` |
| 上传 HTTP 500 / Permission denied | 卷属主不匹配 appuser | 授权后把实际 storage 卷 chown 为 `10001:10001` |
| Node 在失败上传后出现 libuv assertion | 上游 HTTP 失败后的次生退出 | 先修后端 413/500，再 `--no-build` 重试 |

## 最终验收

完成报告必须包含：

- `wind` 的分支与发布 SHA。
- 安装包绝对路径、字节数、SHA-256。
- `ridge-cloud` 预期 SHA、Action run ID 与镜像 revision。
- 健康接口 HTTP 状态和版本。
- remote 上传响应中的激活版本。
- 持久卷中 `releases/<version>/desktop-app/index.html` 与 `mobile-app/index.html` 的存在性。
- 两个仓库最终 `git status --short --branch`，并明确哪些是发布前就存在的脏文件。
- 是否发生过取消、解锁、Nginx 修改或 chown，以及对应授权。
