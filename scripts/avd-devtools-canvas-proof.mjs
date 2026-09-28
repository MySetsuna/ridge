// scripts/avd-devtools-canvas-proof.mjs
// Before/after canvas pixel proof: feed marker → verify canvas changed.
import WebSocket from "ws";
import { writeFileSync } from "node:fs";

const MARKER = process.argv[2] || `RIDGE-AVD-PROOF-${Date.now()}`;
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

async function screenshot(name) {
  const r = await send("Page.captureScreenshot", { format: "png" });
  const b64 = r.result?.data;
  if (b64) {
    const path = `artifacts/release/avd-visual/proof-${name}.png`;
    writeFileSync(path, Buffer.from(b64, "base64"));
    return path;
  }
  return null;
}

ws.on("open", async () => {
  try {
    // Get composite key
    const composite = await evaluate(`(() => {
      const map = JSON.parse(localStorage.getItem('rg-remote-pane-map:lan:localhost:9620') || '{}');
      const activeWs = localStorage.getItem('rg-remote-active-ws:lan:localhost:9620');
      return activeWs + ':' + map[activeWs];
    })()`);
    console.log("Composite:", composite);

    // BEFORE: screenshot + visible text
    const beforeShot = await screenshot("before");
    const beforeText = await evaluate(`(() => {
      try { return JSON.stringify(window.__windE2E.visibleText('${composite}')?.slice(-5)); }
      catch(e) { return JSON.stringify({err:e.message}); }
    })()`);
    console.log("Before text:", beforeText);

    // FEED marker
    const feed = await evaluate(`(() => {
      try {
        window.__windE2E.feedPty('${composite}', '\\r\\n${MARKER}\\r\\n');
        return 'FED';
      } catch(e) { return 'ERR:' + e.message; }
    })()`);
    console.log("Feed:", feed);

    // Wait for repaint
    await new Promise(r => setTimeout(r, 3000));

    // AFTER: screenshot + visible text
    const afterShot = await screenshot("after");
    const afterText = await evaluate(`(() => {
      try {
        const lines = window.__windE2E.visibleText('${composite}');
        return JSON.stringify({ total: lines?.length, tail: (lines||[]).slice(-10) });
      } catch(e) { return JSON.stringify({err:e.message}); }
    })()`);
    console.log("After text:", afterText);

    // Canvas element screenshot (isolated)
    const canvasShot = await evaluate(`(() => {
      const c = document.querySelector('canvas');
      if (!c) return 'NO_CANVAS';
      return c.toDataURL('image/png').slice(0, 50) + '...';
    })()`);
    console.log("Canvas:", canvasShot);

    // Verify marker in visible text
    const hasMarker = afterText.includes(MARKER);
    console.log("MARKER_IN_MODEL:", hasMarker);

    // Pixel diff
    const fs = await import("node:fs");
    const before = fs.readFileSync(beforeShot);
    const after = fs.readFileSync(afterShot);
    console.log("Before size:", before.length, "After size:", after.length);
    console.log("Shots differ:", before.length !== after.length || !before.equals(after));

    process.exit(hasMarker ? 0 : 1);
  } catch (e) {
    console.error("Error:", e.message);
    process.exit(1);
  }
});

ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
