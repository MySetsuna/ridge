import { homedir } from 'node:os';
import { existsSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';

const extension = process.platform === 'win32' ? '.exe' : '';

/** Resolve developer tools to explicit binaries instead of inheriting PATH. */
export function cargoTool(name) {
  const envName = `RIDGE_${name.replaceAll('-', '_').toUpperCase()}_PATH`;
  const configured = process.env[envName]?.trim();
  if (configured) return resolve(configured);
  const filename = `${name}${extension}`;
  const cargoHomePath = join(process.env.CARGO_HOME || join(homedir(), '.cargo'), 'bin', filename);
  if (existsSync(cargoHomePath)) return cargoHomePath;

  const pathValue = process.env.PATH ?? process.env.Path ?? '';
  for (const entry of pathValue.split(delimiter)) {
    if (!entry) continue;
    const candidate = join(entry, filename);
    if (existsSync(candidate)) return candidate;
  }

  return cargoHomePath;
}

export function gitTool() {
  const configured = process.env.RIDGE_GIT_PATH?.trim();
  if (configured) return resolve(configured);
  if (process.platform === 'win32') {
    return join(process.env.ProgramFiles || String.raw`C:\Program Files`, 'Git', 'cmd', 'git.exe');
  }
  return '/usr/bin/git';
}

export function systemTool(name) {
  if (process.platform === 'win32') {
    const systemRoot = String.raw`C:\Windows`;
    if (name === 'taskkill') return join(systemRoot, 'System32', 'taskkill.exe');
    if (name === 'powershell') return join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
    if (name === 'docker') return join(String.raw`C:\Program Files`, 'Docker', 'Docker', 'resources', 'bin', 'docker.exe');
    if (name === 'pnpm') return join(String.raw`C:\DevKit\nodejs\pnpm.cmd`);
  }
  if (name === 'taskkill') return '/usr/bin/taskkill';
  if (name === 'powershell') return '/usr/bin/pwsh';
  if (name === 'docker') return '/usr/bin/docker';
  if (name === 'pnpm') return '/usr/local/bin/pnpm';
  throw new Error(`unsupported system tool: ${name}`);
}
