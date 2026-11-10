import type { TerminalInputContext } from '@ridge/remote/shared/transport/paneRpcScheduler';

/** Capture key intent without guessing terminal modes before attachment. */
export type RemoteKey = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>;

export interface RemoteKeyEncoder {
  encodeKey(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean): Uint8Array;
}

export type RemoteInputIntent = Uint8Array | RemoteKey | { paste: string };

export function encodeRemoteInput(kernel: RemoteKeyEncoder & { encodePaste(text: string): Uint8Array }, input: RemoteInputIntent): Uint8Array {
  if (input instanceof Uint8Array) return input;
  return 'paste' in input ? kernel.encodePaste(input.paste) : encodeRemoteKey(kernel, input);
}

/** Component-owned input awaiting its canonical terminal binding. */
export class PendingRemoteInput {
  private inputs: RemoteInputIntent[] = [];
  private bytes = 0;
  private epoch: string | null = null;
  private observedEpoch: string | null = null;
  failed = false;

  constructor(private readonly limit = 64 * 1024) {}

  get length(): number { return this.inputs.length; }

  observe(context: TerminalInputContext | null): void {
    if (context) this.observedEpoch = context[1];
  }

  enqueue(input: RemoteInputIntent, context: TerminalInputContext | null): boolean {
    this.observe(context);
    if (this.failed) return false;
    let bytes;
    if (input instanceof Uint8Array) bytes = input.byteLength;
    else if ('paste' in input) bytes = new TextEncoder().encode(input.paste).byteLength;
    else bytes = Math.max(128, input.key.length * 2);
    if (this.bytes + bytes > this.limit) {
      this.cancel();
      return false;
    }
    if (!bytes) return true;
    if (!this.inputs.length) this.epoch = this.observedEpoch;
    this.inputs.push(input instanceof Uint8Array ? input.slice() : { ...input });
    this.bytes += bytes;
    return true;
  }

  drain(context: TerminalInputContext | null): RemoteInputIntent[] {
    this.observe(context);
    if (!context || this.failed) return [];
    if (this.epoch !== null && this.epoch !== context[1]) {
      this.cancel();
      return [];
    }
    const inputs = this.inputs;
    this.reset();
    return inputs;
  }

  reset(): void {
    this.inputs = [];
    this.bytes = 0;
    this.epoch = null;
    this.failed = false;
  }

  cancel(): void {
    this.reset();
    this.failed = true;
  }
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
