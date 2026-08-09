# Ridge 迭代闭环

本项目的 NotebookLM 循环按以下顺序运行：

1. 读取当前 `CONTRACT-iteration-N.md` 与 `LOG.md` 最近记录。
2. 严格按合同范围实现或校准。
3. 用编译、测试和退出码验收。
4. 写带日期的迭代报告并上传目标 NotebookLM。
5. 获取下一步建议，做来源与代码现实的对抗评审。
6. 归档建议、追加 `LOG.md`、生成下一轮合同。

NotebookLM 是规划 maker；编译器、测试和独立代码审查是 checker。默认授权等级为 Level 2：在独立分支形成可审查提交，不自动合并或发布。

## NLM 认证闸门（2026-08-08 固化）

- 默认代理为 `http://127.0.0.1:51081`，仅注入本次 Chrome/CLI 进程，不改系统代理。
- `nlm login --check` 只作预检；用户明确要求刷新时，即使预检失败也继续外部 CDP 流程。
- 使用专用 Chrome 与 `nlm_auth_flow.py launch`，等待 `/json/version`、`/json/list` 可读并确认
  `cdp_ready=true` 后再登录；`notebook.google.com` 为有效落点。
- 登录后用 `save_external_cdp_auth.py` 的 `wait_for_login=False` 路径抽取认证，不依赖
  `nlm login --cdp-url` 收尾。
- 同代理验证 `login_check_exit=0` 与 `notebook_list_exit=0` 后才进入冷循环；仅记录退出码、数量、
  标题/URL。Cookie、storage、密码、token 永不进入输出、日志、来源或提交。
- 失败分为 `cdp_unavailable`、`google_login_incomplete`、`auth_extract_failed`、
  `cli_network_or_wrapper_failed`、`notebook_api_failed`。

验收采用双轨制：**自动轨**（编译/测试/退出码可判定的目标，合同验收对象）与**用户轨**（真机 smoke、生产实跑、人工核验、分支合并等只能由人完成的事项，见 `docs/plans/user-verification-checklist.md`）分列；自动轨不得以实验室模拟数据冒充用户轨结论。

节律（iteration 10 起固化）：每轮闭环提交前必跑 `node scripts/generate-review-pack.mjs` 刷新 `docs/review/branch-review-guide.md`，使审查导读始终覆盖分支全部领先提交；用户轨积压未消化期间，不启动扩协议面的重型变更。

维护态（iteration 12 起，**仅当开放愿景清单 open=0** 时启用）：一轮维护 = 全门禁绿（cargo/vitest/svelte-check）+ 导读刷新 + 零回归即闭环，不设新功能目标。若 notes / `docs/iterations/*-open-vision-checklist.md` 仍有 `open` 行，**禁止**进入维护态空转。

**Notes 清空 ≡ 愿景全实现**（`notebooklm-iteration-loop` 硬规矩）：未代码落地的愿景不得删 note；可用标题 `[已实现]` 或清单行 `implemented` **视同清理**。禁止把「待用户轨」写成实现后清空 notes。原「待用户裁定」项：先 NLM 深研 → 对抗评审 → **执行者拍板并实现**。

红线不变：无充分理由不扩协议面/新状态源/改 E2EE；真机/生产/合并仍见 `docs/plans/user-verification-checklist.md`（不挡功能代码愿景闭合）。

## 当前迭代交接（2026-08-08）

Remote/pane/workspace 本轮已完成本地修复与 E2E 验证；下一轮 NLM 只读输入为
`docs/iterations/2026-08-08-remote-e2e-nlm-next-iteration.md`、本文件及
`docs/PROJECT-STATE.md`。先对登记的 E2E 异常作根因对抗审查，再生成下一轮
contract/decision；未获用户明确授权，不上传/删除 NLM source，不发布 Remote/Cloud，
不激活生产。

## 本轮交接（2026-08-08）

- NLM 认证失效已用外部 CDP + 固定本地代理修复；`nlm login --check` 与 22 本 notebook list 通过后，才执行主笔记仅读对话抽取。NLM 只产出假设，代码/测试事实优先；worker 不调用 NLM。
- 并行 Ridge/Claude 审查须记录执行事实：本轮 Claude pane 未登录，任务未执行，不得伪称并行审查完成。
- 每轮代码变更后依序运行 CodeGraph 终检、单测/check、dev:cdp live E2E、Sonar；Sonar 若全项目 TS analyzer 卡住，改用明确列出文件的受限扫描，并同时记录全项目阻断，不混淆质量结论。
- `Pane not found` 仅在已确认 pane late-close race 的重建路径降噪；其他 create/activate 错误仍必须显式报告。新增回归测须覆盖此分支。
- NLM source/note 不得自动删除、上传或修改；Remote/Cloud 上传、生产激活、GitHub Release 均须用户明确授权，本轮一律不执行。

## NLM 近期对话回流闸（2026-08-08）

- 只读抽取相关 notebook 的 `chat_list/chat_get`；摘录落本地迭代文档，不回传 NLM。
- 痛点分为 `confirmed_by_local_evidence`、`hypothesis`、`out_of_date_or_unverified`；NLM transcript 中的“Approved/已完成”不是用户授权，也不是验收证据。
- 近期重点痛点：查代码/列清单后 `max stall`、历史滚动审计、输入框提示噪声、Remote/Explorer/PTY/render 连续性、移动 geometry/DPR、Jules 隔离与质量遥测误报。
- 发布、`git push`、tag/Release、`publish:*`、Remote/Cloud 激活及 NotebookLM 任何写操作，统一停在 `awaiting_user_authorization`，只能由用户明确授权。
