/** Fence asynchronous bootstrap/retries so detached windows cannot install a client. */
export function presentationSession<T extends { disconnect(): void }>(
  connect: () => Promise<T>,
  install: (client: T) => () => void,
  status: (error?: unknown) => void,
) {
  let generation = 0;
  let disposed = false;
  let detach: (() => void) | undefined;
  return {
    async start() {
      if (disposed) return;
      const attempt = ++generation;
      detach?.();
      detach = undefined;
      let client: T | undefined;
      try {
        client = await connect();
        if (disposed || attempt !== generation) { client.disconnect(); return; }
        const uninstall = install(client);
        const current = client;
        detach = () => { try { uninstall(); } finally { current.disconnect(); } };
        status();
      } catch (error) {
        client?.disconnect();
        if (!disposed && attempt === generation) status(error);
      }
    },
    dispose() { disposed = true; ++generation; detach?.(); detach = undefined; },
  };
}
