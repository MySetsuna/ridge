// Real task-owned Host acceptance: desktop session recovery and semantic history.
// Usage: node scripts/terminal-history-session-acceptance.mjs [--native]
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { chromium } from '@playwright/test';
import { resolveCdpPort, DEV_USER_DATA_DIR } from './cdp-port.mjs';
const config=JSON.parse(fs.readFileSync(`${DEV_USER_DATA_DIR}/tauri-dev-cdp.config.json`,'utf8'));
const targets=await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/list`).then(r=>r.json());
const target=targets.find(t=>t.type==='page'&&t.url.startsWith(config.build.devUrl));
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r,j)=>{socket.onopen=r;socket.onerror=j;});
let sequence=0;const pending=new Map();socket.onmessage=e=>{const message=JSON.parse(e.data);if(message.id){pending.get(message.id)?.(message);pending.delete(message.id);}};
async function invoke(cmd,args={}) {const id=++sequence;const reply=await new Promise(resolve=>{pending.set(id,resolve);socket.send(JSON.stringify({id,method:'Runtime.evaluate',params:{expression:`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},${JSON.stringify(args)})`,awaitPromise:true,returnByValue:true}}));});assert.ok(!reply.result?.exceptionDetails);return reply.result.result.value;}
const native=process.argv.includes('--native');
const version=native?await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/version`).then(r=>r.json()):null;
const browser=native?await chromium.connectOverCDP(version.webSocketDebuggerUrl):await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
const managerAsset=fs.readdirSync('remote-dist/desktop/_app/immutable/chunks').find(name=>name.endsWith('.js')&&fs.readFileSync('remote-dist/desktop/_app/immutable/chunks/'+name,'utf8').includes('static tryInstance('));
const evidence={route:native?'native IPC/WebView2':'production explicit desktop LAN/WSS',checks:[]};
let page;let dropTerminalFrames=false;
try {
 await invoke('set_remote_enabled',{enabled:true});
 const context=native?browser.contexts()[0]:await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1440,height:1000}});
 page=native?context.pages().find(p=>p.url().startsWith(config.build.devUrl)):await context.newPage();
 let failWorkspace=false;let verificationRequests=0;
 if(!native) {
  await context.routeWebSocket(/\/ws(?:\?|$)/,route=>{
   const server=route.connectToServer();const failIds=new Set();
   route.onMessage(message=>{if(typeof message==='string'){const req=JSON.parse(message);if(failWorkspace&&(req.method==='list_workspaces'||req.cmd==='list_workspaces')){failIds.add(req.id??req._reqId);failWorkspace=false;}}server.send(message);});
   server.onMessage(message=>{if(dropTerminalFrames&&typeof message!=='string')return;if(typeof message==='string'){const response=JSON.parse(message);const id=response.id??response._reqId;if(failIds.delete(id)){route.send(JSON.stringify({jsonrpc:'2.0',id,error:{code:-32603,message:'temporary fixture read failure'}}));return;}}route.send(message);});
  });
  page.on('request',r=>{if(new URL(r.url()).pathname==='/verify')verificationRequests++;});
  await page.addInitScript(()=>{window.__RIDGE_E2E__=true;performance.setResourceTimingBufferSize(5000);});
  const host=await invoke('get_remote_info');
  await page.goto(`https://127.0.0.1:${host.port}/?ui=desktop`);
  await page.locator('input[inputmode="numeric"]').fill(host.totpCode);await page.locator('.wr-card button').click();
 }
 await page.waitForFunction(()=>window.__windE2E?.paneKeys().length>0,{},{timeout:30000});
 if(!native){
  const token=await page.evaluate(()=>localStorage.getItem('ridge_remote_token'));assert.ok(token);
  failWorkspace=true;await page.reload();
  await page.locator('.wr-error').waitFor({timeout:20000});
  assert.equal(await page.evaluate(()=>localStorage.getItem('ridge_remote_token')),token);
  assert.equal(await page.locator('input[inputmode="numeric"]').count(),0);
  await page.locator('.wr-gate button').click();
  await page.waitForFunction(()=>window.__windE2E?.paneKeys().length>0,{},{timeout:30000});
  assert.equal(verificationRequests,1);evidence.checks.push('temporary workspace failure retains session and retry issues no OTP');
  await page.reload();await page.waitForFunction(()=>window.__windE2E?.paneKeys().length>0,{},{timeout:30000});
  assert.equal(verificationRequests,1);evidence.checks.push('valid reload uses session without OTP');
 }
 const key=await page.evaluate(()=>window.__windE2E.paneKeys()[0]);
 const pane={workspaceId:await invoke('get_active_workspace_id'),paneId:key};
 const fixture=`${DEV_USER_DATA_DIR}/history-fixture.cjs`;const marker=`HISTORY_${Date.now()}`;
 fs.writeFileSync(fixture,`process.stdout.write('\\x1b[?1049l\\x1b[2J\\x1b[H'+Array.from({length:600},(_,i)=>'${marker}_'+i+'\\r\\n').join(''));`);
 await invoke('write_to_pty',{...pane,data:'\x03'});
 await invoke('write_to_pty',{...pane,data:`node "${fixture}"\r`});
 await page.waitForFunction(({key,marker})=>window.__windE2E.visibleText(key).join('').includes(marker+'_599'),{key,marker},{timeout:20000});
 if(native){const cdp=await context.newCDPSession(page);await cdp.send('Page.enable');await cdp.send('Page.navigate',{url:config.build.devUrl+'/?historyNonce='+Date.now()});}
 else await page.reload();
 await page.waitForFunction(()=>window.__windE2E?.paneKeys().length>0,{},{timeout:30000});
 await page.waitForFunction(key=>window.__windE2E.scrollbackLen(key)>0,key,{timeout:20000});
 const initial=await page.evaluate(key=>window.__windE2E.scrollbackLen(key),key);
 assert.ok(initial>0&&initial<=256,`bounded tail: ${initial}`);
 const canvas=page.locator(`[data-rg-pane-id="${key}"]`).first();await canvas.waitFor();const bounds=await canvas.boundingBox();assert.ok(bounds);
 await page.mouse.move(bounds.x+bounds.width/2,bounds.y+bounds.height/2);await page.mouse.wheel(0,-10000);
 await page.waitForFunction(({key,initial})=>window.__windE2E.scrollbackLen(key)>initial,{key,initial},{timeout:15000});
 const after=await page.evaluate(key=>window.__windE2E.scrollbackLen(key),key);
 // Pages may be shorter at the retained boundary, including ConPTY's coalesced output.
 assert.ok(after-initial>0&&after-initial<=128);
 evidence.history={initialRows:initial,afterRows:after,pageRows:after-initial,maxPageRows:128};evidence.checks.push('real PTY bounded snapshot and bounded semantic page on trusted wheel');
 if(!native){
  const anchor=await page.evaluate(async ({key,managerAsset})=>{const url=performance.getEntriesByType('resource').map(e=>e.name).find(url=>new URL(url).pathname.endsWith('/'+managerAsset));const module=await import(url);const Manager=Object.values(module).find(value=>typeof value==='function'&&typeof value.tryInstance==='function');const manager=Manager.tryInstance();const kernel=manager.getKernel(key);window.__historyRecoveryManager=manager;kernel.setSelection(0,0,0,4);return {top:kernel.dumpVisibleText()[0],selection:kernel.getSelectionText(),context:kernel.terminalInputContext(),cols:kernel.cols(),rows:kernel.rows(),request:kernel.terminalHistoryRequest(128)};},{key,managerAsset});
  evidence.anchorBefore=anchor;dropTerminalFrames=true;const gapMarker='RECOVERY_'+Date.now();fs.writeFileSync(fixture,`process.stdout.write(Array.from({length:400},(_,i)=>'${gapMarker}_'+i+'\\r\\n').join('')+'${gapMarker}_DONE\\r\\n');`);
  await invoke('write_to_pty',{...pane,data:`node "${fixture}"\r`});
  let recoveredSnapshot;const deadline=Date.now()+20000;
  while(Date.now()<deadline){const snapshot=await invoke('get_terminal_snapshot',{...pane,activationId:anchor.context[2]});const done=await page.evaluate(({key,snapshot,pane,activation,gapMarker})=>{const current=window.__historyRecoveryManager.getKernel(key);const observer=new current.constructor(48,120,2000);try{observer.applyRemoteFrame(Uint8Array.from(atob(snapshot),ch=>ch.charCodeAt(0)),pane.workspaceId,pane.paneId,activation);return observer.dumpVisibleText().some(line=>line.trim()===gapMarker+'_DONE');}finally{observer.free();}},{key,snapshot,pane,activation:anchor.context[2],gapMarker});if(done){recoveredSnapshot=snapshot;break;}await new Promise(resolve=>setTimeout(resolve,50));}
  assert.ok(recoveredSnapshot,'real Host gap output reached final row');
  await page.evaluate(({key,snapshot,pane,activation})=>window.__historyRecoveryManager.applyRemoteFrame(key,Uint8Array.from(atob(snapshot),ch=>ch.charCodeAt(0)),pane.workspaceId,pane.paneId,activation),{key,snapshot:recoveredSnapshot,pane,activation:anchor.context[2]});
  evidence.anchorAfterSnapshot=await page.evaluate(key=>{const kernel=window.__historyRecoveryManager.getKernel(key);return {pending:kernel.historyRecoveryPending(),top:kernel.dumpVisibleText()[0],selection:kernel.getSelectionText(),len:kernel.scrollbackLen(),offset:kernel.scrollOffset(),cols:kernel.cols(),rows:kernel.rows(),request:kernel.terminalHistoryRequest(128)};},key);
  await page.waitForFunction(({key,anchor})=>{const kernel=window.__historyRecoveryManager.getKernel(key);return !kernel.historyRecoveryPending()&&kernel.dumpVisibleText()[0]===anchor.top&&kernel.getSelectionText()===anchor.selection;},{key,anchor},{timeout:20000});
  dropTerminalFrames=false;evidence.checks.push('mounted desktop component automatically pages disjoint snapshot recovery to the original viewport and selection');
 }
 evidence.result='PASS';
} catch(error) {evidence.error=error.message;evidence.diagnostic=await page?.evaluate(()=>({panes:window.__windE2E?.paneKeys().map(key=>({key,len:window.__windE2E.scrollbackLen(key),tail:window.__windE2E.visibleText(key).filter(s=>s.trim()).slice(-4),recovery:window.__historyRecoveryManager?(()=>{const kernel=window.__historyRecoveryManager.getKernel(key);return {pending:kernel.historyRecoveryPending(),offset:kernel.scrollOffset(),top:kernel.dumpVisibleText()[0],selection:kernel.getSelectionText()};})():null}))})).catch(()=>null);await page?.screenshot({path:`${DEV_USER_DATA_DIR}/history-failure.png`}).catch(()=>{});throw error;} finally {await browser.close();socket.close();fs.writeFileSync(`${DEV_USER_DATA_DIR}/desktop-session-history${native?'-native':''}.json`,JSON.stringify(evidence,null,2));}
console.log(JSON.stringify(evidence,null,2));
