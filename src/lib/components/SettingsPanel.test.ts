import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./SettingsPanel.svelte', import.meta.url), 'utf8');
const pageSource = readFileSync(new URL('../../routes/+page.svelte', import.meta.url), 'utf8');
const hostPortsSource = readFileSync(new URL('../terminal/hostPorts.ts', import.meta.url), 'utf8');
const themeBridgeSource = readFileSync(new URL('../../../packages/remote/src/shared/terminal/themeBridge.ts', import.meta.url), 'utf8');

describe('settings panel performance guards', () => {
  it('does not start shell discovery on panel open or blur the terminal surface', () => {
    expect(source).toContain("section === 'terminal'");
    expect(source).toContain("void loadShells()");
    expect(source).not.toContain("if (open) void loadShells()");
    expect(source).not.toContain('backdrop-blur-sm');
  });

  it('defers heavy previews and rejects stale async results', () => {
    expect(source).toContain('scheduleIdle');
    expect(source).toContain('agentLoadGeneration');
    expect(source).toContain('themeUrlGeneration');
    expect(source).toContain('generation !== agentLoadGeneration');
    expect(source).toContain('loading="lazy"');
  });

  it('does not mirror unrelated setting changes into default-cwd RPC', () => {
    expect(pageSource).toContain('queueDefaultCwdSync');
    expect(pageSource).toContain('defaultCwdSyncLastQueued');
    expect(pageSource).not.toContain("void invoke('set_user_default_cwd'");
  });

  it('keeps theme propagation while filtering unrelated terminal settings', () => {
    expect(hostPortsSource).toContain('themeId: s.theme');
    expect(hostPortsSource).toContain('`${s.theme}');
    expect(themeBridgeSource).toContain('pendingPushFrame');
  });
});
