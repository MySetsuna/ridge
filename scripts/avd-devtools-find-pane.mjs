// scripts/avd-devtools-find-pane.mjs — find the actual pane UUID
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
    // Try to find pane IDs from various sources
    const result = await evaluate(`JSON.stringify({
      // Check DOM for pane attributes
      domPaneIds: [...document.querySelectorAll('[data-pane-id]')].map(e=>e.getAttribute('data-pane-id')),
      domPaneKeys: [...document.querySelectorAll('[data-pane-key]')].map(e=>e.getAttribute('data-pane-key')),
      // Check for any UUID-like strings in the DOM
      uuids: [...new Set((document.body.innerHTML.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi)||[]))],
      // Try to access manager via global
      hasManager: !!window.__windManager,
      // Check localStorage
      localStorageKeys: Object.keys(localStorage),
      // Try to find pane from hooks - try some common patterns
      tryRows: (() => {
        const hooks = window.__windE2E;
        if (!hooks) return 'NO_HOOKS';
        const results = {};
        // Try UUIDs from DOM
        const uuids = [...new Set((document.body.innerHTML.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi)||[]))];
        for (const u of uuids.slice(0,5)) {
          try { results[u] = hooks.rows(u); } catch(e) { results[u] = 'ERR:' + e.message.slice(0,50); }
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
