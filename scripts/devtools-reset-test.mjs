// scripts/devtools-reset-test.mjs — verify reset=1 actually clears localStorage
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
  // First check what's there
  const before = await send('Runtime.evaluate', {
    expression: `JSON.stringify({lsBefore: Object.keys(localStorage), url: location.href, resetParam: new URL(location.href).searchParams.get('reset')})`,
    returnByValue: true,
  });
  console.log('BEFORE:', before.result?.result?.value);

  // Navigate fresh with reset=1
  await send('Page.navigate', { url: 'https://localhost:9620/_app/?reset=1&debug=pane=1' });
  await new Promise(r => setTimeout(r, 5000));

  const after = await send('Runtime.evaluate', {
    expression: `JSON.stringify({lsAfter: Object.keys(localStorage), url: location.href})`,
    returnByValue: true,
  });
  console.log('AFTER NAV:', after.result?.result?.value);

  // Wait for app to settle, dump again
  await new Promise(r => setTimeout(r, 3000));
  const settled = await send('Runtime.evaluate', {
    expression: `JSON.stringify({lsSettled: Object.keys(localStorage), activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'), paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620')})`,
    returnByValue: true,
  });
  console.log('SETTLED:', settled.result?.result?.value);

  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });