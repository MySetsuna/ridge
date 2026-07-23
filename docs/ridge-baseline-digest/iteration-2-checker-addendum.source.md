# Iteration 2 checker addendum — incremental svelte-check

> 由 NotebookLM 来源导出归档（2026-07-23），原 char_count=380

2026-07-21 checker 补充：NotebookLM 建议的 

pnpm check --filter ./packages/*

 实测 exit 1，两个被选 workspace 都没有 check script。真实可用命令 

pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error

 在 24.6 秒完成，检查 70 files，0 errors、7 warnings，exit 0。因此默认 

pnpm check

 300 秒无输出不再足以证明工具链不可用或 P0 阻断；更准确的结论是根脚本缺少增量模式导致检查性能不可接受。该证据应覆盖旧来源中“pnpm/工具链不可用”的陈旧判断。
