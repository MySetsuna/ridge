# Ridge 迭代闭环

本项目的 NotebookLM 循环按以下顺序运行：

1. 读取当前 `CONTRACT-iteration-N.md` 与 `LOG.md` 最近记录。
2. 严格按合同范围实现或校准。
3. 用编译、测试和退出码验收。
4. 写带日期的迭代报告并上传目标 NotebookLM。
5. 获取下一步建议，做来源与代码现实的对抗评审。
6. 归档建议、追加 `LOG.md`、生成下一轮合同。

NotebookLM 是规划 maker；编译器、测试和独立代码审查是 checker。默认授权等级为 Level 2：在独立分支形成可审查提交，不自动合并或发布。

验收采用双轨制：**自动轨**（编译/测试/退出码可判定的目标，合同验收对象）与**用户轨**（真机 smoke、生产实跑、人工核验、分支合并等只能由人完成的事项，见 `docs/plans/user-verification-checklist.md`）分列；自动轨不得以实验室模拟数据冒充用户轨结论。

节律（iteration 10 起固化）：每轮闭环提交前必跑 `node scripts/generate-review-pack.mjs` 刷新 `docs/review/branch-review-guide.md`，使审查导读始终覆盖分支全部领先提交；用户轨积压未消化期间，不启动扩协议面的重型变更。

维护态（iteration 12 起，**仅当开放愿景清单 open=0** 时启用）：一轮维护 = 全门禁绿（cargo/vitest/svelte-check）+ 导读刷新 + 零回归即闭环，不设新功能目标。若 notes / `docs/iterations/*-open-vision-checklist.md` 仍有 `open` 行，**禁止**进入维护态空转。

**Notes 清空 ≡ 愿景全实现**（`notebooklm-iteration-loop` 硬规矩）：未代码落地的愿景不得删 note；可用标题 `[已实现]` 或清单行 `implemented` **视同清理**。禁止把「待用户轨」写成实现后清空 notes。原「待用户裁定」项：先 NLM 深研 → 对抗评审 → **执行者拍板并实现**。

红线不变：无充分理由不扩协议面/新状态源/改 E2EE；真机/生产/合并仍见 `docs/plans/user-verification-checklist.md`（不挡功能代码愿景闭合）。

