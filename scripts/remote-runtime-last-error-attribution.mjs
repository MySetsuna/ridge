#!/usr/bin/env node
/**
 * Remote mobile runtime.lastError attribution probe.
 *
 * This probe never filters or rewrites browser Console output.  It compares a
 * fresh incognito context with one fresh persistent profile per extension.
 * A clean profile is necessary but insufficient evidence: when no extension
 * directories are supplied the result is explicitly `attributionComplete:
 * false`, so it cannot be used to blame (or clear) a third-party extension.
 *
 * Usage:
 *   node scripts/remote-runtime-last-error-attribution.mjs \
 *     --url https://127.0.0.1:9527/?ui=mobile \
 *     --extensions-root C:\\path\\to\\unpacked-extensions
 *
 * The target URL may be a real phone-facing Remote URL.  Authentication is
 * intentionally not automated here; the probe records the page state while
 * the operator completes any gate.  No credentials are read or persisted.
 */

import { chromium } from '@playwright/test';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const RUNTIME_LAST_ERROR =
  'Unchecked runtime.lastError: A listener indicated an asynchronous response by returning true, but the message channel closed before a response was received';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_URL = process.env.RIDGE_URL || 'https://127.0.0.1:9527/?ui=mobile';
const DEFAULT_DURATION_MS = 8_000;

function usage() {
  return [
    'Usage: node scripts/remote-runtime-last-error-attribution.mjs [options]',
    '',
    '  --url <url>                    Remote URL (default RIDGE_URL or localhost)',
    '  --extension <dir>              Unpacked extension; repeat for one-at-a-time A/B',
    '  --extensions-root <dir>        Discover child directories containing manifest.json',
    '  --duration-ms <n>               Observe each profile for n milliseconds',
    '  --output <file>                JSON evidence path',
    '  --headed                       Run extension profiles headed (useful for phone-like CDP)',
    '  --help                         Show this help',
  ].join('\n');
}

export function parseArgs(argv) {
  const out = {
    url: DEFAULT_URL,
    extensionPaths: [],
    extensionsRoot: '',
    durationMs: DEFAULT_DURATION_MS,
    output: join(ROOT, '.iteration', 'artifacts', 'remote-runtime-last-error-attribution.json'),
    headed: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      out.help = true;
      continue;
    }
    if (arg === '--headed') {
      out.headed = true;
      continue;
    }
    const [flag, inline] = arg.split('=', 2);
    const value = inline ?? argv[++i];
    if (!value || value.startsWith('--')) throw new Error(`${flag} requires a value`);
    if (flag === '--url') out.url = value;
    else if (flag === '--extension') out.extensionPaths.push(value);
    else if (flag === '--extensions-root') out.extensionsRoot = value;
    else if (flag === '--duration-ms') out.durationMs = Math.max(250, Number(value) || DEFAULT_DURATION_MS);
    else if (flag === '--output') out.output = value;
    else throw new Error(`unknown option: ${arg}`);
  }
  return out;
}

function extensionManifest(path) {
  try {
    const manifest = JSON.parse(readFileSync(join(path, 'manifest.json'), 'utf8'));
    return manifest && typeof manifest === 'object' ? manifest : null;
  } catch {
    return null;
  }
}

function resolveExtensionRoot(path) {
  if (existsSync(join(path, 'manifest.json'))) return path;
  // Chrome/Edge profile roots use <extension-id>/<version>/manifest.json.
  // Resolve exactly one version level; never crawl a user's whole profile.
  try {
    const version = readdirSync(path, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => join(path, entry.name))
      .find((candidate) => existsSync(join(candidate, 'manifest.json')));
    return version || null;
  } catch {
    return null;
  }
}

/** Return stable, deduplicated unpacked extension paths. Invalid paths remain
 * in the list so the run can report an explicit load failure instead of
 * silently dropping an operator's requested candidate. */
