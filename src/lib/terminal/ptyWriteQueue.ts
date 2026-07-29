/** Preserve per-pane input order across asynchronous desktop IPC writes. */
const tails = new Map<string, Promise<void>>();

export function enqueuePtyWrite(key: string, write: () => Promise<unknown>): Promise<void> {
  const previous = tails.get(key) ?? Promise.resolve();
  const next = previous.catch(() => undefined).then(async () => {
    await write();
  });
  tails.set(key, next);
  void next.finally(() => {
    if (tails.get(key) === next) tails.delete(key);
  }).catch(() => undefined);
  return next;
}
