/**
 * Lossless hand-off buffer for the keyed Remote TerminalCanvas.
 *
 * A pane switch briefly has no mounted canvas while the keep-alive kernel is
 * rebound. Raw PTY frames arriving in that interval must retain order, while
 * a broken/paused transport still needs a hard memory ceiling.
 */
export interface DrainedPaneFrames {
  frames: Uint8Array[];
  needsResync: boolean;
}

export const DEFAULT_PANE_SWITCH_BUFFER_BYTES = 4 * 1024 * 1024;

export class PaneSwitchBuffer {
  private readonly frames = new Map<string, Uint8Array[]>();
  private readonly overflowed = new Set<string>();
  private retainedBytes = 0;

  constructor(private readonly maxBytes = DEFAULT_PANE_SWITCH_BUFFER_BYTES) {}

  enqueue(key: string, data: Uint8Array): void {
    if (!key || data.byteLength === 0) return;
    const frame = data.slice();
    const list = this.frames.get(key) ?? [];
    list.push(frame);
    this.frames.set(key, list);
    this.retainedBytes += frame.byteLength;
    while (this.retainedBytes > this.maxBytes) {
      const oldestKey = this.frames.keys().next().value as string | undefined;
      if (!oldestKey) break;
      const oldest = this.frames.get(oldestKey);
      const dropped = oldest?.shift();
      if (!dropped) {
        this.frames.delete(oldestKey);
        continue;
      }
      this.retainedBytes -= dropped.byteLength;
      this.overflowed.add(oldestKey);
      if (oldest && oldest.length === 0) this.frames.delete(oldestKey);
    }
  }

  drain(key: string): DrainedPaneFrames {
    const frames = this.frames.get(key) ?? [];
    this.frames.delete(key);
    for (const frame of frames) this.retainedBytes -= frame.byteLength;
    return { frames, needsResync: this.overflowed.delete(key) };
  }

  clear(): void {
    this.frames.clear();
    this.overflowed.clear();
    this.retainedBytes = 0;
  }

  get bytes(): number { return this.retainedBytes; }
}
