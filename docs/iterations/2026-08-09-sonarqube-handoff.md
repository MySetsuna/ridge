# SonarQube 交接（2026-08-09）

## 地址与账户

- 地址：`http://127.0.0.1:9000`
- 版本：`26.7.0.124771`
- 账户：`admin`
- 密码：由当前用户管理；不写入仓库、日志或交接文档。
- 当前为本机默认账户，仅适用于本地开发；若开放局域网或公网，须立即改密并停用默认凭据。

最新已用 SonarQube 表单登录验证成功（HTTP 200），并用会话读取项目指标（HTTP 200）。旧默认凭证已失效；API 操作请用下文的会话方式或临时 token。浏览器页未保留登录会话；手动接管时打开地址，使用上述账户与当前密码登录。

## 扫描

项目配置：`sonar-project.properties`。

```powershell
$env:SONAR_TOKEN = '<临时 token>'
& .tools/sonar-scanner-8.0.1.6346-windows-x64/bin/sonar-scanner.bat `
  '-Dsonar.scanner.skipJreProvisioning=true'
```

当前扫描器使用本机 Java，避免访问 SonarCloud JRE 下载端点。临时 token 由管理员 API 生成，扫描完成后撤销；本交接文档不保存 token。

## 当前结果

> 本节记录较早的特定分析快照；当前全项目扫描与 Quality Gate 结论以文末 `Latest verified scan` 为准。

- 最新分析：`0822e35f-8dea-48fb-9e85-df88e9903b98`
- 最新上传任务：`d9dacd28-10b4-4dd5-938f-fd4c60061d94`
- 上传：成功，scanner exit `0`
- 分支：`main`
- New Code：特定分析基线 `a0a17a14-89a9-42ee-a3dd-7009557a7357`
- Quality Gate：`OK`
- 新代码违规：`0`
- 当前总覆盖率：`5.9%`
- 当前分支覆盖率：`26.2%`
- 当前总违规：`337`
- 最新本地 Vitest 回归：`157` files，`1622` passed，`1` skipped；V8 Statements `43.11%`，Branches `38.25%`，Functions `43.71%`，Lines `45.54%`。Sonar 数字仍以上述最新成功分析为准，未把本地新跑结果冒充新 Sonar 分析。

`Quality Gate=OK` 依赖“特定分析”新代码基线，表示当前批次无新增违规；不等于历史总覆盖率已达 80%。扫描仍有两类告警：`src/lib/stores/paneTree.ts` 存在 UTF-8 字符告警，LCOV 有 3 个路径无法解析；后续应单独修正编码与路径映射。

本轮 SCM 改动后的 scanner 复扫曾因本机 scanner 阶段超过 180 秒而中止，Sonar CE 未产生新任务；已清理本次 scanner 进程树。故上列质量数字仍以最新成功分析为准，不将超时误报为新分析成功。

## 常用 API

```powershell
$session = $null
Invoke-WebRequest -UseBasicParsing -Method Post `
  -Uri 'http://127.0.0.1:9000/api/authentication/login' `
  -Body @{ login = 'admin'; password = '<current-password>' } `
  -SessionVariable session | Out-Null

Invoke-RestMethod `
  'http://127.0.0.1:9000/api/qualitygates/project_status?projectKey=MySetsuna_ridge&branch=main' `
  -WebSession $session
```

不要把 token、Cookie 或密码写入仓库、日志、截图或提交记录。

## Latest verified scan (2026-08-09 17:12 +08:00)

- Scanner task: `0e8ef213-f7ed-4d05-8dbf-842da76d2dd2`
- Analysis: `09735c32-b9e6-4fc8-9054-3891f4f44795`
- Full project scan: succeeded in `43737ms`; scan did not narrow `sonar.sources`.
- LCOV fix: `scripts/normalize-lcov.mjs` plus `pnpm test:coverage:sonar` converts Windows `SF:` separators to repository-relative `/` paths. `packages/remote/src/shared/reconnectPolicy.ts` is now `100.0%` in Sonar.
- Project measures: coverage `40.3%`, line coverage `41.2%`, branch coverage `38.9%`, violations `841`.
- Quality Gate: `ERROR` because `new_coverage=47.6%` and `new_violations=133`; the required project coverage `>=80%` is not met. This is an open quality gap, not a successful baseline.

## Post-DTO local verification

- Full Vitest: `164` files，`1649` passed，`1` skipped；`svelte-check`：`0 errors and 0 warnings`。
- 本次未重新上传 Sonar 分析；上述 `Latest verified scan` 仍为当前可靠 Sonar 结果，覆盖率目标仍未达成。

## Current local coverage run (2026-08-09)

- Full V8 coverage run: `164` files, `1650` passed, `1` skipped.
- Local metrics: Statements `43.65%`, Branches `38.79%`, Functions `44.54%`, Lines `46.19%`.
- `scripts/normalize-lcov.mjs` returned `{"ok":true}` and normalized `coverage/lcov.info`. No new authenticated Sonar upload was performed; server-side `40.3%` and Quality Gate `ERROR` remain authoritative.
- Sonar handoff remains `http://127.0.0.1:9000`, account `admin`, password managed by the user; no token or cookie is stored in the repository.

