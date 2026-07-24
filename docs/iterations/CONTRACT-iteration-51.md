# CONTRACT — Iteration 51 / AC4-C51（约 2 日 · 终端链接打开主机路径）

**Credit ID**: C51 · OP-TERM-LINK 加厚

## 产品结果
linkOpenHost：安全 URL、path:line:col、file-url、underline dataset、CSS tokens；与 linkAffordance 仲裁组合；manager 已有 Ctrl-hover 接线。

## 独占主文件
- `packages/remote/src/shared/terminal/linkOpenHost.ts`(+test)
- `packages/remote/src/shared/terminal/linkAffordance.ts`
- `packages/remote/src/shared/terminal/manager.ts`（hover underline）

## 验收
vitest linkOpenHost + linkAffordance；composition protocol_link 场景。

## 停机
重写整终端；TUI mouse on 时裸单击开链。
