// scripts/avd-devtools-feed2.mjs
import WebSocket from "ws";

const MARKER = process.argv[2] || `RIDGE-AVD-FEED2-${Date.now()}`;
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
    const paneInfo = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      return JSON.stringify({ composite: activeWs + ':' + map[activeWs], bare: map[activeWs] });
    })()`);
    const pi = JSON.parse(paneInfo);
    console.log("Keys:", pi);

    // Feed with bare UUID
    const feedBare = await evaluate(`(() => {
      try {
        window.__windE2E.feedPty('${pi.bare}', '\\r\\n${MARKER}\\r\\n');
        return 'FED_BARE';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Feed bare:", feedBare);

    // Feed with composite key
    const feedComposite = await evaluate(`(() => {
      try {
        window.__windE2E.feedPty('${pi.composite}', '\\r\\n${MARKER}-C\\r\\n');
        return 'FED_COMPOSITE';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Feed composite:", feedComposite);

    await new Promise(r => setTimeout(r, 2000));

    // Read full visible text
    const visible = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${pi.composite}');
        return JSON.stringify({ total: lines?.length, all: lines });
      } catch(e) { return JSON.stringify({ err: e.message }); }
    })()`);
    console.log("Visible:", visible);

    // Also try bare
    const visibleBare = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${pi.bare}');
        return JSON.stringify({ total: lines?.length, tail: (lines||[]).slice(-5) });
      } catch(e) { return JSON.stringify({ err: e.message }); }
    })()`);
    console.log("Visible bare:", visibleBare);

    // Screenshot
    const screenshot = await send("Page.captureScreenshot", { format: "png" });
    const b64 = screenshot.result?.data;
    if (b64) {
      const fs = await import("node:fs");
      fs.writeFileSync(`artifacts/release/avd-visual/cdp-feed2-${MARKER}.png`, Buffer.from(b64, "base64"));
      console.log("Screenshot saved");
    }

    process.exit(0);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
