import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');

describe('LAN Remote E2E process ownership fence', () => {
  const lanProbe = readFileSync(resolve(root, 'scripts/rdg-remote-e2e.mjs'), 'utf8');
  const keyboardProbe = readFileSync(resolve(root, 'scripts/mobile-keyboard-e2e.mjs'), 'utf8');

  it('isolates default status files per probe process', () => {
    expect(lanProbe).toContain('lan-host-status-e2e-${process.pid}.json');
    expect(keyboardProbe).toContain('lan-host-status-mobile-e2e-${process.pid}.json');
  });

  it('requires the spawned port and PID before browsing', () => {
    expect(lanProbe).toContain('waitStatus(45_000, port, hostHandle.pid)');
    expect(keyboardProbe).toContain('waitReady(45_000, port, host.pid)');
    expect(lanProbe).toContain('Number(st.port) === expectedPort');
    expect(lanProbe).toContain('Number(st.pid) === expectedPid');
    expect(keyboardProbe).toContain('Number(status.port) === expectedPort');
    expect(keyboardProbe).toContain('Number(status.pid) === expectedPid');
  });

  it('drives a real terminal input surface before accepting the matrix', () => {
    expect(lanProbe).toContain('.rg-ime-helper');
    expect(lanProbe).toContain('inputSent');
    expect(lanProbe).toContain('resizeSent');
  });

  it('removes probe status files during teardown', () => {
    expect(lanProbe).toContain('unlinkSync(STATUS_FILE)');
    expect(keyboardProbe).toContain('unlinkSync(STATUS_FILE)');
  });
});
