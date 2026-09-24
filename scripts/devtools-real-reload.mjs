// scripts/devtools-real-reload.mjs — Page.reload to actually re-init SPA
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
  await send('Page.enable');
  // Wipe ALL localStorage first
  await send('Runtime.evaluate', {
    expression: `localStorage.clear(); Object.keys(localStorage)`,
    returnByValue: true,
  });
  // Reload with reset=1
  await send('Page.reload', { ignoreCache: true });
  await new Promise(r => setTimeout(r, 6000));
  const result = await send('Runtime.evaluate', {
    expression: `JSON.stringify({ls: Object.keys(localStorage), activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'), paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620'), url: location.href, ready: document.readyState})`,
    returnByValue: true,
  });
  console.log('after reload:', result.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });