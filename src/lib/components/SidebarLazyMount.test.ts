import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(new URL('../../routes/+page.svelte', import.meta.url), 'utf8');

describe('desktop sidebar mount guards', () => {
  it('does not mount hidden tabs before first visit', () => {
    expect(pageSource).toContain("let sidebarVisited = $state<Set<SidebarTab>>(new Set(['files']))");
    expect(pageSource).toContain("sidebarVisited.has('git')");
    expect(pageSource).toContain("sidebarVisited.has('search')");
    expect(pageSource).toContain("sidebarVisited.has('remote')");
    expect(pageSource).toContain("sidebarVisited.has('agents')");
    expect(pageSource).toContain("sidebarVisited.has('hosts')");
  });

  it('retains visited panels instead of rebuilding them on every tab switch', () => {
    expect(pageSource).toContain("if (!sidebarVisited.has(tab)) sidebarVisited = new Set([...sidebarVisited, tab]);");
    expect(pageSource).toContain("sidebarTab === 'files' ? '' : 'hidden'");
  });

  it('starts pane highlight sync when teammate is enabled without visiting the Agent tab', () => {
    expect(pageSource).toContain("import AgentPaneHighlightSync from '$lib/teammate/AgentPaneHighlightSync.svelte'");
    expect(pageSource).toContain('{#if teammateEnabled}');
    expect(pageSource).toContain('<AgentPaneHighlightSync />');
    expect(pageSource).toContain("sidebarVisited.has('agents')");
    const highlightMount = pageSource.indexOf('<AgentPaneHighlightSync />');
    const agentsVisitGate = pageSource.indexOf("sidebarVisited.has('agents')");
    expect(highlightMount).toBeGreaterThan(0);
    expect(agentsVisitGate).toBeGreaterThan(0);
    expect(pageSource.slice(highlightMount - 80, highlightMount + 80)).not.toContain("sidebarVisited.has('agents')");
  });
});
