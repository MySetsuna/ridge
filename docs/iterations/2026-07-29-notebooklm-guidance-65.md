# NotebookLM guidance 65

## 结论

- WebWorker 适合 scrollback 的 UTF-8/ANSI 解码、seq 区间校验、分页合并准备、历史搜索；主线程仍拥有 terminal kernel、DOM、焦点与提交游标。
- 使用可转移 `ArrayBuffer`，消息协议带 `requestId`、`PaneRef`、seq 区间和取消；有界高水位，关闭/切换时丢弃过期结果并归零计数。
- WebWorker 不能修复服务端 WebSocket writer 阻塞。LAN host 必须独立 writer actor，以 bounded critical/active/background 队列调度；禁止第二物理连接。
- 复合 `(workspaceId,paneId)`、键盘“回底→光标/中心→focus”、后台保活及 Agent 状态同源均属本轮硬验收。

## 对抗评审 / 重构取舍

| 建议 | 采用方式 | 反例与替代 |
|---|---|---|
| Worker 镜像 terminal | 仅计算快照/解析，不拥有 SSOT | 不传 DOM，不建立第二 socket；kernel 仍由主线程/后端 seq 驱动 |
| DRR/优先 writer | 服务端分离发送生命周期与 reader，bounded lanes | Worker 无法解决 TCP/服务端队头阻塞 |
| 缺 workspace 兼容 | 仅握手且唯一映射时升级 | 业务帧缺失/冲突直接拒绝，禁 activeWorkspace 回退 |
| 性能指标 | 测队列峰值、取消归零、输入优先与身份竞态 | 不以固定 FPS/延时魔法数宣称流畅 |

## 迭代 65 验收

1. `wsA/pane1` 延迟响应在切到 `wsB` 后不得落错 pane；缺 workspace 业务帧拒绝。
2. scrollback 大流挂起时 stdin/control/active raw 仍入队且最多等待一个已开始 low frame。
3. Worker 取消后 pending/bytes 计数归零，过期页不推进 cursor；主线程可继续输入。
4. 键盘唤起顺序固定为 `scrollToBottom → cursor/fallback center → focus`，不改变 PTY rows/cols。
5. `ws1/A → ws1/B → ws2/C → ws1/A` 后三 pane 保活，切回不 RIS/全量 replay。

来源：NotebookLM notebook `66919cb9-1329-4ddf-955c-f426d15a9fe6` 的深研报告与 REQUIREMENTS-SPEC、PROJECT-STATE；代码事实以 CodeGraph 为准。
