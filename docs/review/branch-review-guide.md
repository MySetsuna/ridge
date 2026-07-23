# 分支审查导读 — `codex/remote-git-diff-iteration-1`

生成：`node scripts/generate-review-pack.mjs`（范围 `origin/main..HEAD`，共 **45** 提交；手改无效，重跑刷新）。

## 审查优先级建议

1. **协议面提交（5）**：动了 allowlist/能力合同/矩阵——远端可达面变化，逐条核。
2. **安全面提交（10）**：hitl/e2ee/totp/trust/suspend 路径。
3. 其余按类型抽查；docs 类可速览。

## 协议面提交清单

- `f30333e` fix(remote): synchronize cloud capability mirror
- `7860ae4` feat(remote): enforce cross-entry capability contract
- `c8f7ead` feat(remote): make the cross-entry capability matrix machine-readable
- `72ef6dd` feat(remote): expose read-only teammate roster across entries
- `2af7dc7` feat(remote): expose sanitized read-only HITL pending list (P2 phase 1)

## feat（11）

| SHA | 标题 | 文件数 | 变更量 | 标注 |
| --- | --- | --- | --- | --- |
| `7860ae4` | **remote** enforce cross-entry capability contract | 12 |  12 files changed, 308 insertions(+), 50 deletions(-) | 协议面 |
| `8f65c59` | **remote** audit cloud fallback surfaces with pinning gates | 4 |  4 files changed, 163 insertions(+), 2 deletions(-) | 安全面 |
| `aedb8e0` | **scripts** add read-only two-line prod status probe | 1 |  1 file changed, 63 insertions(+) | — |
| `c8f7ead` | **remote** make the cross-entry capability matrix machine-readable | 2 |  2 files changed, 184 insertions(+) | 协议面 |
| `72ef6dd` | **remote** expose read-only teammate roster across entries | 15 |  15 files changed, 248 insertions(+), 21 deletions(-) | 协议面 安全面 |
| `db7f3b2` | **remote** count fallback occurrences for S1 retirement gates | 5 |  5 files changed, 27 insertions(+), 1 deletion(-) | 安全面 |
| `2af7dc7` | **remote** expose sanitized read-only HITL pending list (P2 phase 1) | 12 |  12 files changed, 155 insertions(+), 12 deletions(-) | 协议面 安全面 |
| `a1a2f40` | **remote** count S1 F3/F4 fallbacks and retire F5 hook | 9 |  9 files changed, 67 insertions(+), 173 deletions(-) | 安全面 |
| `81a0f59` | **teammate** soft suspend/resume agent input (G1 phase 1) | 10 |  10 files changed, 216 insertions(+), 29 deletions(-) | 安全面 |
| `f2014ed` | **scripts** rdg gap report and branch review pack generators | 4 |  4 files changed, 254 insertions(+) | — |
| `2177e3d` | **teammate** persist suspended panes across restarts (M1 slice 1) | 5 |  5 files changed, 169 insertions(+), 2 deletions(-) | 安全面 |

## fix（6）

| SHA | 标题 | 文件数 | 变更量 | 标注 |
| --- | --- | --- | --- | --- |
| `f30333e` | **remote** synchronize cloud capability mirror | 2 |  2 files changed, 40 insertions(+), 8 deletions(-) | 协议面 |
| `ee9afa3` | **remote** gate reconnect recovery on authorization | 4 |  4 files changed, 468 insertions(+), 14 deletions(-) | — |
| `292adb3` | **remote** pin vendored signaling files to LF | 1 |  1 file changed, 5 insertions(+) | — |
| `ea1f0ae` | **windows** delay-load comctl32 so lib test hosts boot | 2 |  2 files changed, 22 insertions(+) | — |
| `a6dd1b7` | **remote** probe localStorage writability in deviceTrust store | 1 |  1 file changed, 20 insertions(+), 10 deletions(-) | 安全面 |
| `f544d8e` | **workspace** converge close/rename to single core, fix LAN broadcast gap | 3 |  3 files changed, 97 insertions(+), 101 deletions(-) | — |

## refactor（3）

