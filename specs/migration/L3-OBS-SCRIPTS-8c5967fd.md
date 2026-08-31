---
id: L3-OBS-SCRIPTS-8c5967fd
level: L3
parent: L2-OBS-SCRIPTS-8c5967fd
title: scripts module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - scripts/build-remote-desktop.mjs
  - scripts/build-ridge-mcp-sidecar.mjs
  - scripts/build-ridge.mjs
  - scripts/build-validate.mjs
  - scripts/cdp-agent-autodiscover.mjs
  - scripts/cdp-agent-panel-ui.mjs
  - scripts/cdp-cell-graphics-e2e.mjs
  - scripts/cdp-cloud-full-e2e.mjs
  - scripts/cdp-cloud-seed.mjs
  - scripts/cdp-cross-volume-acl-e2e.mjs
  - scripts/cdp-cross-volume-e2e.mjs
  - scripts/cdp-dirchildren-probe.mjs
  - scripts/cdp-dpr-e2e.mjs
  - scripts/cdp-enable-remote.mjs
  - scripts/cdp-freeze-detail.mjs
  - scripts/cdp-get-totp.mjs
  - scripts/cdp-lan-probe.mjs
  - scripts/cdp-multitab-freeze.mjs
  - scripts/cdp-pane-graph.mjs
  - scripts/cdp-port.mjs
  - scripts/cdp-pty-parsers.mjs
  - scripts/cdp-pty-state.mjs
  - scripts/cdp-remote-mobile-agents.mjs
  - scripts/cdp-smoke.mjs
  - scripts/cdp-sonar-status.mjs
  - scripts/cdp-teammate-e2e.mjs
  - scripts/cdp-teammate-mcp-e2e.mjs
  - scripts/cdp-term-input.mjs
  - scripts/cdp-term-render-e2e.mjs
  - scripts/cdp-wait-and-enable.mjs
  - scripts/check-capability-matrix.mjs
  - scripts/check-desktop-only-hosts.mjs
  - scripts/check-prod-status.mjs
  - scripts/check-release-version.mjs
  - scripts/check-user-rail-gates.mjs
  - scripts/copy-teammate-shim.mjs
  - scripts/dev-cli.sh
  - scripts/dev-wind.sh
  - scripts/dev-with-cloud.sh
  - scripts/ensure-teammate-shim.mjs
  - scripts/gen-remote-icons.mjs
  - scripts/generate-review-pack.mjs
  - scripts/mobile-keyboard-e2e.mjs
  - scripts/normalize-lcov.mjs
  - scripts/post-build-rename.mjs
  - scripts/prune-stale-fonts.mjs
  - scripts/publish-remote-cloud.mjs
  - scripts/rdg-gap-report.mjs
  - scripts/rdg-remote-e2e.mjs
  - scripts/remote-cleanup.mjs
  - scripts/remote-gc-e2e.mjs
  - scripts/remote-leak-trace.mjs
  - scripts/remote-runtime-last-error-attribution.mjs
  - scripts/remote-state-e2e.mjs
  - scripts/remote-verify-fix.mjs
  - scripts/run-weaknet-lab.mjs
  - scripts/start-all.sh
  - scripts/start-remote-dev.mjs
  - scripts/start-vite-dev.mjs
  - scripts/sync-generated-csp.mjs
  - scripts/sync-signaling.mjs
  - scripts/tauri-build-debug.mjs
  - scripts/tauri-build.mjs
  - scripts/tauri-dev-cdp-env.mjs
  - scripts/tauri-dev-cdp.mjs
  - scripts/tauri-dev-with-cloud.mjs
  - scripts/teammate-tmux-smoke.sh
  - scripts/validate-remote-smoke-evidence.mjs
  - scripts/verify-remote-pwa-build.mjs
