// scripts/devtools-trace6.mjs — full sequence with all state
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
  await send('Runtime.enable');
  await send('Page.enable');
  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: `
      window.__ridgeLSlog = [];
      var _origSet = Storage.prototype.setItem;
      Storage.prototype.setItem = function(k, v) {
        if (this === localStorage) {
          window.__ridgeLSlog.push({op:'set', k:k, v:String(v).slice(0, 100), ts:Date.now()});
        }
        return _origSet.call(this, k, v);
      };
      var _origRem = Storage.prototype.removeItem;
      Storage.prototype.removeItem = function(k) {
        if (this === localStorage) {
          window.__ridgeLSlog.push({op:'rem', k:k, ts:Date.now()});
        }
        return _origRem.call(this, k);
      };
    `,
  });
  await send('Page.navigate', { url: 'https://localhost:9620/_app/?reset=1&debug=pane=1&t=' + Date.now() });
  // Sample every 500ms for 10s
  for (let i = 0; i < 20; i++) {
    await new Promise(r => setTimeout(r, 500));
    const s = await send('Runtime.evaluate', {
      expression: `JSON.stringify({
        ms: ${i*500},
        appRoot: !!document.querySelector('.app-root'),
        hasInput: !!document.querySelector('input[type=text]'),
        authScreen: document.body.innerText.slice(0, 50).includes('Enter 6-digit'),
        lsKeys: Object.keys(localStorage).filter(k => k.startsWith('rg-remote')),
        activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620')?.slice(0, 8),
        paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620')?.slice(0, 40),
        token: localStorage.getItem('ridge_remote_token')?.slice(0, 12),
        visibleHead: document.body.innerText.slice(0, 60).replace(/\\n/g, ' '),
      })`,
      returnByValue: true,
    });
    console.log(s.result?.result?.value);
  }
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });