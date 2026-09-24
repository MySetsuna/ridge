// scripts/devtools-query.mjs — query Chrome page DOM via DevTools protocol
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
  // Eval JS in page context
  const result = await send('Runtime.evaluate', {
    expression: `
      JSON.stringify({
        docReady: document.readyState,
        appRoot: !!document.querySelector('.app-root'),
        overlay: !!document.querySelector('[data-pane-debug-main]'),
        allMain: Array.from(document.querySelectorAll('[data-pane-debug-main]')).map(el => ({
          text: el.innerText.slice(0, 200),
          rect: el.getBoundingClientRect ? JSON.stringify(el.getBoundingClientRect()) : null,
          style: getComputedStyle(el).cssText.slice(0, 200),
          display: getComputedStyle(el).display,
          visibility: getComputedStyle(el).visibility,
          position: getComputedStyle(el).position,
          zIndex: getComputedStyle(el).zIndex,
          opacity: getComputedStyle(el).opacity,
          transform: getComputedStyle(el).transform,
        })),
        url: location.href,
        title: document.title,
        bodyChildren: Array.from(document.body.children).map(el => el.tagName + '.' + el.className.slice(0, 50)),
        // Find all scripts loaded
        scripts: Array.from(document.querySelectorAll('script[src]')).map(s => s.src),
        // Check console errors
        errors: window.__ridgeErrors || [],
      })
    `,
    returnByValue: true,
  });
  console.log('Eval result:', result.result?.result?.value);
  // Get console logs
  const consoleResult = await send('Runtime.evaluate', {
    expression: `
      JSON.stringify({
        // Check the actual served JS
        serviceWorker: navigator.serviceWorker ? 'present' : 'absent',
        scriptCount: document.querySelectorAll('script').length,
        // Check if index bundle is loaded
        indexLoaded: Array.from(document.scripts).some(s => s.src.includes('index-CNb-nLSp')),
      })
    `,
    returnByValue: true,
  });
  console.log('Script check:', consoleResult.result?.result?.value);
  process.exit(0);
});
ws.on('error', (e) => { console.error('WS error:', e.message); process.exit(1); });