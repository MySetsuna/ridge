# Iteration 61 Closeout

日期：2026-07-27
合同：`docs/iterations/CONTRACT-iteration-61.md`
需求：`docs/REQUIREMENTS-SPEC.md` v0.2.0

## 交付

- `rdg` LAN/public Remote 改为显式启停；LAN URL 去 `/login`。
- LAN Web 先判启动模式并直入 LAN boot，免云端 bootstrap 白壳。
- ridge-cloud 配额按实时用户组计算，以 `parked_by_quota` 隔离人工禁用。
- Mobile 弹层 portal 化；Team/Agent 图标与无边框动作收敛。
- Agent Center 跨工作区聚合；显示 Claude/Codex 最近 assistant 回复。
- Agent 进程自动登记/释放；tmux native session 记录 creator workspace/pane，并可从 Agent Center 唤醒。

## 提交

- wind：`367b293`、`0b1985e`、`3bde775`、`f110dd0`、`6b00ddb`
- ridge-cloud：`beb87ea`

## 运行证据

- `pnpm test`：99 files；1,237 passed；1 skipped。
- `pnpm check`：0 errors；2 个既有 a11y warnings。
- `cargo check --manifest-path src-tauri/Cargo.toml --lib`：exit 0。
- `cargo check --manifest-path src-tauri/Cargo.toml --bin tmux`：exit 0。
- `cargo test -p ridge-tmux`：11/11。
- `cargo test -p ridge-cli tui::dashboard::tests`：2/2。
- JSONL fixtures/project filter：3/3。
- ridge-cloud `db::device_quota::tests`：6/6。

## NotebookLM 替换与评审

Notebook：`66919cb9-1329-4ddf-955c-f426d15a9fe6`。

- 旧 `PROJECT-STATE` 来源 `c36fe556-c8ee-48bb-a968-09557cfab133` 已由新来源 `d64e0549-4513-4bc8-8c2b-170a903f4846` 覆盖替换。
- `REQUIREMENTS-SPEC` 来源 `1c450a6b-2ea4-4793-9817-89a5e246efed` 保持不变。
- NotebookLM 首选下一步：不再写代码，执行一次 30 分钟真机/生产证据清偿，覆盖 S1/T3/R1。
- 对抗裁决：保留 `__faultRig.ts` 与 `weakNetLab.test.ts`；一次真机通过不能替代长期确定性故障注入。其“删除独立 weaknet 脚本”减法建议仅留候选，未获运行冗余证据前不实施。

## 下一步与停止条件

唯一下一步属用户轨：按现有 runbook 验证 LAN/public 首屏、移动弹层触控、Agent 退出后 roster 收敛、无头会话唤醒及公网换网恢复。若出现 OOM/渲染断层、F1/F4 异常降级或会话无法回收，停止验收并回到对应故障最小复现；否则不扩代码范围。
