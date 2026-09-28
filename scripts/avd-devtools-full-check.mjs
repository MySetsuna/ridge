// scripts/avd-devtools-full-check.mjs
import WebSocket from "ws";

const MARKER = process.argv[2] || `RIDGE-AVD-FULL-${Date.now()}`;
console.log("MARKER:", MARKER);

const ws = new WebSocket("ws://localhost:9222/devtools/page/2");
let msgId = 0;
const pending = new Map();

function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++msgId;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error("timeout")); } }, 15000);
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
    const composite = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      return activeWs + ':' + map[activeWs];
    })()`);
    console.log("Composite:", composite);

    // Feed marker
    await evaluate(`(() => { window.__windE2E.feedPty('${composite}', '\\r\\n${MARKER}\\r\\n'); return 'OK'; })()`);
    await new Promise(r => setTimeout(r, 3000));

    // FULL visible text check
    const result = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${composite}');
        const joined = (lines||[]).join('\\n');
        return JSON.stringify({
          total: lines?.length,
          hasMarker: joined.includes('${MARKER}'),
          markerLine: (lines||[]).findIndex(l => l.includes('${MARKER}')),
          allLines: lines
        });
      } catch(e) { return JSON.stringify({err:e.message}); }
    })()`);
    console.log("Result:", result);

    // Screenshot
    const ss = await send("Page.captureScreenshot", { format: "png" });
    if (ss.result?.data) {
      const fs = await import("node:fs");
      fs.writeFileSync(`artifacts/release/avd-visual/full-${MARKER}.png`, Buffer.from(ss.result.data, "base64"));
      console.log("Screenshot saved");
    }

    const parsed = JSON.parse(result);
    process.exit(parsed.hasMarker ? 0 : 1);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
