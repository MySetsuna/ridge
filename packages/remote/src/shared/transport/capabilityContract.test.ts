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

  it('derives Files/Git/Search/Team visibility from negotiated capabilities', () => {
    const available = new Set<RemoteCapability>(['pane', 'fs', 'search', 'teammate']);
    expect(getRemotePanelAvailability((capability) => available.has(capability))).toEqual({
      files: true,
      git: false,
      search: true,
      team: true,
    });
  });

  it('wires negotiated capabilities into the controller shell and sidebar', () => {
    expect(mainApp).toContain('ws.onCapabilitiesChanged(refreshCapabilities)');
    for (const panel of ['files', 'git', 'search', 'team']) {
      expect(mainApp).toContain(`{#if panelAvailability.${panel}}`);
      expect(remoteSidebar).toContain(`{#if available.${panel}}`);
    }
  });

  it('admits the teammate roster read while HITL adjudication stays host-privileged', () => {
    // P2 阶段 1：只读 roster + 脱敏待审批快照两方法；均非 mutating。
    for (const method of ['get_teammate_topology', 'list_hitl_pending']) {
      expect(REMOTE_ALLOWLIST).toContain(method);
      expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).toContain(method);
      expect(rustStringArray(rustCapability, 'MUTATING_METHODS')).not.toContain(method);
    }
    // P2 之前 HITL 裁决与 Agent 配置写路径不得远程可达。
    for (const method of ['resolve_hitl_request', 'set_hitl_enabled']) {
      expect(REMOTE_ALLOWLIST).not.toContain(method);
      expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).not.toContain(method);
    }
    // 三个宣告 teammate 的 host 面 + 不宣告的 rdg 无头 host。
    expect(desktopCapabilities).toContain('teammate');
    expect(CLOUD_HOST_CAPABILITIES).toContain('teammate');
    expect(CLIENT_CAPABILITIES).toContain('teammate');
    expect(cliCapabilities).not.toContain('teammate');
  });
});

describe('docs/capability-matrix.json stays a projection of the canonical declarations (A2)', () => {
  interface MatrixCapability {
    methods: string[];
    cells: Record<string, string>;
  }
  const matrix = JSON.parse(source('docs/capability-matrix.json')) as {
    entries: string[];
    capabilities: Record<string, MatrixCapability>;
    guards: string[];
  };
  const cliCapabilities = rustStringArray(source('packages/ridge-cli/src/rpc.rs'), 'CLI_CAPABILITIES');
  const lanCapabilities = rustStringArray(
    source('src-tauri/src/remote_host_impl.rs'),
    'HOST_CAPABILITIES',
  );

  it('lists exactly the negotiated capability names', () => {
    expect(Object.keys(matrix.capabilities).sort()).toEqual([...CLIENT_CAPABILITIES].sort());
  });

  it('mirrors the controller-minimum methods per capability', () => {
    for (const [capability, entry] of Object.entries(matrix.capabilities)) {
      expect(entry.methods, capability).toEqual(
        REMOTE_CAPABILITY_METHODS[capability as RemoteCapability],
      );
    }
  });

  it('fills every entry cell with a known verdict', () => {
    const verdicts = new Set(['supported', 'denied', 'degraded', 'not-applicable']);
    for (const [capability, entry] of Object.entries(matrix.capabilities)) {
      expect(Object.keys(entry.cells), capability).toEqual(matrix.entries);
      for (const [column, verdict] of Object.entries(entry.cells)) {
        expect(verdicts.has(verdict), `${capability}/${column}=${verdict}`).toBe(true);
      }
    }
  });

  it('matches rdgHost cells to CLI_CAPABILITIES (supported iff advertised)', () => {
    for (const [capability, entry] of Object.entries(matrix.capabilities)) {
      const expected = cliCapabilities.includes(capability) ? 'supported' : 'denied';
      expect(entry.cells.rdgHost, capability).toBe(expected);
    }
  });

  it('matches lan/cloud cells to the advertised host capability sets', () => {
    for (const [capability, entry] of Object.entries(matrix.capabilities)) {
      expect(entry.cells.lan, capability).toBe(lanCapabilities.includes(capability) ? 'supported' : 'denied');
      expect(entry.cells.cloudDesktop, capability).toBe(
        CLOUD_HOST_CAPABILITIES.includes(capability) ? 'supported' : 'denied',
      );
      // 移动端与桌面共用同一 cloud 协议面；差异只允许出现在呈现层（低于本矩阵粒度）。
      expect(entry.cells.cloudMobile, capability).toBe(entry.cells.cloudDesktop);
    }
  });

  it('keeps every supported remote cell admissible under REMOTE_ALLOWLIST', () => {
    for (const [capability, entry] of Object.entries(matrix.capabilities)) {
      const remoteSupported = ['lan', 'cloudDesktop', 'cloudMobile', 'rdgHost'].some(
        (column) => entry.cells[column] === 'supported',
      );
      if (!remoteSupported) continue;
      for (const method of entry.methods) {
        expect(REMOTE_ALLOWLIST, `${capability}: ${method}`).toContain(method);
      }
    }
  });

  it('points at guard files that exist', () => {
    for (const guard of matrix.guards) {
      expect(() => source(guard), guard).not.toThrow();
    }
  });
});
