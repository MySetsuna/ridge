// scripts/devtools-localstorage.mjs — dump SPA localStorage
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
        lsKeys: Object.keys(localStorage),
        lsData: Object.fromEntries(Object.keys(localStorage).map(k => [k, localStorage.getItem(k)?.slice(0, 800)])),
        sessionStorage: Object.fromEntries(Object.keys(sessionStorage).map(k => [k, sessionStorage.getItem(k)?.slice(0, 400)])),
      })
    `,
    returnByValue: true,
  });
  console.log(result.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });