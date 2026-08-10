import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

const extension = process.platform === 'win32' ? '.exe' : '';

/** Resolve developer tools to explicit binaries instead of inheriting PATH. */
export function cargoTool(name) {
  const envName = `RIDGE_${name.replaceAll('-', '_').toUpperCase()}_PATH`;
  const configured = process.env[envName]?.trim();
  if (configured) return resolve(configured);
  return join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin', `${name}${extension}`);
}

export function gitTool() {
  const configured = process.env.RIDGE_GIT_PATH?.trim();
  if (configured) return resolve(configured);
  if (process.platform === 'win32') {
    return join(process.env.ProgramFiles || 'C:\\Program Files', 'Git', 'cmd', 'git.exe');
  }
  return '/usr/bin/git';
}
