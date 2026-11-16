---
id: L4-CLOUD-PANE-SUBSCRIPTION-CONTINUITY
level: L4
title: Cloud 分屏订阅与控制连接连续性
status: LOCKED
lifecycle: ACTIVE
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - src/remote/lib/cloudRemote.ts
  - packages/ridge-kernel/src/cloud/connection.rs
test_targets:
  - src/remote/lib/cloudRemote.test.ts
  - scripts/kernel-cloud-owner-acceptance.mjs
acceptance_checks:
  - id: cloud-pane-subscription-activation
    command:
      - pnpm
      - exec
      - vitest
      - run
      - src/remote/lib/cloudRemote.test.ts
    description: 退订携带原分屏的激活标识，无有效标识则跳过，覆盖分屏切换与旧回调
  - id: cloud-pane-switch-browser
    command:
      - node
      - scripts/kernel-cloud-owner-acceptance.mjs
    description: 真实浏览器与 Kernel 经 WebRTC/E2EE 切换两个 shell 和一个无 PTY 分屏，验证控制连接、输入连续性及旧退订作用域
depends_on:
  - L4-REMOTE-WORKSPACE-PANE-CONTINUITY
---

# Cloud 分屏订阅与控制连接连续性

## 范围

约束 Cloud 已鉴权连接上的分屏切换及终端输出订阅。以现有 workspaceId、paneId 和 activationId 定位一次绑定。分屏切换仅影响终端流，控制连接及其他业务请求继续运行。

## 行为

退订取旧分屏的激活标识，再清理客户端对应状态。无有效激活标识时跳过退订。Kernel 按当前连接、分屏及激活标识核对目标，重复或过期退订不影响已激活的新流。缺少标识、错误归属、目标已不存在，或已登记的分屏尚无可订阅的 PTY 时，将失败限制在对应分屏操作。未启动的分屏继续显示其现有状态，不能因此创建终端或关闭控制连接。带请求 ID 时返回相应错误，通知不产生响应。继续执行现有的鉴权、工作区授权及激活校验。真实连接断开仍按现行 Cloud 重连契约处理。

## 验收

生产 Remote 通过真实 WebRTC/E2EE 连接隔离 Kernel，打开两个独立真实 shell。连续切换并切回原分屏，验证双方继续显示输出并接收正确作用域的输入，原 shell 进程仍在，连接无重连或重复鉴权，Files 侧栏能打开。用客户端回归核对退订发出的旧激活标识和未建立绑定时不发送。在同一工作区另加入已登记、无运行 PTY 的分屏。切入和切回时控制连接持续在线，无重复鉴权或隐式启动。用真实连接验证重复、迟到、缺少激活标识与错误归属的退订均不关闭控制通道，也不取消新绑定，授权之外的分屏仍不可访问。按实际环境记录信令、账号、手机与公网覆盖。执行 npm test、npm run lint、npm run typecheck 及 SpecTree 门禁。
