// scripts/avd-devtools-write-bare.mjs
import WebSocket from "ws";

const MARKER = process.argv[2] || `RIDGE-AVD-BARE-${Date.now()}`;
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
    // Get pane UUID
    const paneInfo = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      return JSON.stringify({ paneId: map[activeWs] });
    })()`);
    const pi = JSON.parse(paneInfo);
    console.log("PaneId:", pi.paneId);

    // Write with bare UUID
    const writeResult = await evaluate(`(async () => {
      try {
        await window.__windE2E.writePty('${pi.paneId}', 'echo ${MARKER}\\n');
        return 'WRITTEN';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Write result:", writeResult);

    // Wait and read with composite key
    await new Promise(r => setTimeout(r, 5000));
    const composite = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      return activeWs + ':' + map[activeWs];
    })()`);
    const visible = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${composite}');
        return JSON.stringify({ lines: lines?.length, tail: (lines||[]).slice(-15) });
      } catch(e) { return JSON.stringify({ err: e.message }); }
    })()`);
    console.log("Visible:", visible);

    // Screenshot
    const screenshot = await send("Page.captureScreenshot", { format: "png" });
    const b64 = screenshot.result?.data;
    if (b64) {
      const fs = await import("node:fs");
      fs.writeFileSync(`artifacts/release/avd-visual/cdp-bare-${MARKER}.png`, Buffer.from(b64, "base64"));
      console.log("Screenshot saved");
    }

    process.exit(0);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
