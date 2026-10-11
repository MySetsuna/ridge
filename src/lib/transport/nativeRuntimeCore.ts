// Native OS presentation stays native. Domain requests never fall back to IPC.
import { invoke as nativeInvoke } from '@ridge/native-os-core';
import { invoke as runtimeInvoke } from './tauriShim/core';
export { Channel } from './tauriShim/core';
export { convertFileSrc, isTauri, transformCallback } from '@ridge/native-os-core';

const OS_COMMANDS = new Set([
  'bootstrap_runtime_client',
]);
const OS_PLUGINS = ['plugin:window|', 'plugin:webview|', 'plugin:event|',
  'plugin:dialog|', 'plugin:clipboard-manager|', 'plugin:notification|', 'plugin:opener|'];

export function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (OS_COMMANDS.has(command) || OS_PLUGINS.some(prefix => command.startsWith(prefix))) {
    return nativeInvoke<T>(command, args);
  }
  return runtimeInvoke<T>(command, args);
}
