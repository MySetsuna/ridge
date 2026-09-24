// scripts/devtools-query2.mjs — get full body HTML and inline JS
const WebSocket = (await import('ws')).WebSocket;
const ws = new WebSocket('ws://localhost:9222/devtools/page/3');
let id = 1;
const pending = new Map();
ws.on('message', (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});
function send(method, params = {}) {
  const myId = id++;
  return new Promise((resolve) => {
    pending.set(myId, resolve);
    ws.send(JSON.stringify({ id: myId, method, params }));
  });
}
ws.on('open', async () => {
  const result = await send('Runtime.evaluate', {
    expression: `
      JSON.stringify({
        bodyHTML: document.body.innerHTML.slice(0, 4000),
        appDiv: document.getElementById('app') ? document.getElementById('app').innerHTML.slice(0, 200) : 'NO #app',
        appDivExists: !!document.getElementById('app'),
        visibleText: document.body.innerText.slice(0, 500),
        // Check for JS errors via runtime state
        hasGlobalError: typeof window.__ridgeLastError !== 'undefined' ? window.__ridgeLastError : null,
        // Try to find any console error logs (RIDGE trace, etc)
        globalKeys: Object.keys(window).filter(k => k.startsWith('__') || k.startsWith('ridge')).slice(0, 30),
      })
    `,
    returnByValue: true,
  });
  console.log(result.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });