import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./TerminalCanvas.svelte', import.meta.url), 'utf8');

describe('remote pane Agent status chrome contract', () => {
  it('uses visual-only inset rails without changing terminal geometry', () => {
    expect(source).toContain("class:agent-working={agentState === 'busy'}");
    expect(source).toContain("class:agent-starting={agentState === 'starting'}");
    expect(source).toContain("class:agent-idle={agentState === 'idle'}");
    expect(source).toContain('.container.agent-working{box-shadow:inset 0 0 0 2px var(--rg-ansi-green,#3fb950)}');
    expect(source).not.toContain('.container.agent-working{border:');
  });

  it('keeps renderer cursor ownership when the mobile IME sink blurs', () => {
    expect(source).toContain('onfocus={() => manager.setFocused(paneId, true)}');
    expect(source).not.toContain('onblur={() => manager.setFocused(paneId, false)}');
    expect(source).toContain('caret-color:var(--rg-accent,#58a6ff)');
  });
});
