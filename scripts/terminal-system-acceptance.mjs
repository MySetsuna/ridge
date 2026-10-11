import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {chromium} from '@playwright/test';
import {resolveCdpPort,DEV_USER_DATA_DIR} from './cdp-port.mjs';
const config=JSON.parse(fs.readFileSync(path.join(DEV_USER_DATA_DIR,'tauri-dev-cdp.config.json'),'utf8'));
const targets=await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/list`).then(r=>r.json());
const target=targets.find(t=>t.type==='page'&&t.url.startsWith(config.build.devUrl));assert.ok(target);
const socket=new WebSocket(target.webSocketDebuggerUrl);await new Promise((r,j)=>{socket.onopen=r;socket.onerror=j;});
let sequence=0;const requests=new Map();socket.onmessage=e=>{const r=JSON.parse(e.data);if(r.id){requests.get(r.id)?.(r);requests.delete(r.id);}};
async function nativeEval(expression){const id=++sequence;const r=await Promise.race([new Promise(resolve=>{requests.set(id,resolve);socket.send(JSON.stringify({id,method:'Runtime.evaluate',params:{expression,awaitPromise:true,returnByValue:true}}));}),new Promise((_,reject)=>{const timer=setTimeout(()=>reject(new Error('native evaluation timeout')),20000);timer.unref();})]);assert.ok(!r.result?.exceptionDetails,'native command failed');return r.result.result.value;}
const invoke=(cmd,args={})=>nativeEval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)},${JSON.stringify(args)})`);
const fixture=path.join(DEV_USER_DATA_DIR,'held-cursor-fixture.cjs');
fs.writeFileSync(fixture,String.raw`process.stdin.setRawMode(true);process.stdin.resume();let count=0;const reset=()=>process.stdout.write('\x1b[?1049l\x1b[?1l\x1b[?25h\x1b[?12h');process.stdout.write('\x1b[?1049h\x1b[2J\x1b[H'+process.argv[2]+'\x1b[?1h\x1b[?25h\x1b[?12l\x1b[2;3H');let tail='';process.stdin.on('data',bytes=>{if(bytes.includes(3)){reset();process.exit(0);}tail+=bytes.toString('latin1');let match,consumed=0;const arrows=/\x1b(?:\[|O)C/g;while((match=arrows.exec(tail))){consumed=match.index+match[0].length;count++;process.stdout.write('\x1b['+(2+Math.floor(count/16)%4)+';'+(3+count%16)+'H');}tail=tail.slice(consumed);if(tail.length>2)tail=tail.slice(-2);});process.on('exit',reset);`);
if(process.argv.includes('--reload-frontend'))await invoke('set_remote_enabled',{enabled:false});await invoke('set_remote_enabled',{enabled:true});const host=await invoke('get_remote_info');
const nativeMode=process.argv.includes('--native');
const desktopUi=process.argv.includes('--desktop-ui');
const version=nativeMode?await fetch(`http://127.0.0.1:${resolveCdpPort()}/json/version`).then(r=>r.json()):null;
const browser=nativeMode?await chromium.connectOverCDP(version.webSocketDebuggerUrl):await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
let pane;
const evidence={route:'real current-source Host PTY + production LAN/WSS Remote',input:'trusted browser held ArrowRight keydown repeats',samples:[],limitations:['Chromium emulation, no physical handset or WAN']};
try{
 const context=nativeMode?browser.contexts()[0]:await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1440,height:1000}});const page=nativeMode?context.pages().find(page=>page.url().startsWith(config.build.devUrl)):await context.newPage();
 assert.ok(page);
 if(!nativeMode){
 await page.addInitScript(()=>{window.__RIDGE_E2E__=true;performance.setResourceTimingBufferSize(5000);});
 await page.goto(`https://127.0.0.1:${host.port}${desktopUi?'/?ui=desktop':''}`);await page.locator('input[inputmode="numeric"]').fill(host.totpCode);await page.locator(desktopUi?'.wr-card button':'.card button').click();await page.locator(desktopUi?'.rg-ime-helper':'.hidden-input:not([disabled])').first().waitFor({timeout:30000});
 }
 await page.waitForFunction(()=>window.__windE2E?.paneKeys().length>0);
 const key=await page.evaluate(()=>window.__windE2E.paneKeys()[0]);const divider=key.indexOf(':');pane=nativeMode||desktopUi?{workspaceId:await invoke('get_active_workspace_id'),paneId:key}:{workspaceId:key.slice(0,divider),paneId:key.slice(divider+1)};
 const ready=`HELD_${Date.now()}`;await invoke('write_to_pty',{...pane,data:'\x03'});await invoke('write_to_pty',{...pane,data:`node "${fixture}" ${ready}\r`});
 await page.waitForFunction(({key,ready})=>window.__windE2E.visibleText(key).join('').includes(ready),{key,ready});
 const desktopManager=desktopUi?fs.readdirSync('remote-dist/desktop/_app/immutable/chunks').find(name=>name.endsWith('.js')&&fs.readFileSync(path.join('remote-dist/desktop/_app/immutable/chunks',name),'utf8').includes('__windE2E')):null;await page.evaluate(async ({key,desktopManager})=>{const url=performance.getEntriesByType('resource').map(e=>e.name).find(u=>desktopManager?new URL(u).pathname.endsWith('/'+desktopManager):/\/manager-[^/]+\.js|\/terminal\/manager\.ts/.test(u));const module=await import(url);const Manager=Object.values(module).find(v=>typeof v==='function'&&typeof v.tryInstance==='function');const manager=Manager.tryInstance();const entry=manager.panes.get(key);assertEntry();function assertEntry(){if(!entry||entry.parked||!entry.handle)throw new Error('active rendered pane required');}window.__heldTrace={keys:[],paints:[]};window.addEventListener('keydown',event=>{if(event.key==='ArrowRight')window.__heldTrace.keys.push({at:performance.now(),repeat:event.repeat,trusted:event.isTrusted});},true);const original=entry.handle.render;entry.handle.render=function(kernel){const expected={row:kernel.cursorRow()+kernel.scrollOffset(),col:kernel.cursorCol()};const result=original.call(this,kernel);if(result&&window.__heldTrace.paints.length<512)window.__heldTrace.paints.push({at:performance.now(),expected,painted:{row:this.presentedCursorRow(),col:this.presentedCursorCol()}});return result;};},{key,desktopManager});
 await page.locator(nativeMode||desktopUi?'.rg-ime-helper':'.hidden-input').first().focus();for(let i=0;i<48;i++){await page.keyboard.down('ArrowRight');await page.waitForTimeout(50);}await page.keyboard.up('ArrowRight');
 await page.waitForFunction(key=>{const cursor=window.__windE2E.presentedCursor(key);return cursor?.row===4&&cursor.col===2;},key,{timeout:20000});
 const trace=await page.evaluate(()=>window.__heldTrace);assert.equal(trace.keys.length,48);assert.ok(trace.keys.every(k=>k.trusted));assert.ok(trace.keys.slice(1).every(k=>k.repeat));
 const movement=trace.paints.filter(p=>p.painted.row>=1&&p.painted.row<=4&&p.painted.col>=2&&p.painted.col<=17);assert.ok(movement.length>0);
 for(const paint of movement)assert.deepEqual(paint.painted,paint.expected,'each presented cursor follows current authoritative cell');
 const latencies=[];for(const paint of movement){const index=(paint.painted.row-1)*16+paint.painted.col-2;if(index>=1&&index<=48&&trace.keys[index-1])latencies.push(paint.at-trace.keys[index-1].at);}
 latencies.sort((a,b)=>a-b);evidence.route=nativeMode?'real current-source Host PTY + desktop native IPC/WebGPU':evidence.route;evidence.limitations=nativeMode?['Automated trusted WebView2 input; no physical handset or WAN']:evidence.limitations;evidence.desktopUi=desktopUi;evidence.result='PASS';evidence.keys=trace.keys.length;evidence.repeatedKeys=trace.keys.filter(k=>k.repeat).length;evidence.paintedMovementFrames=movement.length;evidence.uniquePresentedCells=new Set(movement.map(p=>`${p.painted.row}:${p.painted.col}`)).size;evidence.keyToPaintP50Ms=latencies[Math.floor(latencies.length*.5)];evidence.keyToPaintP95Ms=latencies[Math.floor(latencies.length*.95)];evidence.samples=movement.slice(0,64);await page.screenshot({path:path.join(DEV_USER_DATA_DIR,nativeMode?'held-cursor-native.png':desktopUi?'held-cursor-desktop-ui.png':'held-cursor-real-host.png')});
}finally{if(pane)await invoke('write_to_pty',{...pane,data:'\x03'}).catch(()=>{});await browser.close();socket.close();fs.writeFileSync(path.join(DEV_USER_DATA_DIR,nativeMode?'held-cursor-native.json':desktopUi?'held-cursor-desktop-ui.json':'held-cursor-real-host.json'),JSON.stringify(evidence,null,2));}
console.log(JSON.stringify({...evidence,samples:undefined},null,2));
