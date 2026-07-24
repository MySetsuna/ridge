# [待调研·未开工] 终端链接/路径跳转 · hover 下划线 · TUI 鼠标行为回归

**状态**：等待调研 → 实现 → 修复 · **先不开工**  
**笔记本用途**：产品/交互审核 + 排期备忘；不计入当前 open 愿景（与「完整 WS 出站 PTY」note 同级：规划态）  
**相关代码（现状锚点，非验收）**：
- `packages/remote/src/shared/terminal/manager.ts`（pointer / Ctrl+click / TUI mouse 优先）
- `packages/remote/src/shared/terminal/linkSpans.ts`（纯文本 URL/路径检测）
- ridge-term WASM：`hyperlinkAt` / OSC 8、mouse reporting modes
- `tuiGate.ts`（键/滚轮是否归 TUI；与鼠标分支协同）
- `RidgePane.svelte`（host 快捷 vs 程序鼠标）

---

## 1. 问题域（要修什么）

### A. 终端里的「连接」与文件路径

| 场景 | 期望 | 现状粗描（待调研核实） |
|---|---|---|
| **OSC 8 超链接** | 可发现、可点、可跳转 | `kernel.hyperlinkAt`；Ctrl/Cmd+click 打开；hover 时 cursor=pointer |
| **纯文本 URL** | 同上 | `LinkSpanIndex` lazy 扫可见区 |
| **文件路径**（绝对/相对/~/file URL） | 点击跳转 **编辑器打开 / 资源管理器定位**（产品定） | linkSpans 已分 kind（url/win-abs/posix-abs/home/rel）；打开路径与 openUrl 分支需统一产品语义 |
| **行:列**（`foo.rs:12:3` 类） | 可选：打开并定位 | 是否支持待调研 |

### B. Hover 下划线（明确缺口）

**用户要求**：可点连接/路径在 hover（或 Ctrl-hover，产品定）时 **显示下划线**，而不只是改 cursor。

| 点 | 说明 |
|---|---|
| 现状 | 注释写 renderer 有 underline pass / OSC 8 相关；Ctrl-hover 主要改 **pointer 光标**；纯文本 span 是否稳定画下划线需调研 |
| 目标 | 用户能 **看见** 可点区域（下划线或等价 affordance），减少「点了没反应 / 误点」 |
| 风险 | 与 SGR 自带 underline、选区高亮、TUI 自绘 UI 叠层冲突 |

### C. 与 TUI CLI 鼠标控制的交互 / 回归

TUI（vim、tmux、claude/Ink、fzf…）开启 DEC mouse（`?1000/?1002/?1003` + SGR）时：

| 规则（设计意图，manager 注释已写） | 常见回归症状（待复现建用例） |
|---|---|
| **Mouse reporting 开启 → 鼠标事件优先给程序**（无 Alt 逃生改 host 选区） | 点选被 TUI 吃掉；或反过来 host 抢选区导致 TUI 错位 |
| 滚轮：reporting 时 SGR wheel；alt-screen 无 mouse 时 arrow 兜底 | 滑轮进 shell 垃圾序列；或 pager 滑不动 |
| `tuiGate`：键盘/历史弹层不与 TUI 抢 | ArrowUp 弹出 shell-history（历史 bug 类） |
| pointermove rAF 批处理 / 松键 dedup | 「点选从上一帧位置开始」类错位 |
| 选区状态与 TUI motion 隔离（`selecting` 不泄漏） | 退出 TUI 后残留拖选 |

**本 note 要求**：链接/路径跳转与 hover 下划线的方案 **不得** 以破坏 TUI 鼠标契约为代价；任何改动需带 **回归用例**。

---

## 2. 建议调研清单（开工前）

1. **渲染**：underline 画在 WASM renderer 还是 DOM overlay？OSC 8 与 linkSpans 是否共用一条绘制路径？  
2. **触发手势**：始终 hover 下划线 vs 仅 Ctrl/Cmd-hover（VS Code/iTerm 常后者）？移动端 Remote 无 Ctrl 如何表达？  
3. **点击手势**：Ctrl+click only vs 单击（与 TUI click 冲突时如何仲裁）？  
4. **路径解析**：相对路径相对谁（pane cwd / workspace root）？不存在路径时 toast？  
5. **回归矩阵**：mouse off / ?1000 / ?1002 / ?1003 × 链接 hover/click × 选区 × 滚轮 × tuiGate 键。  
6. **Remote 桌面/手机**：同一 TerminalManager 路径是否双端一致？

---

## 3. 目标交互（草案，供批，非定稿）

### 3.1 发现

- 悬停在可点 span 上：  
  - **下划线**（本 note 硬需求）  
  - cursor=pointer  
  - 可选：tooltip 显示解析后目标  

### 3.2 激活

- **默认草案**（可驳）：  
  - **TUI mouse 开**：单击 → **仅** 程序；链接跳转需 **Ctrl/Cmd+click**（或程序自己的 click）。  
  - **TUI mouse 关**：Ctrl/Cmd+click → 打开链接/路径；是否允许无修饰单击路径 → 调研后定。  
- 文件路径：优先 Ridge 内 `FileEditor` 打开；目录 → 资源管理器/树定位；http(s) → 系统浏览器。

### 3.3 失败

- 解析失败 / 文件不存在：非模态提示，不吞键、不崩 TUI。

---

## 4. 实现落点猜想（调研后修正）

| 模块 | 可能改动 |
|---|---|
| ridge-term 渲染 | hover  span 下划线 pass；与 OSC 8 cell 属性一致 |
| `linkSpans.ts` | 规则/行:列；dirty 时机 |
| `manager.ts` | hover 状态机、与 mouseReporting 仲裁、下划线 invalidate |
| `tuiGate` / RidgePane | 仅当链接手势与键位冲突时扩展；默认少动 |
| 打开动作 | 统一 opener：url vs path vs editor |

---

## 5. 验收方向（实现阶段再写成合同）

1. OSC 8 + 纯文本 URL/路径：hover **可见下划线**，Ctrl+click 正确跳转。  
2. 文件路径进编辑器（或产品定的目标）可测。  
3. TUI mouse on：单击不误开链接；程序仍收 SGR。  
4. 历史回归：无 shell-history 抢 Arrow；无 wheel 垃圾；无选区从旧坐标起。  
5. 单测：linkSpans hit + 仲裁纯函数；可选 manager 手势表。

---

## 6. 排期与边界

- **先不开工**；本 note 仅挂账。  
- 不并入「完整 WS 出站 PTY」里程（正交：本 note 是本机/已连接终端的渲染与输入仲裁）。  
- 不自动改 open 愿景清单；立项时再进 contract。

## 7. 请审核

1. 下划线：始终 hover 还是仅 Ctrl-hover？  
2. 路径单击是否允许（TUI off 时）？  
3. 相对路径 cwd 规则？  
4. 是否必须覆盖手机 Remote？  

**请批后可开调研；未批不写码。**
