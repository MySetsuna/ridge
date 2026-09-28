// scripts/avd-devtools-pane-map.mjs
import WebSocket from "ws";

const ws = new WebSocket("ws://localhost:9222/devtools/page/2");
let msgId = 0;
const pending = new Map();

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error("timeout")); } }, 10000);
  });
}

async function evaluate(expression) {
  const r = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  return r.result?.result?.value;
}

ws.on("message", (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id).resolve(msg);
    pending.delete(msg.id);
  }
});

ws.on("open", async () => {
  try {
    const result = await evaluate(`JSON.stringify({
      paneMap: localStorage.getItem('rg-remote-pane-map:lan:localhost:9620'),
      activeWs: localStorage.getItem('rg-remote-active-ws:lan:localhost:9620'),
      device: localStorage.getItem('ridge_remote_device'),
      // Try to find pane via manager internals
      // The manager might be accessible via Svelte component internals
      // Let's try to find it via the canvas element's __svelte or similar
      canvasCount: document.querySelectorAll('canvas').length,
      // Try visibleText with the UUIDs
      tryVisible: (() => {
        const hooks = window.__windE2E;
        if (!hooks) return 'NO_HOOKS';
        const uuids = [...new Set((document.body.innerHTML.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi)||[]))];
        const results = {};
        for (const u of uuids) {
          try {
            const lines = hooks.visibleText(u);
            results[u] = { lines: lines?.length, sample: (lines||[]).slice(0,3) };
          } catch(e) { results[u] = 'ERR:' + e.message.slice(0,80); }
        }
        return results;
      })()
    })`);
    console.log(result);
    process.exit(0);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