public_interface:
  - export async function collectEvidence({ args = [], env = process.env,
    fetchImpl = fetch, now = new Date()
  - export async function emit(size, scale, file, { outDir, sharpImpl = sharp,
    io = console } = {})
  - export async function main()
  - export async function main(args = process.argv.slice(2)
  - export async function main({ args = process.argv.slice(2)
  - export async function main({ env = process.env, args = process.argv.slice(2)
  - export async function main({ repo = srcRepo, exists = existsSync, io =
    console } = {})
  - export async function main({ rootDir = root, outDir = path.resolve(rootDir,
    'src/remote/public')
  - export async function probe(baseUrl, path, headers = {}, fetchImpl = fetch)
  - export const die = (msg) =>
  - export function applyKernelBreakawayPolicy(env, allowHarnessFallback = false)
  - export function buildDebugPlan(envSource = process.env, platform =
    process.platform, spawnSyncImpl = spawnSync)
  - export function buildGapReport(matrix)
  - export function buildPlan(envSource = process.env, platform =
    process.platform, spawnSyncImpl = spawnSync)
  - export function classifyAttribution(runs)
  - export function cloudBrowserNetworkArgs(baseDomain = '')
  - export function cloudHostResolverRule(baseDomain = '')
  - export function collectCommits(git, range = RANGE)
  - export function copyTeammateShim({ rootDir = root, platform =
    process.platform, fsImpl = { existsSync, mkdirSync, copyFileSync }, io =
    console } = {})
  - export function discoverExtensions({ extensionPaths = [], extensionsRoot =
    '' } = {})
  - export function hasBin(name, spawnSyncImpl = spawnSync, platform =
    process.platform)
  - export function hostTriple()
  - export function isTeammateShimStale({ rootDir = root, platform =
    process.platform, fsImpl = { existsSync, statSync } } = {})
  - export function main(args = process.argv.slice(2)
  - export function main(repoRoot = root, io = console)
  - export function main(reportPath = path.resolve(process.env.LCOV_PATH ||
    'coverage/lcov.info')
  - export function main(rootDir = root)
  - export function main({ env = process.env, spawnImpl = spawn, io = console,
    onSignal = process.on } = {})
  - export function main({ envSource = process.env, platform = process.platform,
    spawnImpl = spawn, spawnSyncImpl = spawnSync, fsImpl = fs, rootDir = root,
    io = console, now = Date.now } = {})
  - export function main({ envSource = process.env, platform = process.platform,
    spawnImpl = spawn, spawnSyncImpl = spawnSync, io = console, now = Date.now }
    = {})
  - export function main({ rootDir = root, execFileSyncImpl = execFileSync,
    fsImpl = { mkdirSync, writeFileSync }, io = console } = {})
  - export function main({ rootDir = root, fsImpl = { readFileSync,
    writeFileSync, mkdirSync }, io = console } = {})
  - export function main({ rootDir = root, platform = process.platform, fsImpl =
    { existsSync, statSync }, exec = execSync, io = console } = {})
  - export function main({ spawnImpl = spawn, prune = pruneOutputs, syncCsp =
    syncGeneratedCspFile, io = console } = {})
  - export function normalizeLcov(report)
  - export function parseArgs(args = [])
  - export function parseArgs(argv)
  - export function pruneOutputs({ dirs = SCAN_DIRS, fsImpl, io = console } = {})
  - export function readDevToolsActivePort()
  - export function renameArtifacts({ rootDir = root, fsImpl = fs, io = console
    } = {})
  - export function renameDebugArtifacts(baseDomain, profDir, bundleList, {
    rootDir = root, fsImpl = fs, io = console } = {})
  - export function renderReviewGuide(commits, range = RANGE)
  - export function resolveCdpPort()
  - export function resolveDevUserDataDir(configured =
    process.env.RIDGE_CDP_USER_DATA_DIR)
  - export function runGates({ rootDir = root, args = [], env = process.env,
    fsImpl = { existsSync, readFileSync }, io = console } = {})
  - export function shouldAnnounceCdpPort(port, previousPort)
  - export function shouldRequestPaneList(summary, state)
  - export function sidecarPaths(forTarget)
  - export function sourceError(repo = srcRepo, exists = existsSync)
  - export function syncGeneratedCsp(html, options = {})
  - export function syncGeneratedCspFile(filePath, fsImpl = fs)
  - export function validateBuildArtifacts({ root = REPO_ROOT, args = [], fs = {
    existsSync, readFileSync }, io = console, } = {})
  - export function validateCapabilityMatrix({ matrix, rustAllow, io = console }
    = {})
  - export function validateDesktopOnlyHosts({ desktop, rustAllow, tsAllow, io =
    console } = {})
  - export function validateEvidence(value, evidencePath, checkAttachments =
    true)
  - export function validateVersions(versions)
  - export function versionSet(rootDir = root)
  - export function viteArgs(env = process.env)
  - export function walk(dir, out, fsImpl)
---

# scripts module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