## Latest credential and monitor recheck (2026-08-09 19:00 +08:00)

- 本地服务仍为 UP，版本 `26.7.0.124771`。
- `admin` + 当前用户密码表单登录成功；会话校验 `valid=true`，项目指标 API 返回 HTTP 200。
- 生成临时扫描 token 时必须携带登录响应的 `XSRF-TOKEN` 作为 `X-XSRF-TOKEN` 请求头；本次临时 token 已在任务结束后撤销，仓库未保存 token、Cookie 或会话。
- 使用新临时 token 的 scanner 尝试超过 `180s` 未完成，已回收 scanner 进程树；Sonar 未产生新 CE 任务，故不得把本次尝试称为成功扫描。
- 当前服务端仍为上一次可靠分析：coverage `40.3%`、line `41.2%`、branch `38.9%`、violations `841`；Quality Gate `ERROR`，`new_coverage=47.6%`、`new_violations=133`。
- 本会话无可用 Browser 实例，无法真实打开并截图 Sonar 监控页；手动接管地址仍为 `http://127.0.0.1:9000`。

## Post-change runtime acceptance

- After the E2E harness fixes, `tauri:dev:cdp` passed DPR, pane graph, multitab, mobile Remote, cross-volume move, ACL rejection/restoration, and Cloud/Postgres checks. This does not change the last Sonar server analysis because the fresh scanner attempt timed out before submitting a CE task.

## Latest local coverage command

- `pnpm test:coverage:sonar` passed with serialized coverage processing (`--coverage.processingConcurrency=1 --no-file-parallelism`) and LCOV normalization.
- Local V8: Statements `52.17%`, Branches `46.88%`, Functions `54.03%`, Lines `55.32%`; `165` files, `1660` passed, `1` skipped.
- Sonar server remains at the last successful analysis (`40.3%` coverage, Quality Gate `ERROR`); no new upload is claimed.

## Latest scanner timeout recheck (2026-08-09)

- A fresh authenticated scanner attempt exceeded the outer `364s` limit and exited `124`; no CE task or new analysis was created.
- The exact scanner process tree was terminated. Sonar service Java processes were left running; actual scanner Java count is now `0`.
- The temporary `codex-local-scan-*` token created for this attempt was revoked; remaining temporary token count is `0`.
- Server-side measures therefore remain the last successful analysis: coverage `40.3%`, line coverage `41.2%`, branch coverage `38.9%`, violations `841`, Quality Gate `ERROR`.
- Treat the scanner timeout as an open host/integration issue. Do not claim the current local `52.17%` coverage as a Sonar upload.

## Browser monitor recheck (2026-08-09)

- `admin` + 当前用户密码表单登录与项目指标 API 仍可用；当前服务端指标仍为 coverage `40.3%`、line `41.2%`、branch `38.9%`、violations `841`，Quality Gate `ERROR`。
- 本轮按浏览器控制规范尝试连接本机监控页 `http://127.0.0.1:9000`；连接器返回 `No browser is available`，可用浏览器列表为空，故未声称已打开 UI、截图或完成视觉监控验收。
- 手动接管：浏览器打开上述地址，登录 `admin` 与当前密码；登录后进入项目 `Ridge`（project key `MySetsuna_ridge`）查看 Dashboard、Issues、Measures、Quality Gate。联网前继续使用非默认、仅本机可用的密码。

## Host disk / scanner incident (2026-08-10)

- A fresh authenticated upload failed in CE task `5c57440f-6f4b-40a7-8568-dec11c91e6af`: Elasticsearch flood-stage watermark made `projectmeasures` read-only. Server `es.log` confirmed the local data path reached about `4.1%` free.
- Generated `C:\code\wind\target\debug` was moved in recoverable portions to `D:\wind-target-debug-archive-20260810`; once free space crossed the watermark, Elasticsearch logged that it released the read-only blocks. This archive is build cache, not source.
- Keep `sonar.typescript.tsconfigPaths=tsconfig.json` in project config to avoid transient `.claude/worktrees` TSConfig discovery. A complete retry after disk recovery still exceeded the host bound in Rust analysis after cache relocation; exact scanner trees and temporary tokens were cleaned.
- Do not treat the failed CE task or local V8/Rust LCOV percentages as a passed Gate. Accepted server baseline remains coverage `48.5%`, line `49.4%`, branch `47.0%`, violations `835`, Quality Gate `ERROR`; target `>=80%` remains open.

## Current monitor recheck (2026-08-10)

- SonarQube `http://127.0.0.1:9000` remains healthy: version `26.7.0.124771`, status `UP`.
- The current project API reports coverage `56.7%`, line coverage `57.0%`, branch coverage `54.2%`, violations `835`; Quality Gate `ERROR`, with `new_coverage=64.9%` and `new_violations=130`.
- Latest analysis record observed by API: `3c585f9e-554f-4a4e-81e8-2e21a387c707`, `2026-08-10T02:05:52+0800`. No scanner exit or CE task was inferred from this monitor-only recheck.
- Local V8 single-worker coverage is now Lines `67.83%`, still below the explicit `>=80%` project target. Historical Rust LCOV is not treated as current evidence.
- After the final bootstrap/CDP test additions, the single-worker local run is Statements `65.49%`, Branches `57.95%`, Functions `66.74%`, Lines `69.23%`; full Vitest is `193` files / `1798` passed / `1` skipped. Sonar server metrics are unchanged by this local-only run.

