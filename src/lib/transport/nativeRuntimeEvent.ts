import * as native from '@ridge/native-os-event';
import { bridge } from './tauriShim/bridge';
export type { Event, EventCallback, EventName, EventTarget, Options, UnlistenFn } from '@ridge/native-os-event';
export { TauriEvent } from '@ridge/native-os-event';

function osEvent(event: string) {
  return event.startsWith('tauri://') || ['ridge://memory-reclaim', 'ridge://memory-restore', 'ridge://auth-focus',
    'editor-window-open-file', 'editor-window-closed'].includes(event);
}
export async function listen<T>(event: string, handler: native.EventCallback<T>, options?: native.Options): Promise<native.UnlistenFn> {
  return osEvent(event) ? native.listen(event, handler, options) : bridge.listen(event, handler);
}
export async function once<T>(event: string, handler: native.EventCallback<T>, options?: native.Options): Promise<native.UnlistenFn> {
  if (osEvent(event)) return native.once(event, handler, options);
  const stop = bridge.listen<T>(event, payload => { stop(); handler(payload); });
  return stop;
}
export async function emit(event: string, payload?: unknown): Promise<void> {
  if (!osEvent(event)) throw new Error('Domain events originate in Runtime; use an authorized RPC');
  return native.emit(event, payload);
}
export async function emitTo(target: string | native.EventTarget, event: string, payload?: unknown): Promise<void> {
  if (!osEvent(event)) throw new Error('Domain events originate in Runtime; use an authorized RPC');
  return native.emitTo(target, event, payload);
}
