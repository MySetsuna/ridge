# Project Guidelines

## Communication
- Think in English, respond in Simplified Chinese. Code comments follow the existing language style of each repo.

## Workflow（Ponytail 建造精简法）
- 不强制阶段化流程或设计 spec 文档；重大改动以一句 rationale + 聚焦提交说明代替。
- 建造走 **Ponytail**：改前先读懂；沿「先谋删 → YAGNI → 复用本仓 → 标准库 → 平台原生 → 已装依赖 → 一行 → 最小可跑」止于首个成立档；每行自问「不写何害」，删优于增；根因修（grep 全调用方，共有函数一处修好）；`diff>50` 行先一句陈由；非平凡逻辑留最小自检。
- 不写未请求的抽象，不做多分支/大爆炸式重构；**不省**：信任边界校验、防丢数据错误处理、安全、无障碍、用户明确要求。

## Collaboration Rules
- Follow the user's instructions precisely, and within that scope act autonomously: gather the necessary context and complete the requested work end-to-end in this run, asking questions only when essential information is missing or the instructions are critically ambiguous.
- For hard-to-reverse or outward-facing actions (deletes, pushes, publishing), confirm the scope first unless explicitly authorized.
- One feature/concern per commit; keep commits focused.
- **Solo default: work on `main`.** Do not open long-lived feature branches unless the user asks for isolation. No multi-branch/PR theater for single-developer flow.

## External processes & concurrency（可复用教训，2026-07-24 git.exe 风暴）

详述与检查清单见 `docs/iterations/2026-07-24-git-pileup-postmortem.md`。写/改任何会 `spawn` 外部二进制（git、helper、CLI…）的路径时遵守：

1. **逻辑并发 ≠ OS 进程生命周期**  
   Semaphore / 线程池 / `pLimit` 只约束「同时进入某段代码的任务数」。已 spawn 的子进程必须另有：**墙钟超时、取消、进程树杀掉、permit/引用与进程同生死**。验收不得只 assert「permit ≤ N」——要有超时后活跃计数归零或峰值进程信号。

2. **AbortSignal / drop future 不杀子进程**  
   前端 abort 或 async cancel 默认只停调度。产品写「取消刷新」时，实现必须落到 **kill 进程树**，否则用户看到取消后仍占 CPU。

3. **「已实现」以运行事实为准**  
   对账表/文档标关闭须同时有：（1）代码符号（2）确定性测（3）与现场故障模式**同构**的断言。仅有 debouncer/限流而无超时杀树，只算部分实现。

4. **应急顺序：先停派发方，再清子进程**  
   在「有界队列 + 无超时」系统里，只杀子进程会**喂养队列**（permit 释放 → 重生风暴）。先停宿主/停 fan-out，或依赖应用内超时自回收。

5. **双端 cap 同具名常量**  
   前后端并发上限禁止各写魔法数；共享常量（如 `GIT_CONCURRENCY_MIN/MAX`）+ 交叉注释；能测则契约测。

6. **Windows 按进程树设计**  
   设计清单含：`CREATE_NO_WINDOW`、Job Object / `taskkill /T`、权限失败路径。单测优先**真挂起假二进制**走超时杀，禁止 mock 掉唯一出口却宣称护栏绿。

7. **多触发同源共闸**  
   watcher / heartbeat / pane / SCM 等必须汇入**同一**外部进程出口。新入口直 `Command::new("git")`（或等价）绕过闸 = 护栏失效。

8. **修可快，空 Release 不可**  
   硬护栏 + 确定性测可先合 main；versioned Release 仍遵守下方 Releases 硬规矩（等矩阵资产，禁止空 tag）。

**同类子系统检查清单（提交前扫一眼）**

- [ ] 外部进程是否经唯一出口？
- [ ] 墙钟超时 + 杀进程树？
- [ ] 取消/超时是否释放许可与计数？
- [ ] 前端 abort 与后端回收语义一致？
- [ ] 前后端并发 cap 同常量？
- [ ] 测覆盖：并发峰值、挂起超时回收、真二进制冒烟？

## Releases（硬规矩）

