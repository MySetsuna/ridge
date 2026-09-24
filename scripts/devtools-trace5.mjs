// scripts/devtools-trace5.mjs — capture full state sequence
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
  // Install hook that captures Storage ops AND tracks ui.activeWorkspaceId if exposed
  await send('Page.addScriptToEvaluateOnNewDocument', {
    source: `
      window.__ridgeLSlog = [];
      var _origSet = Storage.prototype.setItem;
      Storage.prototype.setItem = function(k, v) {
        if (this === localStorage) {
          window.__ridgeLSlog.push({op:'set', k:k, v:String(v).slice(0, 80), ts:Date.now()});
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
  // Navigate directly to URL with reset=1 — no manual clear
  await send('Page.navigate', { url: 'https://localhost:9620/_app/?reset=1&debug=pane=1&t=' + Date.now() });
  await new Promise(r => setTimeout(r, 6000));
  // Check LS state BEFORE verify
  const before = await send('Runtime.evaluate', {
    expression: `JSON.stringify({
      url: location.href,
      appRoot: !!document.querySelector('.app-root'),
      lsKeys: Object.keys(localStorage),
      activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'),
      paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620'),
      logLen: (window.__ridgeLSlog || []).length,
      log: (window.__ridgeLSlog || []).slice(0, 30),
    }, null, 2)`,
    returnByValue: true,
  });
  console.log('=== BEFORE VERIFY (after page load) ===');
  console.log(before.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });