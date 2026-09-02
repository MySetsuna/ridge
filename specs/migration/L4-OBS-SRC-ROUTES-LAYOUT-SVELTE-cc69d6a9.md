---
id: L4-OBS-SRC-ROUTES-LAYOUT-SVELTE-cc69d6a9
level: L4
parent: L3-OBS-SRC-ROUTES-bccc4bc4
title: +layout.svelte
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/routes/+layout.svelte
test_targets:
  - src/remote/authDeviceBinding.test.ts
---

# +layout.svelte

桌面版 Remote Web 入口在以 TOTP 调用 POST /verify 取得会话 token 时，必须提交当前浏览器稳定的 device 标识；随后 WebSocket 鉴权必须复用同一标识，使服务端的 token 装置与来源 IP 双重绑定可通过且不降级。验收：桌面与手机两条 /verify 路径均携带 device，严格绑定 token 可建立 WebSocket，现有鉴权失败与重连行为不变。
