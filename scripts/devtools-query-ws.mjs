// scripts/devtools-query-ws.mjs — get WS state
const WebSocket = (await import('ws')).WebSocket;
const ws = new WebSocket('ws://localhost:9222/devtools/page/3');
let id = 1;
const pending = new Map();
ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
});
function send(method, params = {}) {
  const myId = id++;
  return new Promise((resolve) => { pending.set(myId, resolve); ws.send(JSON.stringify({ id: myId, method, params })); });
}
ws.on('open', async () => {
  const result = await send('Runtime.evaluate', {
    expression: `
      JSON.stringify({
        // Try to find the workspaces from React Query cache
        queryKeys: (window.__RIDGE_REMOTE_PERF?.keys) || [],
        // Check the actual SPA state
        activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'),
        // Get all SW cached responses
        cachesKeys: navigator.serviceWorker?.controller?.scriptURL,
      })
    `,
    returnByValue: true,
  });
  console.log(result.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });