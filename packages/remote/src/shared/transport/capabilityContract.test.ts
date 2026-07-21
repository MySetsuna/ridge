import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { HOST_CAPABILITIES as CLOUD_HOST_CAPABILITIES } from '../cloud/cloudHostBridge';
import { REMOTE_ALLOWLIST } from '../cloud/remoteAllowlist';
import {
  REMOTE_CAPABILITY_METHODS,
  getRemotePanelAvailability,
  type RemoteCapability,
} from './capabilityContract';
import { CLIENT_CAPABILITIES } from './rpcClient';

const root = resolve(import.meta.dirname, '../../../../..');

function source(path: string): string {
  return readFileSync(resolve(root, path), 'utf8');
}

function rustStringArray(text: string, name: string): string[] {
  const match = text.match(new RegExp(`(?:pub\\s+)?const\\s+${name}[^=]*=\\s*&\\[([\\s\\S]*?)\\];`));
  if (!match) throw new Error(`Rust array ${name} not found`);
  return [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
}

function requiredMethods(capabilities: readonly string[]): string[] {
  return capabilities.flatMap((capability) =>
    REMOTE_CAPABILITY_METHODS[capability as RemoteCapability] ?? [],
  );
}

describe('cross-entry Remote capability contract', () => {
  const rustCapability = source('packages/ridge-core/src/capability.rs');
  const desktopHost = source('src-tauri/src/remote_host_impl.rs');
  const coreDispatch = source('packages/ridge-core/src/dispatch.rs');
  const cliRpc = source('packages/ridge-cli/src/rpc.rs');
  const mainApp = source('src/remote/MainApp.svelte');
  const remoteSidebar = source('src/remote/lib/RemoteSidebar.svelte');

  const desktopCapabilities = rustStringArray(desktopHost, 'HOST_CAPABILITIES');
  const cliCapabilities = rustStringArray(cliRpc, 'CLI_CAPABILITIES');
  const cliRouteTable = cliRpc.slice(
    cliRpc.indexOf('pub fn route_method'),
    cliRpc.indexOf('#[cfg(test)]'),
  );

  it('keeps LAN desktop, Cloud desktop, and controller capability names aligned', () => {
    expect(desktopCapabilities).toEqual(CLIENT_CAPABILITIES);
    expect(CLOUD_HOST_CAPABILITIES).toEqual(CLIENT_CAPABILITIES);
  });

  it('admits and handles every method required by an advertised desktop capability', () => {
    for (const method of requiredMethods(desktopCapabilities)) {
      expect(REMOTE_ALLOWLIST, `${method} must be remotely admitted`).toContain(method);
      expect(`${desktopHost}\n${coreDispatch}`, `${method} must have a desktop remote handler`).toMatch(
        new RegExp(`"${method}"\\s*=>`),
      );
    }
  });

  it('routes every method required by an advertised rdg capability', () => {
    for (const method of requiredMethods(cliCapabilities)) {
      expect(cliRouteTable, `${method} must be routed by ridge-cli`).toContain(`"${method}"`);
    }
  });

  it('keeps snapshot/diff admitted while host-privileged methods remain excluded', () => {
    expect(REMOTE_ALLOWLIST).toContain('get_workspace_snapshot');
    expect(REMOTE_ALLOWLIST).toContain('git_diff_file');
    expect(REMOTE_ALLOWLIST).not.toContain('get_remote_info');
    expect(REMOTE_ALLOWLIST).not.toContain('get_remote_totp');
    expect(rustCapability).toContain('pub const REMOTE_ALLOWLIST');
  });

  it('derives Files/Git/Search visibility from negotiated capabilities', () => {
    const available = new Set<RemoteCapability>(['pane', 'fs', 'search']);
    expect(getRemotePanelAvailability((capability) => available.has(capability))).toEqual({
      files: true,
      git: false,
      search: true,
    });
  });

  it('wires negotiated capabilities into the controller shell and sidebar', () => {
    expect(mainApp).toContain('ws.onCapabilitiesChanged(refreshCapabilities)');
    for (const panel of ['files', 'git', 'search']) {
      expect(mainApp).toContain(`{#if panelAvailability.${panel}}`);
      expect(remoteSidebar).toContain(`{#if available.${panel}}`);
    }
  });
});
