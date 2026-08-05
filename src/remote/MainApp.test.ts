import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./MainApp.svelte', import.meta.url), 'utf8');

describe('remote Agent attention monitor', () => {
  it('keeps live roster attention polling while the drawer is closed', () => {
    expect(source).toContain("const RemoteTeamRoster = import('./lib/SidebarTeamRoster.svelte');");
    expect(source).toContain("ui.sidebarTab !== 'team'");
    expect(source).toContain('class="agent-attention-monitor"');
    expect(source).toContain('onAttentionChange={updateAgentAttention}');
  });

  it('routes mobile terminal paths through the file viewer without opening the keyboard', () => {
    expect(source).toContain("ridge:remote-open-text-link");
    expect(source).toContain('openFileViewer(path, line);');
    expect(source).toContain('remotePerfStart(\'pane-switch\'');
    expect(source).toContain('onFirstPaint={markPaneFirstPaint}');
  });

  it('frame-budgets pane output so input events keep the main thread turn', () => {
    expect(source).toContain("import { PaneFeedScheduler } from './lib/paneFeedScheduler';");
    expect(source).toContain('paneFeedScheduler.enqueue(key, data);');
    expect(source).toContain('paneFeedScheduler.setActive(subscriptionKey);');
    expect(source).toContain('paneFeedScheduler.clearAll();');
  });
});
