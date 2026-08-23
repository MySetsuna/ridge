# SpecTree 迁移与性能基线（2026-08-23）

## 本轮确认并修复

- Agent activity 稳定计时原以 pane 为键，且每个工作区刷新后清全局表；并发工作区
  相互逐出，故空闲可长期不收敛。现按 workspace + pane 隔离并仅清当前工作区。
- autodiscover 原缓存「某一组 pane 的匹配结果」；250 ms 活动潮中不同工作区交替
  使 500 ms TTL 失效，反复枚举全机进程。现缓存全局进程快照，各工作区只内存匹配。
- Agent 高亮原仅靠事件刷新；输出停止后若 Commune 未挂载，无事件在 12 秒窗后唤醒。
  现全局同步器按工作区设一次性 expiry timer，并在卸载/工作区移除时清理。
- Commune 原在全局同步器之外再监听 pane 输出并拉全量 topology；现复用共享 store，
  常规 layout/output 刷新由全局数据面独占，同次亦复用 HITL response；暂停/裁决等
  明确用户动作才由面板显式刷新。
- 工作区移除时同步清除 topology、status、attention、ack、观察 Map、队列与 expiry
  timer，免共享前端运行态长期滞留。
- Remote 视图关闭原只令 `TerminalCanvas` park，未 detach 单例管理器中的 WASM kernel；
  pane 关闭亦仅清帧调度器，切换缓冲、重同步 timer、性能 trace 仍滞留。现 pane、
  workspace 与整视图销毁皆经同一释放口，退订 transport、清逐-pane 状态并释放 kernel；
  已取消的重同步 microtask 不得复活。
- 侧栏原仅以 `sidebarVisited` 阻止隐藏组件挂载，静态 import 仍把 SCM、Search、Remote、
  Commune、Hosts 与共享 Remote surface 拉入首屏模块图。现皆置于原访问/投影闸后的
  dynamic import；全局 Agent 高亮同步器仍常驻，故后台状态能力未换成“开面板才工作”。
- Playwright 原可复用占用 5173 的任意站点，且以整页 `/` 作 server-ready 探针，混淆
  端口碰撞、冷 SSR 与产品断言。现端口严格独占且可配置，轻探 `@vite/client`，页面
  启动由用例本身验收；Vite 冷 reload 不再触发第二次竞态导航。

## 已有确定性证据

- Rust：跨工作区稳定时钟不互逐；不同 workspace query 在 TTL 内仅枚举一次进程。
- 前端：确认后的同状态不再高亮；状态改变后恢复通知；未刷新工作区状态不丢。
- 生命周期：全局 expiry timer 按工作区覆盖、删除并在组件卸载时清空。
- Remote 生命周期：逐-pane hand-off buffer 可独立回收且字节账归零；源码合同锁定
  queue、timer、trace、transport、kernel 五类资源皆由 view teardown 释放。
- 首屏边界：源码合同锁定七个重面板为 dynamic import，且共享 Remote surface 仅在
  projection 存在时取包；相关 focused Vitest 19/19、Svelte check 0/0。

## 仍开放的最大风险

- Remote 公网/实体设备长期 heap、CPU、网络、重连、后台恢复与 IME soak 本轮无新
  运行产物，故不得声称「无内存泄漏」或「Remote 性能完成」。
- 真 Chrome 已排除误连他站并进入独占 Vite；然本机冷 `/` SSR 约耗两分钟，继而
  客户端模块图等待 180 秒仍无 primary rail，最终仅停在启动闸、无 JS 异常。
  现场同时约 88 个 Node 与 32 个 Chrome 进程，故此失败不可作产品视觉结论；所有
  本轮启动/构建子进程均按树回收，5174 无残留监听。
- 后续须按 `L4-REMOTE-PERFORMANCE-GATE-001` 分离 client/host/relay 指标，并验证
  queue、timer、listener、transport、terminal 数量在反复切 pane/重连后回归有界基线。
- 现有确定性调度与 dispose 测只能证明合同分支，不能替代真实网络与设备证据。

## 当前变更

`CHG-001` 覆盖已有代码事实支持的 Agent activity、Commune UX、共享进程扫描、
Remote 生命周期回收与首屏代码分割；Remote/视觉现场性能仍留 DRAFT 闸，未冒充
整体验收完成。
