# NLM 候选核验清单（2026-08-09）

本记录仅保存本轮 NotebookLM 只读查询返回的候选；NLM 输出不构成需求、根因或验收证据。已批准执行边界为 `REQ-NLM-ITERATION-01`，候选须回到本地需求、CodeGraph、测试和运行事实核验。

| ID | 候选痛点 | 本地条款 | NLM 分类 | 最小核验 |
| --- | --- | --- | --- | --- |
| `BUG-MOBILE-RUNTIME-ERROR` | 手机 Remote `runtime.lastError` 归因 | `REQ-MOBILE-REMOTE-RUNTIME-LASTERROR-01` | hypothesis | clean Profile、禁用扩展运行 30 分钟，确认控制台无该错误 |
| `BUG-TERMINAL-GLYPH-RASTER` | ridge-term WebGPU/Canvas2D 字形栅格平滑度 | `REQ-TERMINAL-RASTER-01` | hypothesis | DPR `1/1.25/1.5/2` 与原生 PowerShell 同样例像素对比 |
| `BUG-CODEX-RENDER-STABILITY` | Codex 连续输出半帧、旧帧复活、光标闪跳 | `REQ-CODEX-RENDER-STABILITY-01` | hypothesis | 同一 PTY 录制逐帧 trace，验证 frame generation 单调且无旧行复现 |
| `BUG-EXPLORER-FILE-DESYNC` | Windows 跨卷移动部分失败导致 Explorer 状态漂移 | `REQ-EXPLORER-FILE-CONTINUITY-01` | hypothesis | 物理卷失败场景对账 `FileTree` DTO 与磁盘状态 |
| `BUG-MOBILE-BACKGROUND-ALIVE` | Mobile Remote PWA 后台/换网后连接与状态丢失 | `REQ-REMOTE-SMOOTH-STATE-02`, `REQ-MOBILE-REMOTE-STATE-01` | hypothesis | 物理手机后台超过 15 分钟后切回，无重新 TOTP 且显示 PTY 尾帧 |

## 当前处理规则

- 先用 CodeGraph 定位本地符号与调用链，再运行已有测试/最小实验；未证伪前不宣称实现或根因确认。
- 物理手机、真实 DPR 像素对比、真实 Windows 权限卷等用户轨证据缺失时，只记录 blocked evidence，不以单测替代。
- 不写入、删除或上传 NotebookLM source/note；不执行发布、推送或 Remote/Cloud 激活。

