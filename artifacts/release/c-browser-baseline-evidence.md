# C 浏览器基线（Goal #4）

**Date**: 2026-09-15
**改了什么**: 新增 `packages/remote/src/shared/terminal/managerPerfBaseline.test.ts`（5 tests，覆盖 100/500/1000/5000 行 feed + park/unpark 切回）

## 限制（重要）

> 没有真机/真 WebGPU。本测在 jsdom + vi.useFakeTimers() 下用真实 `TerminalManager` 代码路径，但 kernel 是 mock（`vi.fn()` 替身），surface host 是 fake canvas。**测的是 manager 内部的 buffer-flush / deferred 路径**，不是 wasm kernel.feed 本身。这是「依据测量修复」原则下的最大可执行切片 —— Goal 说「若 WebGPU/浏览器确实无法运行，保存失败证据，继续能测的解析、历史请求和生命周期部分」，本测即该切片。

真机（Android Chrome / iOS Safari）的 xterm canvas + 真 wasm kernel.feed 实测需要 Playwright on device 或真机 + CDP —— 列入下轮，最低人工步骤：

1. 启动 candidate（`./target/test-rdg/release/ridge.exe host --port 5120`）
2. 桌面 Chrome `chrome://inspect` → 远程发现 5120 → inspect
3. 在 console 执行：
   ```js
   await (await import('/_app/immutable/.../manager-XXXX.js')).TerminalManager.instance();
   // 在 DevTools Performance 面板录 5s，触发 5000 行 feed（cat 5000 行文件）
   ```
4. 抓取：JS heap 增长、feed 总时长、render frame rate

## 基线数据（2026-09-15 18:14, Windows 11, jsdom）

```json
{
  "100rows":  { "payloadBytes": 8116,   "wallMs": 0.68,  "kernelFeedCalls": 1  },
  "500rows":  { "payloadBytes": 40583,  "wallMs": 0.17,  "kernelFeedCalls": 3  },
  "1000rows": { "payloadBytes": 81166,  "wallMs": 0.16,  "kernelFeedCalls": 5  },
  "5000rows": { "payloadBytes": 405833, "wallMs": 2.22,  "kernelFeedCalls": 25 },
  "switchBack": {
    "initialFeedCalls": 1,        // 首次 attach + 50 行 → 1 次 kernel.feed
    "initialPrependCalls": 0,     // 初始未触发 prependScrollback
    "midPrependCalls": 0,         // park 期间 30 行 → 未重灌
    "afterUnparkFeedCalls": 2,    // unpark 后 flush 一次 → 累计 2 次
    "afterUnparkPrependCalls": 0  // unpark 后**未**重灌历史 ✓
  }
}
```

完整数据：`artifacts/release/manager-perf-baseline.json`

## Invariants（基线保证）

### 「切回」不重灌历史

```ts
it('park 期间累积 buffer、unpark 后只 flush 一次、不重新 prepend scrollback', async () => {
  // ... park + feed + unpark
  expect(afterUnparkPrependCalls).toBe(initialPrependCalls);
  // 当前：0 == 0  ✓
});
```

- **结论**：park 期间累积的 PTY 字节在 unpark 后通过 deferred buffer flush 路径送到 kernel.feed；**不**重新 prepend 历史（manager 保留 kernel 状态，park/unpark 不动 entry.kernel）。
- **风险**：未来若有人把 `unpark` 改成「从 backend 重新 fetch 完整 scrollback」并 prepend，此测试立即失败。

### 100/500/1000/5000 行能完整 feed

四档 payload 全部跑通，无 throw / 无 drop。5000 行（405KB）wallMs 2.2ms 是在 jsdom mock 下的耗时；真 wasm kernel.feed 涉及更多（ANSI 解析 + 行布局 + 画布上传），wallMs 预期高一个数量级。

## 复现

```bash
RIDGE_BASELINE_OUT=/tmp/ridge-baseline.json \
  pnpm vitest run packages/remote/src/shared/terminal/managerPerfBaseline.test.ts
cat /tmp/ridge-baseline.json
```

## 下一步

- 跑 `cdp-mobile-perf`（Playwright on device）拿真机数据
- 同样的脚本对比前后 → 找 regression：feed 耗时、prependScrollback 调用次数、kernel.free 调用次数、bufferChunks 增长
- 若真机显示 5000 行 wallMs > 200ms，先查 kernel.feed 自身而非 manager 缓冲；**不**盲扩 feed chunk，**不**新写 ANSI parser
