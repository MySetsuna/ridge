// scripts/avd-devtools-probe2.mjs — deeper state check
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
        debugDiv: document.getElementById('exec-debug')?.textContent || 'NONE',
        // Check for canvas elements
        canvases: [...document.querySelectorAll('canvas')].map(c=>({w:c.width,h:c.height})),
        // Check for hidden-input
        hiddenInput: !!document.querySelector('.hidden-input'),
        hiddenInputDisabled: document.querySelector('.hidden-input')?.disabled,
        // Check body text
        bodyText: (document.body?.innerText||'').slice(0,300),
        // Check if app div has content
        appChildren: document.getElementById('app')?.children.length || 0
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
