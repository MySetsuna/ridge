import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import {
  REMOTE_ALLOWLIST,
  MUTATING_METHODS,
  isRemoteAllowed,
  isMutatingMethod,
} from './remoteAllowlist';

const capabilitySource = readFileSync(
  new URL('../../../../ridge-core/src/capability.rs', import.meta.url),
  'utf8',
);

function rustStringArray(name: string): string[] {
  const body = capabilitySource.match(
    new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`),
  )?.[1];
  if (!body) throw new Error(`missing Rust string array: ${name}`);
  return [...body.matchAll(/^\s*"([^"]+)",/gm)].map((m) => m[1]);
}

// Security property (audit ①-1): the cloud host must admit only remote-safe
// commands. These tests pin the host-privileged exclusions and compare both TS
// mirrors item-for-item with capability.rs so any divergence is caught locally.

describe('isRemoteAllowed', () => {
  it('admits the legitimate remote commands', () => {
    for (const m of [
      'get_directory_children',
      'read_file',
      'write_file',
      'get_pane_layout',
      'write_to_pty',
      'load_terminal_font_faces',
      'read_terminal_font_face_chunk',
      'get_pane_scrollback_tail',
      'get_pane_scrollback_before',
      // §R-CLOUD-CONVERGE: the converged full-resync-frame command MUST pass the
      // cloud gate (the prior get_pane_resync_preamble never did → dead cloud fix).
      'get_pane_resync_frame',
      'list_workspaces',
      'get_workspace_snapshot',
      'switch_workspace',
      'get_active_theme_entry',
      'text_search',
      'get_scm_status',
      'git_commit',
      'list_native_sessions',
      'summon_native_session',
    ]) {
      expect(isRemoteAllowed(m)).toBe(true);
    }
  });

  it('rejects host-privileged commands (the RCE guard)', () => {
    // Byte-for-byte mirror of capability.rs's deliberate exclusions.
    for (const m of [
      'get_remote_info', // leaks the LAN TOTP secret — the verified RCE vector
      'set_remote_enabled',
      'disconnect_session',
      'enter_deep_root_mode',
      'set_cloud_remote_active',
    ]) {
      expect(isRemoteAllowed(m)).toBe(false);
    }
  });

  it('rejects unknown / arbitrary method names', () => {
    expect(isRemoteAllowed('')).toBe(false);
    expect(isRemoteAllowed('rm_rf_everything')).toBe(false);
    expect(isRemoteAllowed('__proto__')).toBe(false);
  });
});

describe('isMutatingMethod', () => {
  it('flags fs/git mutations', () => {
    for (const m of ['write_file', 'apply_file_edits', 'replace_in_files', 'git_commit', 'git_reset']) {
      expect(isMutatingMethod(m)).toBe(true);
    }
  });

  it('does not flag read-only methods', () => {
    for (const m of ['read_file', 'get_file_tree', 'search', 'get_scm_status', 'git_list_branches']) {
      expect(isMutatingMethod(m)).toBe(false);
    }
  });

  it('classifies native-only writes without widening the Remote capability set', () => {
    const nativeOnly = new Set(['release_teammate_agent', 'initialize_cloud_controller_identity',
      'git_push_branch', 'git_merge_branch', 'git_delete_branch', 'git_rename_branch', 'git_rebase',
      'git_delete_tag', 'git_push_tag', 'git_stash_push', 'git_stash_apply', 'git_stash_pop',
      'git_stash_drop', 'git_stash_branch', 'git_cherry_pick_abort', 'git_revert_abort']);
    for (const m of MUTATING_METHODS) {
      if (nativeOnly.has(m)) expect(REMOTE_ALLOWLIST).not.toContain(m);
      else expect(REMOTE_ALLOWLIST).toContain(m);
    }
    for (const m of nativeOnly) {
      expect(isMutatingMethod(m)).toBe(true);
      expect(isRemoteAllowed(m)).toBe(false);
    }
    expect(isMutatingMethod('git_fetch')).toBe(true);
    expect(isMutatingMethod('release_teammate_agent')).toBe(true);
    expect(isRemoteAllowed('release_teammate_agent')).toBe(false);
  });
});

describe('mirror integrity (vs capability.rs)', () => {
  it('admits the rebuilt terminal RPCs while protecting input from read-only sessions', () => {
    for (const method of ['write_terminal_input','get_terminal_snapshot','get_terminal_history']) expect(isRemoteAllowed(method)).toBe(true);
    expect(isMutatingMethod('write_terminal_input')).toBe(true);
    expect(isMutatingMethod('get_terminal_snapshot')).toBe(false);
    expect(isMutatingMethod('get_terminal_history')).toBe(false);
    expect(isRemoteAllowed('get_remote_info')).toBe(false);
  });

  it('matches the canonical Rust allow-list item-for-item', () => {
    expect(REMOTE_ALLOWLIST).toEqual(rustStringArray('REMOTE_ALLOWLIST'));
  });
  it('matches the canonical Rust mutating set item-for-item', () => {
    expect(MUTATING_METHODS).toEqual(rustStringArray('MUTATING_METHODS'));
  });
  it('has no duplicate entries', () => {
    expect(new Set(REMOTE_ALLOWLIST).size).toBe(REMOTE_ALLOWLIST.length);
    expect(new Set(MUTATING_METHODS).size).toBe(MUTATING_METHODS.length);
  });
});
