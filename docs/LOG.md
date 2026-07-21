# Ridge 迭代日志

按时间倒序追加；每条记录包含已完成事项、下一步、熔断与被驳回建议。

## 2026-07-21 — iteration 2

- 完成：建立 Controller-facing 最小 capability→RPC 合同与跨入口测试；补齐 rdg `get_file_tree/read_file/text_search` 实际路由；Remote Files/Git/Search、workspace 管理与 theme UI 按 D9 能力协商隐藏/收敛。
- 验证：修复前门禁稳定命中 `get_file_tree` 缺路由；修复后 Remote 定向 Vitest 47/47、ridge-cli rpc 16/16、session 10/10、ridge-core capability 10/10、dispatch 25/25、Remote production build 均 exit 0；增量 svelte-check 70 files、0 errors、exit 0。
- 下一步：iteration 3 仅做 Cloud Remote 重连/背压/恢复的确定性跨层 fault-injection 门禁，优先只改测试。
- 熔断：默认 `pnpm check` 300 秒零诊断超时；checker 证明 `--incremental` 24.6 秒可完成，故未误升为工具链 P0。全仓 rustfmt 被既有格式债务阻断；未批量改无关文件。
- 驳回：NotebookLM 的 `pnpm check --filter ./packages/*` 命令无效；随机 5% 丢包 100% 和 NLC 不作为 CI 验收；Agent 控制台仍缺 Remote topology/安全合同与真实 pane-switch API，继续延期。

## 2026-07-21 — iteration 1

- 完成：校准《远程终端迭代规划与质量修复指南》的陈旧项；修复 Cloud TS capability mirror 缺 `get_workspace_snapshot`、mutating mirror 少 11 项；将固定计数测试改为 Rust canonical 逐项 parity。
- 验证：Remote 定向 Vitest 48/48、transport conformance 32/32、`ridge-term` clear 定向测试、WASM dev build、Remote production bundle 均 exit 0。
- 下一步：iteration 2 仅做“能力宣告→allowlist→handler→UI”的跨入口合同门禁。
- 熔断：note 五类原始建议大多已实现，触发来源陈旧熔断；`pnpm check` 可启动但 180 秒无诊断后超时，完整门禁未宣称全绿。
- 驳回：NotebookLM 的“恢复工具链为 P0”已过期；Remote Agent 控制台在数据能力远程化前不启动；弱网与真机风险无数据，不做确定排序。
- 后续：用户授权跨仓修改后，已同步最新 ridge-cloud，并将 wind 陈旧协议全文收敛为 canonical 入口 + 自动守卫；协议双 SSOT 债务关闭。
