---
name: release-ridge
description: 同步 Ridge 相关仓库并完成 Windows release 构建、Dokku ridge-cloud 发布、分离式 desktop/mobile remote 产物上传与生产验收。用户要求“发布 Ridge”“构建 release”“部署 ridge-cloud”“发布 remote 产物”“清理 Dokku 发布锁”“处理 Ridge 强推”或继续/恢复上述发布流程时使用。
---

# 发布 Ridge

把发布当成五个有门禁的阶段：同步、桌面构建、ridge-cloud 部署、remote 产物发布、生产验收。开始前完整阅读 [references/ridge-release-runbook.md](references/ridge-release-runbook.md)。

## 执行纪律

1. 先记录 `wind` 与 `ridge-cloud` 的分支、HEAD、远端和脏文件；保留所有既有修改与未跟踪文件。
2. 按阶段执行。上一个阶段未验收，不进入下一个阶段。
3. 把普通发布授权限制在拉取、构建、常规 Dokku 推送和产物上传。直接强推、解锁、修改 Nginx、修改持久卷属主仍需明确授权；用户已在当前请求授权时不重复询问。
4. 不打印、写盘或提交 `RIDGE_ARTIFACT_TOKEN`。只在当前进程环境中短暂设置，用完立即清除。
5. 长时间无新日志不等于失败。Windows release 和 Dokku 冷构建都可能持续数十分钟；检查进程、远端状态和 deploy lock 后再决定是否中断。
6. 任何失败先保留原始症状，再按 runbook 的“症状 → 根因 → 恢复”处理；不要用 `git reset --hard`、删除缓存或直接强推掩盖问题。

## 阶段门禁

### 1. 同步

- 对两个仓库执行只读预检。
- 处理会被上游覆盖的未跟踪文件时，先复制/改名保留，再 `pull --ff-only`。
- 验收：本地目标分支与 `origin` 一致，原有用户改动仍在。

### 2. 构建 release

- 使用锁文件安装依赖，并显式绑定 `C:\DevKit\Rust` 工具链和 Cargo 缓存。
- 先验证 `rustc`、`cargo metadata --offline --locked`，再构建。
- 并行 Rust release 出现无诊断 exit code 1 或内存压力时改用 `-j 1`；Rust 主产物已成功时直接 bundle，避免重复全量前端构建。
- 验收：`release/ridge_<version>_x64-setup.exe` 存在，记录大小与 SHA-256；构建未留下意外 tracked diff。

### 3. 发布 ridge-cloud

- 抓取 Dokku `main`，比较它与 `develop` 的左右提交和预期目标 SHA。
- 优先普通推送；确需覆盖远端时先用绑定已观察 SHA 的 `--force-with-lease`。只有当前请求明确授权才使用 `--force`。
- 不因冷构建日志缓冲而主动断开。客户端已中断且服务端确认残留锁时，获授权后解锁并重新推送。
- 验收：Dokku `main` 指向预期 SHA，部署健康检查通过，生产健康接口返回 200。

### 4. 发布 remote 产物

- 先确保新 artifact API 已随 ridge-cloud 上线。
- 安全读取 token，构建并上传 desktop/mobile bundle。上传失败而本地产物完好时用 `pnpm publish:remote-cloud --no-build` 重试。
- 413 检查 Nginx 上传上限；500“产物落盘失败”检查持久卷属主。修复配置或属主属于生产变更，遵守授权门禁。
- 验收：响应给出激活版本，持久卷存在该版本的 desktop/mobile 实体目录。

### 5. 收尾

- 清除秘密环境变量。
- 再次检查两个仓库状态，不清理或提交无关文件。
- 报告源代码 SHA、ridge-cloud SHA、安装包路径/大小/哈希、remote 激活版本、健康检查结果，以及采取过的恢复动作。
- 只有所有必需验收项都有证据时才宣称“发布完成”。
