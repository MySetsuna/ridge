---
id: L4-RUNTIME-EMPTY-WORKSPACE-BOOT
level: L4
title: 空 Runtime 首次启动与已有工作区恢复
status: LOCKED
lifecycle: ACTIVE
parent: L3-RUNTIME-WORKSPACE-CONTINUITY
code_targets:
  - src/lib/stores/paneTree.ts
test_targets:
  - src/lib/stores/paneTree.coverage.test.ts
---

# 空 Runtime 首次启动与已有工作区恢复

全新 Runtime 可合法没有工作区。工作区列表查询成功且为空时，刷新成功结束，不再查询不存在的活动工作区，不隐式创建任何工作区，也不报连接故障；启动路径随后依据这一权威空列表创建默认工作区。列表读取、鉴权或网络失败继续向上传播，不得伪装为空列表并创建工作区。已有工作区按调用方选择与既有时序约束恢复，刷新或重连不得额外创建工作区。并发切换的过期响应不得覆盖当前选择。

验收以真实 Kernel 的全新独立桌面配置复现空白，再验证首次启动得到可输入的终端，重载仍保留同一工作区和 pane。定向 store 测试覆盖空列表时后端活动查询会拒绝、失败不吞、非空列表及已有并发选择回归，运行 npm test、lint、typecheck 和 SpecTree verify。
