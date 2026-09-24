// scripts/devtools-query-deep.mjs — deep dive into SPA state
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
  // Hook XHR to see what the SPA fetches
  await send('Runtime.evaluate', {
    expression: `
      window.__xhrLog = [];
      var _open = XMLHttpRequest.prototype.open;
      XMLHttpRequest.prototype.open = function(method, url) {
        window.__xhrLog.push({method, url, ts: Date.now()});
        return _open.apply(this, arguments);
      };
    `,
    returnByValue: true,
  });
  await new Promise(r => setTimeout(r, 2000));
  const result = await send('Runtime.evaluate', {
    expression: `JSON.stringify(window.__xhrLog.slice(-20), null, 2)`,
    returnByValue: true,
  });
  console.log(result.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });