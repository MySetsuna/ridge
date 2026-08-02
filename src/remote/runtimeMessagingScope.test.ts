import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const remoteMainSource = readFileSync(new URL('./main.ts', import.meta.url), 'utf8');
const serviceWorkerSource = readFileSync(new URL('../service-worker.ts', import.meta.url), 'utf8');

function collectRemoteSources(root: string): string[] {
  const sources: string[] = [];
  for (const entry of readdirSync(root)) {
    const path = join(root, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      sources.push(...collectRemoteSources(path));
      continue;
    }
    // Tests intentionally contain the forbidden token strings themselves;
    // exclude them while scanning all shipped Remote implementation files.
    if (/\.(?:test|spec)\.[cm]?[jt]sx?$/.test(entry)) continue;
    if (/\.(?:ts|svelte|js)$/.test(entry)) sources.push(readFileSync(path, 'utf8'));
  }
  return sources;
}

const productSources = [
  remoteMainSource,
  serviceWorkerSource,
  ...collectRemoteSources(fileURLToPath(new URL('.', import.meta.url))),
  ...collectRemoteSources(fileURLToPath(new URL('../../packages/remote/src/', import.meta.url))),
];

describe('Remote runtime messaging scope', () => {
  it('uses service-worker messaging only and does not ship extension messaging APIs', () => {
    const forbidden = [
      'chrome.runtime',
      'chrome.tabs',
      'browser.runtime',
      'sendResponse',
      'runtime.lastError',
    ];
    for (const source of productSources) {
      for (const token of forbidden) expect(source).not.toContain(token);
    }
  });

  it('keeps service-worker messages one-way and lifecycle-scoped', () => {
    expect(remoteMainSource).toContain("navigator.serviceWorker.addEventListener('message'");
    expect(serviceWorkerSource).toContain('sw.clients.matchAll');
    expect(serviceWorkerSource).toContain('client.postMessage');
  });
});
