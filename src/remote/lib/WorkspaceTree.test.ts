import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./WorkspaceTree.svelte', import.meta.url), 'utf8');

describe('remote workspace popup safe-area contract', () => {
  it('clears the larger coarse-pointer action bar', () => {
    expect(source).toContain('bottom:calc(48px + env(safe-area-inset-bottom,0px) + 8px)');
    expect(source).toContain('@media (pointer: coarse)');
    expect(source).toContain('.tree-popup,.saved-popup{bottom:calc(56px + env(safe-area-inset-bottom,0px) + 8px)}');
  });

  it('keeps pane rows keyed to the runtime Agent state', () => {
    expect(source).toContain("class:agent-working={pane.agentState === 'busy'");
    expect(source).toContain("class:agent-starting={pane.agentState === 'starting'}");
    expect(source).toContain('.pane-row.agent-idle .pane-dot');
  });
});
