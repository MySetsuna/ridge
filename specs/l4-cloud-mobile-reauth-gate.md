---
id: L4-CLOUD-MOBILE-REAUTH-GATE
level: L4
title: 移动 Cloud 重连鉴权完成后再恢复页面连接
status: LOCKED
lifecycle: ACTIVE
parent: L3-REMOTE-INTERACTION-RELIABILITY
code_targets:
  - src/remote/lib/cloudRemote.ts
test_targets:
  - src/remote/lib/cloudRemote.test.ts
acceptance_checks:
  - id: mobile-reauth
    command:
      - pnpm
      - exec
      - vitest
      - run
      - src/remote/lib/cloudRemote.test.ts
    description: 未授权重连不暴露 connected，成功后恢复一次；失败与过时代际不能开门
depends_on:
  - L4-CLOUD-HOST-REJOIN-SESSION
---

# 移动端重连鉴权门

RTC/E2EE 已连接不代表当前主机会话已获用户授权。移动适配器在重新连接时须等本代际 trust grant 或兼容 TOTP 验证成功，清理旧订阅后才发布 connected 与恢复事件，避免页面查询、终端订阅和 resize 抢在鉴权之前。等待期间保留断线或连接中状态；鉴权失败明确报错。重复断线、过期结果和撤销授权不得使新会话进入可用态。首次连接仍由现有 CloudAuthScreen 完成授权。

覆盖延迟授权期间无 connected 观察、成功事件顺序、失败不恢复、重复断线旧代际忽略及输入幂等。公开 Remote 部署后须在实际 TURN 中继下验证主机服务重连后页面自动保留活动终端并可输入，无需手动重新选 pane。
