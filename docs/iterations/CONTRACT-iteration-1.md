# CONTRACT — iteration 1

日期：2026-07-21  
主题：Remote 终端规划基线校准

## 目标

核对 note《远程终端迭代规划与质量修复指南》的五类建议是否仍是当前缺口，用确定性检查生成可信的新规划输入。

## 范围

- Remote Git Diff 的 allowlist、dispatch 与 viewer 调用链。
- 移动复制/选择、TUI mouse reporting、移动白屏防护。
- shell clear 与本地/host scrollback。
- 桌面滚动条主题与复用入口。
- 迭代报告、NotebookLM 查询、对抗评审、iteration 2 合同。

## 边界

- 无失败证据时不改运行时代码。
- 不改安全 allowlist 的排除项。
- 不发布、不合并、不自动部署。

## 可验证验收

1. Remote allowlist、lazy scrollback 与 scrollbar 单测退出码为 0。
2. `ridge-term` clear/scrollback 定向测试退出码为 0。
3. Remote bundle 构建退出码为 0。
4. 报告成功上传 NotebookLM，下一轮指导已归档并经对抗评审。

## 停机条件

- 测试或构建失败：先记录并修复首因，停止“已完成”判定。
- 来源与代码冲突：代码 + 测试优先，冲突写入报告。
- 本轮最多一次 NotebookLM 重规划查询；若仍重复陈旧建议则触发无进展熔断。

