/** Capture key intent without guessing terminal modes before attachment. */
export type RemoteKey = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>;

export interface RemoteKeyEncoder {
  encodeKey(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean): Uint8Array;
}

export function captureRemoteKey(event: RemoteKey): RemoteKey | null {
  if (event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey) return null;
  if (['Shift', 'Control', 'Alt', 'Meta', 'Dead', 'Process', 'Unidentified'].includes(event.key)) return null;
  return { key: event.key, ctrlKey: event.ctrlKey, altKey: event.altKey,
    shiftKey: event.shiftKey, metaKey: event.metaKey };
}

export function encodeRemoteKey(kernel: RemoteKeyEncoder, event: RemoteKey): Uint8Array {
  return kernel.encodeKey(event.key, event.ctrlKey, event.altKey, event.shiftKey, event.metaKey);
}

export function editingKeyForInput(inputType: string): string | null {
  switch (inputType) {
    case 'deleteContentBackward': return 'Backspace';
    case 'deleteContentForward': return 'Delete';
    case 'insertLineBreak':
    case 'insertParagraph': return 'Enter';
    default: return null;
  }
}
