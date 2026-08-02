import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./SidebarTeamRoster.svelte', import.meta.url), 'utf8');

describe('remote Agent drawer alignment contract', () => {
  it('uses one centered inline-flex baseline for labels and icons', () => {
    expect(source).toContain('.subtabs button{display:inline-flex;align-items:center;justify-content:center;gap:4px');
    expect(source).toContain('.subtabs :global(svg),.approval :global(svg),.member-head :global(svg),.act :global(svg),.msg-row :global(svg){display:block;flex:0 0 auto}');
    expect(source).toContain('.member-head{display:flex;align-items:center;gap:6px;min-height:24px');
  });

  it('keeps agent state visible as a left color rail', () => {
    expect(source).toContain('class:status-working={st.key === \'working\'}');
    expect(source).toContain('.member-card.status-pending{border-left-color:var(--rg-ansi-yellow,#d29922)}');
    expect(source).toContain('.member-card.status-idle{border-left-color:var(--rg-fg-muted)}');
  });

  it('maps each Agent card to its live pane CWD and truncates safely', () => {
    expect(source).toContain('panes.find((pane) => pane.id === m.paneId)?.cwd?.trim()');
    expect(source).toContain('<span class="member-cwd" title={cwd}>{cwd}</span>');
    expect(source).toContain('.member-cwd{display:block;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap');
  });
});
