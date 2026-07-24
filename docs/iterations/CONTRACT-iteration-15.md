# CONTRACT — Iteration 15：开放愿景清单闭合（大迭代）

日期：2026-07-24  
前置：Skill 纠偏（Notes 清空≡愿景全实现）；开放清单 11 项 open  
规则：用户侧判断由执行者经 NLM 深研意图 + 对抗评审后**自行拍板**（本文件即拍板）

## 锁定决策（拍板）

| 主题 | 拍板 |
| --- | --- |
| V-G1-RB 回滚语义 | **git worktree 补丁快照**：`checkpoint` = 对 workspace root 跑 `git diff`+`git status --porcelain` 写入 sidecar `rollbackPatches[]`；`rollback` = 将列出路径恢复到快照时 blob（`git checkout HEAD -- path` 对已跟踪 + 删除未跟踪新增）。无 git 仓库 → 明确 Err，不静默。**不做**全盘文件系统快照。 |
| V-G1-OS | 阶段二：**Unix SIGSTOP/CONT 进程组**；**Windows NtSuspendProcess 进程树遍历**（设计候选 A）。Job Object 预建（阶段三）本轮不做。失败回滚已挂起线程。 |
| V-H1 | **最小 live 闭环**：`connect_host` 对 `addr` 做可达探测（TCP 或 HTTPS HEAD）；成功 → status=Connected + 写入探测会话元数据；失败 → Error+detail。**完整出站 PTY 字节流**若本轮时间不够，至少：`RemoteRef` 路由钩 + 集成测证明「Connected 后 attach 命令面存在且未 Connected 拒绝」。优先复用已有 LAN controller 客户端若可无新协议。 |
| V-M1-S3 | goal:string、constraints:string[]、tasks:{id,title,status}[] 读写 API + Agent Center 最小编辑区；无 Remote 协议面。 |
| V-DISC | 可选开关默认关；扫描常见 agent CLI 进程名（claude/codex/cursor-agent 等）映射为侧栏候选；关时零扫描。 |
| V-B3 | 复用 `fs_watch`：打开的图片路径订阅 → 变更 emit → 预览 store 版本号++。 |
| V-MOB-CP | 复制手势：`copy` 路径只 `clipboard.writeText` + 清选区，**不** focus 隐藏 textarea / 不调 `paste`。单测钉死。 |
| V-B6A | 缺 UI 产物时 HTTP body + 可选 `X-Ridge-Error: REMOTE_UI_MISSING`；CLI/库函数 `remote_ui_missing_message()` 可单测。 |
| V-B6B | resize 已透传则补 **grid 尺寸==请求** 回归测；若缺 PTY resize 调用补全。 |
| V-PASTE | `encodePaste`/`wrap_paste` 多行顺序单测（100 行带行号）。 |
| V-TUI-CLK | **核查**：`encode_mouse`+`manager` 鼠标报告已存在 → 跑既有测，标 implemented，不重造。 |

## 目标与验收

| ID | 验收 |
| --- | --- |
| V-TUI-CLK | `cargo test -p ridge-term` mouse/encode 相关绿 |
| V-PASTE | 新增/既有测：100 行 paste 字节序与源一致 exit 0 |
| V-B6A | 单测断言错误码/指引串 |
| V-MOB-CP | vitest：复制路径不触发 focus/paste |
| V-B6B | resize 尺寸一致性测绿 |
| V-M1-S3 | memory 字段 RMW 单测 + 命令注册 |
| V-G1-RB | checkpoint/rollback 单测（临时 git repo） |
| V-G1-OS | suspend 调 OS 路径有 cfg 测或 mock 守卫；soft 路径仍绿 |
| V-DISC | 探测模块单测（假进程表） |
| V-B3 | watch→版本号 测或命令测 |
| V-H1 | connect 状态机测：可达/不可达；未连接 attach 拒绝 |

## 门禁

- `cargo test --workspace` exit 0（或至少 `-p ridge --lib` + 改动 crate）
- vitest 相关包绿
- 更新开放清单 note 行状态；PROJECT-STATE；LOG

## 不做

- 生产 Dokku 实部署、真机手持 smoke、分支 merge
- Job Object 预建（G1 阶段三）
- 扩 E2EE / 新协议 SSOT / auto-merge

## 停机

任一门禁红 → 修到绿；不可伪标 implemented。
