import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { HOST_CAPABILITIES as CLOUD_HOST_CAPABILITIES } from '../cloud/cloudHostBridge';
import { MUTATING_METHODS, REMOTE_ALLOWLIST } from '../cloud/remoteAllowlist';
import {
  REMOTE_CAPABILITY_METHODS,
  capabilityForRemoteMethod,
  getRemotePanelAvailability,
  type RemoteCapability,
} from './capabilityContract';
import { CLIENT_CAPABILITIES } from './rpcClient';
import { DESKTOP_PRIVILEGED_METHODS } from './protocolAdmission';

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
  const desktopHost = source('packages/ridge-kernel/src/host.rs');
  const domainHandlers = `${desktopHost}\n${source('packages/ridge-kernel/src/commune.rs')}\n${source('packages/ridge-kernel/src/workspace_commands.rs')}`;
  const coreDispatch = source('packages/ridge-core/src/dispatch.rs');
  const mainApp = source('src/remote/MainApp.svelte');
  const remoteSidebar = source('src/remote/lib/RemoteSidebar.svelte');

  const desktopCapabilities = rustStringArray(desktopHost, 'KERNEL_HOST_CAPABILITIES');

  it('keeps LAN desktop, Cloud desktop, and controller capability names aligned', () => {
    expect(desktopCapabilities).toEqual(CLIENT_CAPABILITIES);
    expect(CLOUD_HOST_CAPABILITIES).toEqual(CLIENT_CAPABILITIES);
  });

  it('admits and handles every method required by an advertised desktop capability', () => {
    for (const method of requiredMethods(desktopCapabilities)) {
      expect(REMOTE_ALLOWLIST, `${method} must be remotely admitted`).toContain(method);
      expect(`${domainHandlers}\n${coreDispatch}`, `${method} must have a desktop remote handler`).toMatch(
        new RegExp(`"${method}"(?:\\s*\\|\\s*"[^"]+")*\\s*=>|method\\s*==\\s*"${method}"|if\\s+matches!\\(\\s*method,\\s*(?:"[^"]+"\\s*\\|\\s*)*"${method}"`),
      );
    }
  });

  it('keeps CLI LAN, TUI and Cloud entry points on the shared Runtime owner', () => {
    expect(source('packages/ridge-cli/src/kernel_host_impl.rs')).toMatch(/pub use ridge_kernel::host::\*;/);
    expect(source('packages/ridge-cli/src/core_host.rs')).toMatch(/pub use ridge_kernel::host_core::\*;/);
    for (const path of ['packages/ridge-cli/src/host.rs', 'packages/ridge-cli/src/daemon.rs']) {
      expect(source(path), path).toContain('ridge_kernel::auth_client::remote_auth(endpoint');
    }
    expect(source('packages/ridge-cli/src/host.rs')).toContain('runtime.call("set_remote_enabled"');
    const tui = source('packages/ridge-cli/src/tui/mod.rs');
    expect(tui).toContain('totp.call("set_remote_enabled"');
    expect(tui).toContain('crate::host::lan_info(&totp)');
    expect(tui).not.toMatch(/wait_tcp_ready|lan_host::run/);
    expect(source('packages/ridge-cli/src/daemon.rs')).toContain('runtime.call("cloud_host_online"');
    expect(source('packages/ridge-kernel/src/cloud/connection.rs')).toContain('crate::host::implemented_capabilities()');
  });

  it('keeps snapshot/diff admitted while host-privileged methods remain excluded', () => {
    expect(REMOTE_ALLOWLIST).toContain('get_workspace_snapshot');
    expect(REMOTE_ALLOWLIST).toContain('git_diff_file');
    expect(REMOTE_ALLOWLIST).toContain('git_stash_list');
    expect(coreDispatch).toMatch(/"git_stash_list"\s*=>/);
    expect(REMOTE_ALLOWLIST).not.toContain('get_remote_info');
    expect(REMOTE_ALLOWLIST).not.toContain('get_remote_totp');
    expect(rustCapability).toContain('pub const REMOTE_ALLOWLIST');
  });

  // OP-CAP-PARITY: multi-host outbound + orch health surface boundaries.
  it('admits orchestration health read but keeps multi-host outbound desktop-local', () => {
    expect(REMOTE_ALLOWLIST).toContain('get_orchestration_health');
    expect(REMOTE_CAPABILITY_METHODS.teammate).toContain('get_orchestration_health');
    // Desktop-only hosts outbound surface must never be remotely admitted.
    for (const method of [
      'connect_host',
      'attach_host_session',
      'detach_host_session',
      'inject_host_output',
      'forget_host',
      'get_outbound_stats',
      'host_list_snapshot',
      'register_frontend_host',
      'list_host_sessions',
      'disconnect_host',
    ]) {
      expect(REMOTE_ALLOWLIST).not.toContain(method);
    }
  });

  it('keeps DESKTOP_ONLY_HOST_METHODS rust list aligned with deny list', () => {
    const hostsDesktop = source('packages/ridge-core/src/protocol_guard.rs');
    expect(rustStringArray(hostsDesktop, 'DESKTOP_ONLY_HOST_METHODS')).toEqual(
      DESKTOP_PRIVILEGED_METHODS,
    );
    for (const method of [
      'connect_host',
      'detach_host_session',
      'get_outbound_stats',
      'attach_host_session',
      'register_frontend_host',
    ]) {
      expect(hostsDesktop).toContain(`"${method}"`);
      expect(REMOTE_ALLOWLIST).not.toContain(method);
    }
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

  it('maps rejected controller methods back to one coarse capability', () => {
    expect(capabilityForRemoteMethod('get_teammate_topology')).toBe('teammate');
    expect(capabilityForRemoteMethod('detect_available_shells')).toBeUndefined();
  });

  it('wires negotiated capabilities into the controller shell and sidebar', () => {
    expect(mainApp).toContain('ws.onCapabilitiesChanged(refreshCapabilities)');
    for (const panel of ['files', 'git', 'search', 'team']) {
      expect(mainApp).toContain(`{#if panelAvailability.${panel}}`);
      expect(remoteSidebar).toContain(`{#if available.${panel}}`);
    }
  });

  it('admits the teammate roster read while HITL adjudication stays host-privileged', () => {
    // P2：只读 roster + 脱敏待审批快照（非 mutating）+ 阶段 2 远端裁决（mutating）。
    for (const method of ['get_teammate_topology', 'list_hitl_pending', 'read_agent_recent_replies']) {
      expect(REMOTE_ALLOWLIST).toContain(method);
      expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).toContain(method);
      expect(rustStringArray(rustCapability, 'MUTATING_METHODS')).not.toContain(method);
    }
    // 阶段 2：resolve_hitl_remote 远端可达且双侧归类 mutating（只读会话拒）。
    expect(REMOTE_ALLOWLIST).toContain('resolve_hitl_remote');
    expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).toContain('resolve_hitl_remote');
    expect(MUTATING_METHODS).toContain('resolve_hitl_remote');
    expect(rustStringArray(rustCapability, 'MUTATING_METHODS')).toContain('resolve_hitl_remote');
    expect(REMOTE_ALLOWLIST).toContain('set_teammate_groups');
    expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).toContain('set_teammate_groups');
    expect(MUTATING_METHODS).toContain('set_teammate_groups');
    expect(rustStringArray(rustCapability, 'MUTATING_METHODS')).toContain('set_teammate_groups');
    // P2 阶段 2 之前：HITL 裁决、网关开关与 G1 暂停/恢复（写操作）不得远程可达。
    for (const method of ['resolve_hitl_request', 'set_hitl_enabled', 'suspend_agent', 'resume_agent', 'register_teammate_agent', 'register_commune_member', 'release_teammate_agent']) {
      expect(REMOTE_ALLOWLIST).not.toContain(method);
      expect(rustStringArray(rustCapability, 'REMOTE_ALLOWLIST')).not.toContain(method);
    }
    // CLI Host delegates to the same Runtime capability declaration.
    expect(desktopCapabilities).toContain('teammate');
    expect(CLOUD_HOST_CAPABILITIES).toContain('teammate');
    expect(CLIENT_CAPABILITIES).toContain('teammate');
  });
});

const capabilityMatrixExists = existsSync(resolve(root, 'docs/capability-matrix.json'));

describe.skipIf(!capabilityMatrixExists)('docs/capability-matrix.json stays a projection of the canonical declarations (A2)', () => {
  interface MatrixCapability {
    methods: string[];
    cells: Record<string, string>;
  }
  const matrix = JSON.parse(capabilityMatrixExists ? source('docs/capability-matrix.json') : '{}') as {
    entries: string[];
    capabilities: Record<string, MatrixCapability>;
    guards: string[];
  };
  const cliCapabilities = rustStringArray(source('packages/ridge-kernel/src/host.rs'), 'KERNEL_HOST_CAPABILITIES');
  const lanCapabilities = rustStringArray(
    source('packages/ridge-kernel/src/host.rs'),
    'KERNEL_HOST_CAPABILITIES',
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

  it('matches rdgHost cells to shared Runtime capabilities (supported iff advertised)', () => {
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
