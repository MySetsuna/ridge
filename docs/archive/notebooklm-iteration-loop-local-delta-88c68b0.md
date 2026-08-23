# notebooklm-iteration-loop 本地提交存档

退役日期：2026-08-23  
本地提交：`88c68b01ac0cb3c64f6212221a9496e7a984eb9b`  
本地跟踪基线：`94d893425b427f99e5ad627b7cc55c0cd6d7ba4a`  
退役时远端 `master`：`189d2d55a203a9bf028c38978bf5ad3ce7564de3`  
原仓：`git@github.com:MySetsuna/notebooklm-iteration-loop.git`

移除 gitlink 前，本地分支比远端领先一提交。下列为该提交相对远端基线的唯一内容，
留作可恢复历史；其流程自此不再生效。

完整历史另存 `notebooklm-iteration-loop-88c68b0-standalone.bundle`；可用
`git clone <bundle> <目录>` 独立恢复，无需远端先具备该提交。

## `SKILL.md` 新增：认证闸门

认证失败须拆成「CDP 未就绪 / 浏览器未登录 / Cookie 抽取失败 / CLI 网络或包装器失败 /
NotebookLM API 失败」五类。默认代理固定为 `http://127.0.0.1:51081`，只注入当前
进程与 Chrome，不改系统代理。登录后以外部 CDP 抽取保存认证，再以同一代理验证
`nlm login --check` 与 `nlm notebook list`；不得读取或输出 Cookie、storage、密码、token。

## `SKILL.md` 新增：冷循环安全合同

NotebookLM 被限定为只读假设/方案层，不是批准器、代码事实源或发布器。其建议须经
本地 `PROJECT-STATE`、需求闸、CodeGraph 与运行测试核验；NotebookLM 中的“Approved”
或“已完成”不构成授权或完成证据。source/note/Studio/share 写操作及 push、Release、
Remote 云激活皆须用户逐项明确授权。物理设备、公网 Remote、全项目 Sonar 等无证据时
须标为未验证。认证过期即停止冷循环并记录分类诊断。

## `templates/WORKFLOW.md` 新增

模板同步加入固定代理、外部 CDP、认证双退出码、敏感信息禁出、NLM 只读回流，以及
外部发布等待用户授权等闸门。此段只为保全未推送提交之独有语义，不恢复退役工作流。
