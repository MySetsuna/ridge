import { invoke } from '@tauri-apps/api/core';
import { RemoteConnection } from '@ridge/remote/shared/transport/wsRemote';
import { createLanWsTransport } from '@ridge/remote/shared/transport/lanWsAdapter';

/** Consume the Runtime's Rust ClientGrant; credentials never leave this closure
 * for URLs, localStorage, sessionStorage or the business RPC argument stream. */
export async function connectPresentationRuntime(
  bootstrap: () => Promise<Record<string, unknown>> = () => invoke('bootstrap_runtime_client'),
) {
  const grant = await bootstrap();
  const text = (key: string) => {
    const value = grant[key];
    if (typeof value !== 'string' || !value) throw new Error(`Invalid Runtime grant field: ${key}`);
    return value;
  };
  const url = new URL(text('wsUrl'));
  const token = text('token');
  const deviceId = text('deviceId');
  if (url.protocol !== 'ws:' || url.hostname !== '127.0.0.1' || !url.port
    || url.pathname !== '/v1/local/ws' || url.search || url.hash || url.username || url.password
    || !/^[a-f0-9]{64}$/.test(token) || deviceId.length > 128
    || text('origin') !== window.location.origin
    || !Number.isSafeInteger(grant.protocolVersion) || Number(grant.protocolVersion) < 1) {
    throw new Error('Runtime presentation grant identity mismatch');
  }
  const target = new URL(url);
  target.searchParams.set('deviceId', deviceId);
  const connection = new RemoteConnection(() => new WebSocket(target, ['ridge-client', `auth-${token}`]));
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      stop();
      if (error) { connection.disconnect(); reject(error); } else resolve();
    };
    const timer = setTimeout(() => finish(new Error('Runtime presentation connection timed out')), 15_000);
    const stop = connection.onStateChange(state => {
      if (state === 'connected') finish();
      if (state === 'error') finish(new Error(connection.lastFailure()?.message ?? 'Runtime presentation connection failed'));
    });
    try { connection.connect(url.hostname, Number(url.port), token, 'token', false); }
    catch (error) { finish(error instanceof Error ? error : new Error('Runtime socket initialization failed')); }
  });
  const transport = createLanWsTransport(connection);
  return { connection, transport, disconnect: () => { transport.dispose(); connection.disconnect(); } };
}
