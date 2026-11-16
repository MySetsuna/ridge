# CHG-131 提案归档索引

当前产品要求以根 specs/ 为准，实施状态以 changes/ 和 SpecTree 验证为准。工作树里的同号候选可能对应不同需求，不能直接复制到主仓。

下表只记录本轮核对的遗留稿与近期归档。旧稿保留原文，其中的旧限制、未执行计划或完成声明均不替代现行规范。历史 `.change.md`、诊断清单及更早材料不属于待应用提案。

| 原稿来源 | 主仓落地 | 处置 |
| --- | --- | --- |
| [main/.spectree/proposals/CHG-071.json](../../.spectree/proposals/archive/CHG-131/main-CHG-071.json) | [CHG-071](../../changes/CHG-071.md) | 已完成变更的未应用旧稿；与同号既有归档内容不同，保留原文，当前实现以后续规范为准 |
| [remote-detached-cwd/.spectree/proposals/CHG-119.json](../../.spectree/proposals/archive/CHG-131/remote-detached-cwd-CHG-119.json) | [CHG-124](../../changes/CHG-124.md) | 当前分屏 CWD 已由主仓 CHG-124 落地 |
| [commune-shared-daemon-spec/.spectree/proposals/CHG-122.json](../../.spectree/proposals/archive/CHG-131/commune-shared-daemon-spec-CHG-122.json) | [CHG-128](../../changes/CHG-128.md) | 共享 daemon 可信绑定已按主仓新编号 CHG-128 落地 |
| [commune-shared-daemon-spec/.spectree/proposals/CHG-123.json](../../.spectree/proposals/archive/CHG-131/commune-shared-daemon-spec-CHG-123.json) | [CHG-124](../../changes/CHG-124.md) | Cloud 订阅连续性已整合主仓 CHG-124 |
| [commune-shared-daemon-spec/.spectree/proposals/CHG-124.json](../../.spectree/proposals/archive/CHG-131/commune-shared-daemon-spec-CHG-124.json) | [CHG-126](../../changes/CHG-126.md) | Windows CLI CWD 原生目录验收已由 CHG-126 落地 |
| [commune-shared-daemon-spec/.spectree/proposals/CHG-125.json](../../.spectree/proposals/archive/CHG-131/commune-shared-daemon-spec-CHG-125.json) | [CHG-124](../../changes/CHG-124.md) | 手机编组导航已整合主仓 CHG-124 |
| [applied-main/CHG-124.json](../../.spectree/proposals/archive/CHG-124.json) | [CHG-124](../../changes/CHG-124.md) | 补齐已应用提案溯源；文本指纹按 Git 换行规范化核对 |
| [applied-main/CHG-125.json](../../.spectree/proposals/archive/CHG-125.json) | [CHG-125](../../changes/CHG-125.md) | 补齐已应用提案溯源；文本指纹按 Git 换行规范化核对 |
| [applied-main/CHG-126.json](../../.spectree/proposals/archive/CHG-126.json) | [CHG-126](../../changes/CHG-126.md) | 补齐已应用提案溯源；文本指纹按 Git 换行规范化核对 |
| [applied-main/CHG-127.json](../../.spectree/proposals/archive/CHG-127.json) | [CHG-127](../../changes/CHG-127.md) | 补齐已应用提案溯源；文本指纹按 Git 换行规范化核对 |
| [applied-main/CHG-128.json](../../.spectree/proposals/archive/CHG-128.json) | [CHG-128](../../changes/CHG-128.md) | 补齐已应用提案溯源；文本指纹按 Git 换行规范化核对 |

旧稿字节及近期文本指纹、节点和落地编号由 [check-proposal-archives.mjs](../../scripts/check-proposal-archives.mjs) 核对；唯一清单位于 [架构文档规范](../../specs/l3-architecture-documentation.md)。

本轮另复核十个本机资料与生成物观察节点：九个未定义产品行为的占位归档，OpenCode 模块保留 Git 跟踪的配置源。十份原节点仅按既有 Git blob 保存；本机环境及私有配置未迁入。处置及原文索引见[映射治理规范](../../specs/l3-spec-mapping-governance.md)。MCP 原有真实打包产物映射保留；目录缺产物属构建状态，不据此重写有效契约。源分支曾按 0.1.100 构建并核对版本。

六个 Commune 需求原来重复执行同一全量 UI 脚本，现归并至[共用验收单元](../../specs/l4-commune-ui-acceptance.md)。该单元依赖全部六个需求，任一需求变更均须重新执行完整脚本；断言及其他验收命令保留，历史结果不能替代本轮执行。

重复的接入与权限回归亦分别归并至[接入 UI 验收](../../specs/l4-commune-ui-membership-acceptance.md)和[权限 UI 验收](../../specs/l4-commune-ui-permission-acceptance.md)。接入原两次归一，权限原三次归一；保留各源需求依赖、原命令与全部断言，其他命令次数不变。

## 源分支记录

源分支 90c312a5b8b6a241ab14fb1e6ee8082e5239fd2f 的原编号 CHG-129 与主仓同号的焦点修复不同。主仓 CHG-129 对应终端焦点修复，CHG-130 对应 0.1.101 发布，本文资料整合对应 CHG-131。

[源分支已闭环变更](../../.spectree/proposals/archive/CHG-131/source-worktree-CHG-129.change.md)和[源分支原提案](../../.spectree/proposals/archive/CHG-131/source-worktree-CHG-129.json)按 Git 原字节保存；历史内容仅作溯源，当前实施以主仓 specs/、changes/ 及本轮门禁为准。
