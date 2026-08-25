import { homedir } from 'node:os';
import { existsSync } from 'node:fs';
import { delimiter, dirname, join, resolve } from 'node:path';

const extension = process.platform === 'win32' ? '.exe' : '';

/** Resolve developer tools to explicit binaries instead of inheriting PATH. */
export function cargoTool(name) {
  const envName = `RIDGE_${name.replaceAll('-', '_').toUpperCase()}_PATH`;
  const configured = process.env[envName]?.trim();
  // Do not reinterpret a Windows path on a Linux/macOS runner (or vice versa)
  // with the host `resolve`; keep relative overrides host-relative.
  if (configured) {
    return /^[A-Za-z]:[\\/]|^\\\\|^\//.test(configured) ? configured : resolve(configured);
  }
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
    if (name === 'pnpm') {
      const configured = process.env.RIDGE_PNPM_PATH?.trim();
      if (configured) return configured;
      const devKitPath = join(String.raw`C:\DevKit\nodejs`, 'pnpm.cmd');
      if (existsSync(devKitPath)) return devKitPath;
      return 'pnpm';
    }
  }
  if (name === 'taskkill') return '/usr/bin/taskkill';
  if (name === 'powershell') return '/usr/bin/pwsh';
  if (name === 'docker') return '/usr/bin/docker';
  if (name === 'pnpm') return '/usr/local/bin/pnpm';
  throw new Error(`unsupported system tool: ${name}`);
}

/** Return a shell-free pnpm process invocation on every platform. */
export function pnpmInvocation() {
  const command = systemTool('pnpm');
  if (process.platform !== 'win32' || !command.toLowerCase().endsWith('.cmd')) {
    return { command, args: [] };
  }

  const cli = join(dirname(resolve(command)), 'node_modules', 'pnpm', 'bin', 'pnpm.cjs');
  if (!existsSync(cli)) throw new Error(`pnpm CLI not found beside ${command}`);
  return { command: process.execPath, args: [cli] };
}