export function discoverExtensions({ extensionPaths = [], extensionsRoot = '' } = {}) {
  const candidates = [...extensionPaths];
  if (extensionsRoot && existsSync(extensionsRoot)) {
    for (const entry of readdirSync(extensionsRoot, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const path = join(extensionsRoot, entry.name);
      const resolvedPath = resolveExtensionRoot(path);
      if (resolvedPath) candidates.push(resolvedPath);
    }
  }
  const seen = new Set();
  return candidates
    .map((path) => resolveExtensionRoot(resolve(path)) || resolve(path))
    .filter((path) => !seen.has(path) && (seen.add(path), true))
    .sort((a, b) => a.localeCompare(b));
}

function redactText(value) {
  return String(value)
    .replace(/(https?:\/\/[^\s?]+[?&](?:token|code|totp)=)[^&\s]+/gi, '$1<redacted>')
    .replace(/((?:token|code|totp|password|secret)\s*[:=]\s*)[^\s,;]+/gi, '$1<redacted>');
}

function isRuntimeLastError(value) {
  return String(value).includes(RUNTIME_LAST_ERROR);
}

/**
 * Classify only evidence actually collected.  Baseline warnings always win:
 * an extension is not blamed when the clean profile already emits the error.
 */
export function classifyAttribution(runs) {
  const baseline = runs.find((run) => run.kind === 'clean-profile');
  const candidates = runs.filter((run) => run.kind === 'extension');
  const baselineCount = baseline?.runtimeLastErrorCount ?? 0;
  const extensionLoadFailures = candidates.filter((run) => !run.extensionLoaded);
  const extensionWarnings = candidates.filter((run) => run.runtimeLastErrorCount > 0);
  if (!baseline) {
    return {
      status: 'missing-clean-baseline',
      attributionComplete: false,
      ok: false,
    };
  }
  if (baselineCount > 0) {
    return {
      status: 'warning-in-clean-profile',
      attributionComplete: candidates.length > 0 && extensionLoadFailures.length === 0,
      ok: false,
    };
  }
  if (extensionLoadFailures.length > 0) {
    return {
      status: 'extension-load-unverified',
      attributionComplete: false,
      ok: false,
    };
  }
  if (extensionWarnings.length > 0) {
    return {
      status: 'third-party-extension-candidate',
      attributionComplete: candidates.length > 0,
      ok: false,
      candidates: extensionWarnings.map((run) => run.extensionPath),
    };
  }
  return {
    status: candidates.length > 0 ? 'no-warning-in-clean-or-tested-extensions' : 'clean-profile-only',
    attributionComplete: candidates.length > 0,
    ok: true,
  };
}

function mobileContextOptions() {
  return {
    ignoreHTTPSErrors: true,
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
    userAgent:
      'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 '
      + '(KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  };
}

async function observeProfile({ url, durationMs, extensionPath, headed }) {
  const isExtension = Boolean(extensionPath);
  const profileDir = mkdtempSync(join(tmpdir(), 'ridge-runtime-last-error-'));
  const args = ['--no-proxy-server', '--ignore-certificate-errors'];
  if (isExtension) {
    args.push(`--disable-extensions-except=${extensionPath}`);
    args.push(`--load-extension=${extensionPath}`);
  } else {
    args.push('--disable-extensions', '--disable-component-extensions-with-background-pages');
  }
  let browser;
  let context;
  const logs = [];
  const pages = [];
  const attachPage = (page) => {
    if (pages.includes(page)) return;
    pages.push(page);
    page.on('console', (message) => {
      logs.push({ kind: 'console', type: message.type(), text: redactText(message.text()), location: message.location() });
    });
    page.on('pageerror', (error) => {
      logs.push({ kind: 'pageerror', type: 'error', text: redactText(error.message) });
    });
  };
  let navigation = null;
  try {
    if (isExtension) {
      // Persistent context is required for Chromium to load unpacked extensions.
      // Each extension gets a distinct temporary profile and no sibling extension.
      context = await chromium.launchPersistentContext(profileDir, {
        ...mobileContextOptions(),
        headless: !headed,
        args,
      });
    } else {
      browser = await chromium.launch({
        headless: true,
        args,
      });
      context = await browser.newContext(mobileContextOptions());
    }
    context.on('page', attachPage);
    const page = context.pages()[0] || await context.newPage();
    attachPage(page);
    try {
      const response = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
      navigation = { status: response?.status() ?? 0, url: redactText(page.url()) };
    } catch (error) {
      navigation = { status: 0, url: redactText(page.url()), error: redactText(error?.message || error) };
    }
    await new Promise((resolve) => setTimeout(resolve, durationMs));
  } catch (error) {
    navigation = { status: 0, url: redactText(url), error: redactText(error?.message || error) };
  }
  const extensionWorkers = context?.serviceWorkers?.()
    ?.map((worker) => redactText(worker.url()))
    .filter((workerUrl) => workerUrl.startsWith('chrome-extension://')) || [];
  const extensionPages = context?.pages?.()
    ?.map((page) => redactText(page.url()))
    .filter((pageUrl) => pageUrl.startsWith('chrome-extension://')) || [];
  const runtimeLastErrors = logs.filter((entry) => isRuntimeLastError(entry.text));
  const result = {
    kind: isExtension ? 'extension' : 'clean-profile',
    extensionPath: extensionPath || null,
    extensionName: extensionPath ? extensionManifest(extensionPath)?.name || basename(extensionPath) : null,
    extensionLoaded: !isExtension || extensionWorkers.length > 0 || extensionPages.length > 0,
    extensionWorkers,
    extensionPages,
    navigation,
    browser: isExtension ? `Chromium persistent profile (${headed ? 'headed' : 'headless'})` : 'Chromium incognito clean profile',
    runtimeLastErrorCount: runtimeLastErrors.length,
    runtimeLastErrors,
    consoleEntries: logs,
  };
  await context?.close().catch(() => {});
  await browser?.close().catch(() => {});
  rmSync(profileDir, { recursive: true, force: true });
  return result;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }
  const extensions = discoverExtensions(options);
  const runs = [await observeProfile({ url: options.url, durationMs: options.durationMs, headed: options.headed })];
  // Deliberately sequential: never let two extension profiles overlap, or a
  // warning from one extension could be misattributed to another.
  for (const extensionPath of extensions) {
    runs.push(await observeProfile({
      url: options.url,
      durationMs: options.durationMs,
      extensionPath,
      headed: options.headed,
    }));
  }
  const classification = classifyAttribution(runs);
  const evidence = {
    at: new Date().toISOString(),
    url: redactText(options.url),
    probe: 'remote-runtime-last-error-attribution',
    warning: RUNTIME_LAST_ERROR,
    extensionCount: extensions.length,
    extensionPaths: extensions.map((path) => redactText(path)),
    classification,
    runs,
    limitations: [
      'This is browser automation, not physical iOS/Android evidence.',
      'A clean profile without extension runs cannot attribute a third-party source.',
      'Console entries are captured verbatim (apart from credential redaction); no warning is suppressed.',
    ],
  };
  const output = resolve(options.output);
  const outputDir = dirname(output);
  // The caller controls --output; keep the default inside the iteration artifact tree.
  if (!outputDir.startsWith(ROOT)) throw new Error(`refusing evidence path outside repository: ${output}`);
  mkdirSync(outputDir, { recursive: true });
  writeFileSync(output, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(JSON.stringify({ evidence: output, ...classification }));
  if (!classification.ok) process.exitCode = 1;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  await main();
}
