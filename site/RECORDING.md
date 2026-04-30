# 录制 / 截图指南

主页和文档页里那些灰色的 "RECORDING SLOT · 录制位" 占位卡，
都是给你后续替换的位置。这份文档告诉你：**用什么录、录什么、放哪里**。

---

## 1. 推荐工具（Windows 优先）

| 用途 | 工具 | 备注 |
|--|--|--|
| **GIF 短循环（首选）** | [ScreenToGif](https://www.screentogif.com/) | 免费 · 开源 · 可裁剪 / 编辑帧 / 优化体积。Ridge 演示主力 |
| **MP4 视频 / 高质量** | [OBS Studio](https://obsproject.com/) | 免费 · 跨平台 · 可录窗口 / 区域 / 全屏，支持 60 fps |
| **多功能截图 + 录屏** | [ShareX](https://getsharex.com/) | 免费 · 自带快捷键 / 注解 / 上传 |
| **静态截图（精修）** | Snipaste / Windows + Shift + S | Snipaste 支持贴图比对 |
| **终端纯文本演示（可选）** | [asciinema](https://asciinema.org/) | 体积极小，但 Ridge 还嵌了图形 UI，未必合适 |

> 个人建议：**80% 用 ScreenToGif、20% 用 OBS（需要 mp4 时）**。
> ScreenToGif 录完直接就能裁、调速、删帧、压色——不用再过一遍剪辑软件。

---

## 2. 通用录制规范

- **窗口尺寸**：在 Ridge 主窗口右下角拖到 **1280 × 720**（可在 OBS / ScreenToGif 里用 Pixel Ruler 校准）。
  这是占位卡的设计比例，替换后不会出现黑边。
- **DPI**：Windows 下尽量在 **100% 缩放**的显示器上录，否则字体会糊。
- **帧率**：GIF 用 **20–30 fps** 已足够丝滑且体积合理；MP4 推荐 **30–60 fps**。
- **时长**：
  - 主 hero demo：**8–15 秒**循环（开 → 切 → 用 → 回到开始）
  - 单一特性 GIF：**5–8 秒**
- **体积**：GIF 控制在 **3 MB 以下**，MP4 在 **5 MB 以下**。ScreenToGif 的「编辑 → 减少帧 / 调色板」是体积大杀器。
- **去敏感**：录之前关闭你不想出镜的 Tab、把项目 cwd 切成展示用的临时仓库（避免泄露内部路径）。

---

## 3. Shot list（需要录哪些）

把文件按下面的**确切名字 + 路径**保存，主页 / Releases 页会自动检测并用真文件替换占位 SVG。

> 浏览器打开页面后，`site/scripts/main.js` 会针对每个占位卡执行一次 HEAD 探测：
>
> - 如果文件存在并且后缀是 `.mp4`，自动换成 `<video autoplay muted loop>`；
> - 否则（`.gif` / `.png` / `.jpg`）换成 `<img>`。
>
> 占位卡右下角的 `replace: …` 灰字直接告诉你要保存的路径。

| 文件 | 类型 | 要展示什么 |
|--|--|--|
| `site/assets/media/hero-demo.mp4`（或 `.gif`） | 主 demo · 8–15s 循环 | 「打开 Ridge → 切两刀分屏 → 一边跑命令 / 一边编辑 / 一边看 Git 图」整段流畅展示，是首页 hero 右侧的主角 |
| `site/assets/media/splitpanes.gif` | 5–8s | 鼠标拖拽分屏 + 快捷键 (`Ctrl+\` / `Ctrl+-`) 切分。突出「田埂」分割线 |
| `site/assets/media/editor.png` | 静态截图 | 资源管理器 + Monaco 编辑器并存的画面，最好一边是终端一边是文件 |
| `site/assets/media/gitgraph.png` | 静态截图 | Git Graph 视图，多分支、多提交，最好有 ahead/behind pill |
| `site/assets/media/agent-team.gif` | 8–10s | 在 Ridge 内启动 `claude`，让它通过 tmux shim 操作其它 pane（list-panes / rename-window）|

录好之后**直接覆盖**到那些路径。不需要改任何 HTML/JS——刷新页面占位卡就替换了。

---

## 4. 一次完整录制流程（ScreenToGif 例）

1. 打开 Ridge，窗口拖到 1280×720。
2. 启动 ScreenToGif → 选择 "Recorder" → "Window" 模式 → 指向 Ridge。
3. 把窗口里要展示的内容预热好（cwd 切对、字体调好、屏蔽通知）。
4. 按 `F7` 开始 / `F8` 结束。
5. 进入编辑器：
   - **Image** → **Crop** 裁掉标题栏阴影（如果有）
   - **Reduce frame count** 跳每隔 1 帧 → 体积减半
   - **Edit** → **Reduce color depth** 256 色 → 再减半
6. **File** → **Save as** GIF → 保存到对应路径。

> 如果你想要更高质量，输出成 MP4 (用 OBS 或 ScreenToGif 的 "Save as Video")，
> 文件名按上表的 `.mp4` 路径放——主页脚本会自动用 `<video>` 播放。

---

## 5. 静态截图建议

- 截 Ridge 时按 **Win + Shift + S**（区域截图）或用 ShareX 的「带阴影窗口截图」。
- 推荐导出成 **PNG**（无损），让浏览器自己去做缩放和锐化。
- 不要 JPG hero，否则压完看着糊。

---

## 6. 可选：替换 favicon / brand mark

`site/assets/favicon.svg` 和 `site/assets/ridge-mark.svg` 是临时设计的 田 字 logo。
如果你后续做了正式品牌设计，直接覆盖这两个文件即可（保持 SVG 格式 + 大致尺寸）。

---

## 7. 上线前 checklist

- [ ] 至少替换了 `hero-demo` 占位（其它可以分批补）
- [ ] 在浏览器打开 `site/index.html` 本地预览，确认资源都加载
- [ ] 推到 main 触发 `.github/workflows/deploy-pages.yml`
- [ ] 在仓库 Settings → Pages 把 source 设为 "GitHub Actions"
- [ ] 第一次部署后访问 `https://mysetsuna.github.io/ridge/` 验证

---

## 8. 录制时不希望出镜的东西

- 真实的本机用户名 / 路径（演示前 `cd ~/code/ridge-demo` 之类的临时目录）
- 通知中心 / 系统时间（如果不希望暴露时区）
- 浏览器书签条 / 公司内部域名
- 微信 / 飞书等聊天软件弹窗
