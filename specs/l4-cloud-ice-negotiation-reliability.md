---
id: L4-CLOUD-ICE-NEGOTIATION-RELIABILITY
level: L4
title: Cloud RTC 候选交换时序与连接阶段诊断
status: LOCKED
lifecycle: ACTIVE
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - packages/remote/src/shared/cloud/controllerCloudProvider.ts
test_targets:
  - packages/remote/src/shared/cloud/controllerCloudProvider.test.ts
  - scripts/kernel-cloud-owner-acceptance.mjs
acceptance_checks:
  - id: controller-ice-ordering
    command:
      - pnpm
      - exec
      - vitest
      - run
      - packages/remote/src/shared/cloud/controllerCloudProvider.test.ts
    description: 按浏览器限制验证 answer 前和应用中 ICE、队列上限、重连隔离及准确超时阶段
  - id: kernel-browser-rtc
    command:
      - node
      - scripts/kernel-cloud-owner-acceptance.mjs
    description: 实际 Chromium 与 Rust Kernel，经候选先于 answer 的中继调度完成 RTC、加密、鉴权及原终端操作；不冒充公网实测
depends_on: []
---

# Cloud RTC 连接可靠性

保持既有信令、E2EE、鉴权和 DataChannel 协议。控制端收到远端 ICE 早于 answer 或远端描述应用完成时，按该 RTCPeerConnection 实例有界暂存，远端描述成功后逐一提交；队列最多 64 项，超限明确失败并释放该连接。连接重建与关闭清理候选及阶段数据，旧实例异步回调不得污染新实例。可选失效候选不阻断其余有效候选。

连接超时区分主机不在线、尚未发 offer、未收到 answer、远端描述应用中、ICE 未接通、DataChannel 未打开与加密握手未完成。仅输出阶段、候选数量和连接状态等非敏感摘要，不泄漏 SDP、地址、令牌或凭据；未收到 answer 时不得直接归因 NAT。

单元测试采用浏览器约束：未有 remoteDescription 时 addIceCandidate 必须失败，并覆盖 answer 应用延迟、重连与旧实例隔离。真实 Kernel/Chromium 验收在自有中继强制把候选置于 answer 之前，核对鉴权、终端帧及同一进程输入恢复。生产超时根因须另以实际重试证据确认；隔离通过不可称公网问题已解决。
