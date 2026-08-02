import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const remoteMainSource = readFileSync(new URL('./main.ts', import.meta.url), 'utf8');
const serviceWorkerSource = readFileSync(new URL('../service-worker.ts', import.meta.url), 'utf8');
const productSources = [remoteMainSource, serviceWorkerSource];

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