### 发版前工作区洁净闸

若工作区仍有可复用代码修改（未提交或未追踪），不得直接发版。须先判定其为真实逻辑、已废弃内容或生成物：真实逻辑落地测试后提交并推送；生成物/运行态加入明确忽略规则或清理；废弃内容安全移除。发版前必须同时满足 `git diff --exit-code`、`git diff --cached --exit-code` 与 `git ls-files --others --exclude-standard` 均无输出，且 `HEAD` 已推送至目标远端。版本化 Release、Remote artifact、Cloud 发布均受此闸约束。

发 **versioned GitHub Release**（`vX.Y.Z`）时，**必须**带上与该版本号一致的安装包/CLI 产物，形态对齐历史 Release（如 `v0.0.16` / `v0.0.17`），**禁止**只建空 tag + 文字说明、无 asset。

### 发版频率与合并窗口

- 需求与修复须先汇总为一个可验证批次，再发 versioned Release、Remote artifact
  或 Cloud artifact；不得为单个零散修复连续发版。
- 任一自然日最多 3 次发布事件（versioned Release、Remote artifact、Cloud
  artifact 依同一计数）；失败重试、补资产与重跑 workflow 不得借机开启新版本。
- 本次 `v0.1.53` 已占用当前发布窗口；本日后续禁止再次发版或重复发布。若需
  新需求，继续落地、测试、提交、推送并归档，待下一发布窗口合并后一次发布。
- 每次发布前仍须通过工作区洁净闸、版本/资产一致性闸与全量回归；未满足则只修复
  代码或记录阻塞，不以降级/空 Release 绕过。

### 期望产物（与 `.github/workflows/release.yml` 矩阵一致）

| 平台 | 资产名示例（版本号随 release） |
| --- | --- |
| Windows | `ridge_X.Y.Z_x64-setup.exe`、`ridge_X.Y.Z_x64_en-US.msi`、`rdg-X.Y.Z-x86_64-windows.exe` |
| Linux | `ridge_X.Y.Z_amd64.deb`、`ridge_X.Y.Z_amd64.AppImage`、`rdg-X.Y.Z-x86_64-linux` |
| macOS | `ridge_X.Y.Z_aarch64.dmg`、`ridge_X.Y.Z_x64.dmg`（及 app tar 若 CI 产出）、`rdg-X.Y.Z-aarch64-macos` |

验收：`gh release view vX.Y.Z` 列出上述类资产；缺任一类（整平台失败除外且须在 notes 写明）不得声称「Release 完成」。

### 标准流程

1. 同步 `package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 版本号。
2. 提交后打 **annotated tag** `vX.Y.Z` 并 `git push origin vX.Y.Z`（`on.push.tags: v*` 触发 `release.yml`）。
3. **等** workflow 全部 matrix job 结束；用 `gh run list --workflow release.yml` / `gh run watch` 跟进。
4. 确认 Release 资产齐全后，再改正式说明（`gh release edit`）；CI 默认可能出 **draft**，需人工/命令发布。
5. 若 tag 已推但 workflow 未跑/失败：`gh workflow run release.yml -f tag=vX.Y.Z` 重跑，或本机 `RIDGE_BUNDLES=nsis,msi pnpm tauri:build` 后 `gh release upload vX.Y.Z <files> --clobber`（仅补当前平台；全平台仍以 CI 为准）。
6. **Remote 云 artifact**（`pnpm publish:remote-cloud`）是另一条发布线，与桌面安装包并列；有 token 的机器上传，缺 token 写进交接文档，**不能**用「只发了 GitHub 空 Release」代替安装包。

### 禁止

- 仅 `gh release create` 无构建、无 upload、无 CI 产物即收工。
- 资产版本号与 tag / `package.json` 不一致（例如 tag `v0.0.18` 却挂 `0.0.17` 安装包）。

参考：`.github/workflows/release.yml`、`docs/release-signing.md`、历史 Release 资产列表。

## Code Intelligence
- See `.Codex/AGENTS.md` for the CodeGraph MCP usage guide — prefer it for structural code questions (callers/callees/impact/definitions) over grep.