| SHA | 标题 | 文件数 | 变更量 | 标注 |
| --- | --- | --- | --- | --- |
| `b9b4d4f` | **core** drop dead pane-output channel surface | 2 |  2 files changed, 46 insertions(+), 39 deletions(-) | — |
| `5eca329` | **core** single-source the workspace list projection | 2 |  2 files changed, 18 insertions(+), 30 deletions(-) | — |
| `49bcb53` | delete dead pane_tree leaf helpers and parser reframe (A1) | 2 |  2 files changed, 61 deletions(-) | — |

## test（2）

| SHA | 标题 | 文件数 | 变更量 | 标注 |
| --- | --- | --- | --- | --- |
| `d029a65` | **remote** cover watchdog escalation paths | 1 |  1 file changed, 65 insertions(+) | — |
| `8f66e1c` | **remote** add deterministic weak-net lab sweep | 5 |  5 files changed, 490 insertions(+), 222 deletions(-) | — |

## docs（23）

| SHA | 标题 | 文件数 | 变更量 | 标注 |
| --- | --- | --- | --- | --- |
| `6f17196` | **iteration** define remote baseline reconciliation | 5 |  5 files changed, 112 insertions(+) | — |
| `999f3ac` | **iteration** close loop and define capability gate | 5 |  5 files changed, 151 insertions(+), 2 deletions(-) | — |
| `fafbd3e` | **protocol** design cloud SSOT consolidation | 1 |  1 file changed, 34 insertions(+) | — |
| `2c4b7ce` | **protocol** consolidate cloud contract SSOT | 4 |  4 files changed, 38 insertions(+), 418 deletions(-) | — |
| `7215887` | **iteration** close capability gate loop | 5 |  5 files changed, 222 insertions(+), 1 deletion(-) | — |
| `fecb1db` | **remote** design deterministic fault injection gate | 1 |  1 file changed, 74 insertions(+) | — |
| `b32db25` | **iteration** close fault injection loop | 5 |  5 files changed, 173 insertions(+), 1 deletion(-) | — |
| `20b48f6` | **remote** design physical smoke evidence gate | 1 |  1 file changed, 43 insertions(+) | — |
| `6be9173` | **remote** add physical smoke evidence kit | 5 |  5 files changed, 367 insertions(+) | — |
| `2ed74f1` | **nlm** consolidate notebook baseline into single PROJECT-STATE source | 5 |  5 files changed, 2480 insertions(+) | — |
| `f090a94` | **iteration** close iteration 4 loop and contract iteration 5 | 5 |  5 files changed, 143 insertions(+), 2 deletions(-) | — |
| `2cce8aa` | **iteration** close iteration 5 loop and contract iteration 6 | 6 |  6 files changed, 156 insertions(+), 15 deletions(-) | — |
| `b03a793` | **iteration** close iteration 6 loop and contract iteration 7 | 6 |  6 files changed, 141 insertions(+), 10 deletions(-) | — |
| `b5b7da2` | user verification checklist + team panel & dual-track notes | 3 |  3 files changed, 30 insertions(+) | — |
| `a4a5626` | **iteration** record iteration 7 evidence and refresh project state | 3 |  3 files changed, 71 insertions(+), 12 deletions(-) | — |
| `daa69c9` | **iteration** close iteration 7 loop and contract iteration 8 | 3 |  3 files changed, 107 insertions(+) | — |
| `f0a6e55` | **specs** agent suspend/resume cross-platform design (G1) | 1 |  1 file changed, 62 insertions(+) | 安全面 |
| `aca3044` | **iteration** close iteration 8 loop and contract iteration 9 | 6 |  6 files changed, 161 insertions(+), 11 deletions(-) | — |
| `60d1cc1` | **audits** workspace write-path audit + E1/E2 ledger correction | 2 |  2 files changed, 32 insertions(+), 3 deletions(-) | — |
| `96b3612` | **iteration** close iteration 9 loop and contract iteration 10 | 7 |  7 files changed, 158 insertions(+), 14 deletions(-) | — |
| `bf4dd2e` | workspace memory design + rdg gap judgments + review cadence | 4 |  4 files changed, 60 insertions(+), 7 deletions(-) | — |
| `139a17b` | **iteration** close iteration 10 loop and contract iteration 11 | 7 |  7 files changed, 143 insertions(+), 14 deletions(-) | — |
| `d3dc21b` | **specs** remote HITL resolution v2 design (P2 phase 2) | 1 |  1 file changed, 53 insertions(+) | 安全面 |
