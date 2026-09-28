// scripts/avd-devtools-probe.mjs — evaluate JS in the AVD Chrome via DevTools
import WebSocket from "ws";

const ws = new WebSocket("ws://localhost:9222/devtools/page/2");
ws.on("open", () => {
  ws.send(JSON.stringify({
    id: 1,
    method: "Runtime.evaluate",
    params: {
      expression: `JSON.stringify({
        ridgeE2E: window.__RIDGE_E2E__,
        hasHooks: !!window.__windE2E,
        hookKeys: window.__windE2E ? Object.keys(window.__windE2E) : [],
        debugDiv: document.getElementById('exec-debug')?.textContent || 'NONE',
        resultDiv: document.getElementById('exec-result')?.textContent || 'NONE',
        dbg: localStorage.getItem('rg-remote-debug-state')?.slice(0,200) || 'NONE'
      })`,
      returnByValue: true,
    },
  }));
});
ws.on("message", (data) => {
  const msg = JSON.parse(data.toString());
  if (msg.id === 1) {
    console.log(msg.result?.result?.value || JSON.stringify(msg));
    ws.close();
    process.exit(0);
  }
});
ws.on("error", (e) => { console.error("WS error:", e.message); process.exit(1); });
setTimeout(() => { console.log("timeout"); process.exit(1); }, 5000);
