import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const dialogSource = readFileSync(
  new URL('../components/hosts/HostConnectDialog.svelte', import.meta.url),
  'utf8',
);
const panelSource = readFileSync(
  new URL('../components/hosts/HostsPanel.svelte', import.meta.url),
  'utf8',
);
const storeSource = readFileSync(new URL('../stores/hosts.ts', import.meta.url), 'utf8');

describe('Remote host connection flow', () => {
  it('hands progress to the persistent Hosts panel before starting discovery', () => {
    const closeAt = dialogSource.indexOf('    close();\n');
    const connectAt = dialogSource.indexOf("connectHost('remote'", closeAt);
    expect(closeAt).toBeGreaterThanOrEqual(0);
    expect(connectAt).toBeGreaterThan(closeAt);
    expect(dialogSource).toContain('hostConnectProgress');
    expect(dialogSource).toContain("$hostConnectProgress?.phase !== 'error'");
    expect(panelSource).toContain('{#if $hostConnectProgress}');
    expect(panelSource).toContain('aria-live="polite"');
  });

  it('keeps topology progress, drag attach, and first-size synchronization in one path', () => {
    for (const token of [
      "phase: 'connecting'",
      "phase: 'loading-workspaces'",
      "phase: 'error'",
      'refreshLinkedHost(connectedHostId',
      'loadedWorkspaces',
      'use:hostSessionDrag',
      'attachHostSession',
      'schedulePaneSizeSynchronization(paneId)',
    ]) {
      expect(`${storeSource}\n${panelSource}`).toContain(token);
    }
  });
});