## Remote communication coverage recheck (2026-08-10)

- `manager.attach.test.ts` and the new `wsRemote.behavior.test.ts` focused regression passed `27/27`; `pnpm check` passed `0 errors / 0 warnings`.
- Latest single-worker local V8 run: statements `66.38%` (`12307/18538`), branches `58.65%` (`6772/11546`), functions `68.01%` (`2399/3527`), lines `70.22%` (`11118/15832`). Statements need `2524` more covered entries to reach local `80%`.
- This is local evidence only. Sonar project remains `56.7%` coverage with Quality Gate `ERROR`; no new scanner/CE success was observed. Existing `.mjs` remap parse warnings remain open.
- Final non-coverage regression passed: `pnpm test` reported `195` files, `1810 passed / 1 skipped`; `node --check scripts/*.mjs` passed for every script.

## Host onboarding coverage wave (2026-08-10)

- Added deterministic LAN/Cloud host connection coverage; focused `hosts.connect.test.ts` passed `3/3`, including success, transport error, progress, and topology projection paths.
- Latest local V8: statements `66.54%` (`12337/18538`), branches `58.80%` (`6790/11546`), functions `68.10%` (`2402/3527`), lines `70.40%` (`11147/15832`). Local statements remain `2494` short of `80%`.
- Sonar project status remains unchanged; no new scanner/CE success or Quality Gate pass is claimed.

## Latest authenticated scan (2026-08-09)

- Scanner exit `0`；CE task `5418a229-2bc5-4b7f-941e-b6f9bbf59672` 最终 `SUCCESS`，analysis `3ee09772-8826-4b77-8a44-a1f53227a2ad`。
- Server measures：coverage `48.5%`、line `49.4%`、branch `47.0%`、violations `835`。Quality Gate 仍为 `ERROR`：`new_coverage=45.5%`、`new_violations=130`；`>=80%` 尚未达标。
- 本次使用 `sonar.scm.disabled=true` 规避 dirty worktree 的 SCM line mismatch；该参数不改变 source/LCOV 范围。扫描日志显示 TS 分析曾发现 `.claude/worktrees` 下的 12 个 tsconfig，耗时约 `444s`。
- 已在 `sonar-project.properties` 增加 `sonar.typescript.tsconfigPaths`，将 TypeScript program discovery 限定到仓库内 4 个 tsconfig；随后验证扫描超过 10 分钟未完成，已回收精确 scanner 子树，Sonar 服务未动。该优化待下轮以 scanner 日志/CE 终态复证，不把它称为新成功分析。
- 本轮临时 `codex-local-scan-*` token 已撤销，API 查询无残留；仓库未保存 token、Cookie 或会话。
- Sonar 规则驱动的低风险 Remote 清理已落地：能力查找改为 `.includes()`，能力集合字段标为 `readonly`，结构化 invoke 错误避免默认 `[object Object]`；相关 focused tests `67/67` 通过。Rust 高复杂度与参数过多历史项未做 speculative 大重构，仍列开放债项。

## Latest verified scan (2026-08-10 Wave71)

- SonarQube `http://127.0.0.1:9000` scanner upload succeeded; latest CE task `3c78dac8-13b1-41a8-a65e-cdce622ee4cd` is `SUCCESS`.
- Project `MySetsuna_ridge` measures: coverage `80.2%`, line coverage `86.4%`, branch coverage `71.6%`, violations `730` (`17` bugs, `21` vulnerabilities, `692` code smells). Quality Gate remains `ERROR` because `new_violations=152` is not zero; do not claim a green gate.
- Standard `pnpm test:coverage:sonar` hit a Windows coverage temp-file race (`ENOENT coverage/.tmp/coverage-196.json`). Accepted local replacement is serialized isolated coverage: statements `74.52%`, branches `66.63%`, functions `76.10%`, lines `78.73%`; normalized LCOV produced successfully.
- The scan still logged missing generated `.svelte-kit/tsconfig.json` references in the isolated copy and no SCM provider. These are recorded as scanner hygiene gaps; external Tauri WebView2 restart attach, physical mobile/PWA/IME, native pixel matrix, mid-window cross-volume ACL injection, and a green full-project Gate remain open.
- Sonar low-risk fixture findings were addressed in `packages/remote/src/shared/cloud/__faultRig.ts`: pane/lifecycle/ICE state is now observable and covered by deterministic tests. `faultInjection.test.ts` plus quality-helper focused tests: `33/33` passed. Temporary scanner tokens are revoked and are not stored in this handoff.

Manual takeover: open the local URL above and sign in as `admin` with the current user-managed password. The password is intentionally not stored in this repository or in scanner logs; change it again before exposing SonarQube beyond localhost. Validate it against `/api/authentication/validate` after any rotation.
