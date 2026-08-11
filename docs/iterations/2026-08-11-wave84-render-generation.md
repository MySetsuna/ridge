# Wave 84：NLM 终端渲染帧序闭环（2026-08-11）

## 结论

NLM 中“PTY/raw feed 与 delta 混流时旧帧复活”的本地代码缺口已补齐。此前
worker 仅给 `applyDelta` 携带 generation，raw `feed`、清屏和输入模式复位仍走
无序协议；现改为每 pane 共用一条 render generation。

## 实现

- `renderWorker.protocol.ts`：`feed` 增加可选 `frameId`，保持旧调用兼容。
- `workerHostedRenderer.ts` / `workerRendererBridge.ts`：透传 generation，仍复制
  原始字节后转移，避免主线程 kernel buffer 被 detach。
- `renderWorker.ts`：feed 与 delta 共用 `lastAppliedFrameId`；非法、重复、迟到
  frame 在触碰 kernel/renderer 前直接拒绝或忽略。
- `manager.ts`：新增 pane 级 `renderFrameId`；普通 PTY 分片、delta、输入模式复位、
  清 scrollback 均经同一 mirror helper 递增并发送。

## 验证

```text
pnpm check                         0 errors / 0 warnings
pnpm test                          215 files; 1983 passed; 1 skipped
focused terminal Vitest            4 files; 99 passed
pnpm cdp:smoke                     PASS
pnpm cdp:pty                       PASS（需本地自签 TLS：NODE_TLS_REJECT_UNAUTHORIZED=0）
node scripts/cdp-lan-probe.mjs     PASS
node scripts/cdp-dpr-e2e.mjs       PASS（DPR 1.5）
```

第一次 `cdp:pty` 未设置本地 TLS 信任时为 WebSocket 1006；放宽本地测试证书后
复跑通过，未归因于产品链路。随后复跑 UTF-8、OSC title、OSC 7 CWD 均通过。

## 仍属外部门禁

- Sonar 新 scanner/CE/Quality Gate：本机旧分析仍 stale，当前认证不可用；不以本地
  Vitest/LCOV 冒充 Sonar Gate。
- 公开 Remote artifact 新鲜度、真实手机后台/换网/键盘恢复、物理 ConPTY/DPR 与
  第三方 runtime：需对应设备、凭据或现场证据。
- NLM 中若仍将 render generation 标为 ACTIVE，须以本地代码与上述测试更新其状态；
  不能把外部现场门禁误记为已完成。

凭据、Cookie、TOTP、Sonar 密码和 token 不写入本仓库。
