// scripts/avd-devtools-exec-marker.mjs
// Write marker to PTY via composite pane key and read back visible text.
import WebSocket from "ws";

const MARKER = process.argv[2] || `RIDGE-AVD-FINAL-${Date.now()}`;
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
    // Step 1: Find the composite pane key
    const paneInfo = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      const paneId = map[activeWs];
      const composite = activeWs && paneId ? activeWs + ':' + paneId : null;
      return JSON.stringify({ activeWs, paneId, composite });
    })()`);
    console.log("Pane info:", paneInfo);
    const pi = JSON.parse(paneInfo);

    if (!pi.composite) {
      console.log("FATAL: no composite key found");
      process.exit(1);
    }

    // Step 2: Try visibleText with composite key
    const visCheck = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${pi.composite}');
        return JSON.stringify({ lines: lines?.length ?? -1, sample: (lines||[]).slice(-5) });
      } catch(e) { return JSON.stringify({ err: e.message }); }
    })()`);
    console.log("Visible text (composite):", visCheck);

    // Step 3: Write marker to PTY
    const writeResult = await evaluate(`(async () => {
      try {
        await window.__windE2E.writePty('${pi.composite}', 'echo ${MARKER}\\n');
        return 'WRITTEN';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Write result:", writeResult);

    // Step 4: Wait and read visible text
    await new Promise(r => setTimeout(r, 5000));
    const visible = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${pi.composite}');
        return JSON.stringify({ lines: lines?.length, tail: (lines||[]).slice(-10) });
      } catch(e) { return JSON.stringify({ err: e.message }); }
    })()`);
    console.log("Visible after write:", visible);

    // Step 5: Screenshot
    const screenshot = await send("Page.captureScreenshot", { format: "png" });
    const b64 = screenshot.result?.data;
    if (b64) {
      const fs = await import("node:fs");
      fs.writeFileSync(`artifacts/release/avd-visual/cdp-marker-${MARKER}.png`, Buffer.from(b64, "base64"));
      console.log("Screenshot saved");
    }

    process.exit(0);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
