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
});
