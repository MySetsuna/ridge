import { describe, it, expect } from 'vitest';
import {
  REMOTE_ALLOWLIST,
  MUTATING_METHODS,
  isRemoteAllowed,
  isMutatingMethod,
} from './remoteAllowlist';

// Security property (audit ①-1): the cloud host must admit only remote-safe
// commands. These tests pin the host-privileged exclusions + the mirror counts
// so an accidental divergence from capability.rs is caught locally.

describe('isRemoteAllowed', () => {
  it('admits the legitimate remote commands', () => {
    for (const m of [
      'get_directory_children',
      'read_file',
      'write_file',
      'get_pane_layout',
      'write_to_pty',
      'list_workspaces',
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

  it('every mutating method is also in the allow-list', () => {
    for (const m of MUTATING_METHODS) {
      expect(REMOTE_ALLOWLIST).toContain(m);
    }
  });
});

describe('mirror integrity (vs capability.rs)', () => {
  // If these counts change, update capability.rs ⇄ remoteAllowlist.ts together.
  it('allow-list has the expected size', () => {
    expect(REMOTE_ALLOWLIST.length).toBe(89);
  });
  it('mutating set has the expected size', () => {
    expect(MUTATING_METHODS.length).toBe(22);
  });
  it('has no duplicate entries', () => {
    expect(new Set(REMOTE_ALLOWLIST).size).toBe(REMOTE_ALLOWLIST.length);
    expect(new Set(MUTATING_METHODS).size).toBe(MUTATING_METHODS.length);
  });
});
