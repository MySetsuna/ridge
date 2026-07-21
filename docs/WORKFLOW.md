# Ridge 迭代闭环

本项目的 NotebookLM 循环按以下顺序运行：

1. 读取当前 `CONTRACT-iteration-N.md` 与 `LOG.md` 最近记录。
2. 严格按合同范围实现或校准。
3. 用编译、测试和退出码验收。
4. 写带日期的迭代报告并上传目标 NotebookLM。
5. 获取下一步建议，做来源与代码现实的对抗评审。
6. 归档建议、追加 `LOG.md`、生成下一轮合同。

NotebookLM 是规划 maker；编译器、测试和独立代码审查是 checker。默认授权等级为 Level 2：在独立分支形成可审查提交，不自动合并或发布。

