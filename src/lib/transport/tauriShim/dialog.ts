// src/lib/transport/tauriShim/dialog.ts
//
// Browser stand-in for `@tauri-apps/plugin-dialog`. A browser cannot open a
// native folder picker on the remote host, and the picked path must be a path
// on the HOST filesystem (not the browser's). v1 uses a path prompt seeded with
// the host's current project; a richer server-driven directory browser (backed
// by the existing `browse_directory` host command) is a follow-up.

import { bridge } from './bridge';

interface OpenOptions {
  directory?: boolean;
  multiple?: boolean;
  defaultPath?: string;
  title?: string;
}

export async function open(
  options: OpenOptions = {},
): Promise<string | string[] | null> {
  let seed = options.defaultPath ?? '';
  if (!seed) {
    try {
      seed = (await bridge.invoke<string | null>('get_current_project', {})) ?? '';
    } catch {
      seed = '';
    }
  }
  const what = options.directory ? '目录' : '文件';
  const entered =
    typeof window !== 'undefined'
      ? window.prompt(options.title ?? `输入远程主机上的${what}绝对路径`, seed)
      : null;
  if (!entered) return null;
  const path = entered.trim();
  if (!path) return null;
  return options.multiple ? [path] : path;
}

export async function save(options: { defaultPath?: string; title?: string } = {}): Promise<string | null> {
  const entered =
    typeof window !== 'undefined'
      ? window.prompt(options.title ?? '输入远程主机上的保存路径', options.defaultPath ?? '')
      : null;
  const path = entered?.trim();
  return path ? path : null;
}

export async function message(msg: string): Promise<void> {
  if (typeof window !== 'undefined') window.alert(msg);
}

export async function confirm(msg: string): Promise<boolean> {
  return typeof window !== 'undefined' ? window.confirm(msg) : false;
}

export async function ask(msg: string): Promise<boolean> {
  return typeof window !== 'undefined' ? window.confirm(msg) : false;
}
