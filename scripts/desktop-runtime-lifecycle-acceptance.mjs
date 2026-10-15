// Owned debug desktop fixture. Installed applications and their Runtime are untouched.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,writeFile,mkdir,open,copyFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {randomUUID} from 'node:crypto';
import {chromium} from '@playwright/test';
import {WebSocket} from 'ws';
import {measureRuntimeClients,distribution,assertRuntimePerformance} from './lib/runtimePerformance.mjs';
import {verifyRuntimeHostReconnectUi} from './lib/runtimeHostReconnectUi.mjs';
const execute=promisify(execFile);
const root=await mkdtemp(join(tmpdir(),'ridge-desktop-lifecycle-'));
const data=join(root,'dev-kernel');await mkdir(data);
const report={mode:'owned-debug-desktop',completeChange:false,checks:[]};
const delay=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message,ms=30000){const end=Date.now()+ms;while(Date.now()<end){if(await check())return;await delay(100);}throw new Error(message);}
const env={...process.env,RIDGE_KERNEL_DATA_DIR:data,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_WORKSPACE_SAVE_DIR:join(root,'snapshots'),RIDGE_CDP_USER_DATA_DIR:root,RIDGE_CDP_STARTUP_CWD:root,RIDGE_CDP_ALLOW_NON_BREAKAWAY:'1',RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_TEST_TLS_DIR:join(root,'tls')};
env.RIDGE_HOST_TRACE='0';
env.RIDGE_KERNEL_TRACE=process.env.RIDGE_PERFORMANCE_DIAGNOSTICS==='1'?'1':'0';
env.RIDGE_CLI_PERF_TRACE=env.RIDGE_KERNEL_TRACE;
let cliBinary=resolve('target/070-cli-controller/debug/ridge.exe');
if(process.env.RIDGE_LIFECYCLE_CLI_SOURCE){cliBinary=join(root,'fixture-cli.exe');await copyFile(resolve(process.env.RIDGE_LIFECYCLE_CLI_SOURCE),cliBinary);}
const pinnedDesktop=process.env.RIDGE_LIFECYCLE_DESKTOP_SOURCE;
const desktopBinary=pinnedDesktop?join(root,'fixture-desktop.exe'):resolve('target/debug/ridge.exe');
let pinnedDevUrl;
if(pinnedDesktop){
 assert.ok(process.env.RIDGE_LIFECYCLE_DESKTOP_CONFIG,'pinned desktop needs its matching compiled dev config');
 const config=JSON.parse(await readFile(resolve(process.env.RIDGE_LIFECYCLE_DESKTOP_CONFIG),'utf8'));
 pinnedDevUrl=new URL(config.build.devUrl);
 assert.equal(pinnedDevUrl.hostname,'127.0.0.1');assert.equal(pinnedDevUrl.protocol,'http:');assert.ok(Number(pinnedDevUrl.port)>0);
 await copyFile(resolve(pinnedDesktop),desktopBinary);
 await writeFile(join(root,'tauri-dev-cdp.config.json'),JSON.stringify(config));
}
delete env.RIDGE_NATIVE_RUNTIME;
const kernelBinary=join(root,'fixture-kernel.exe');
await copyFile(resolve(process.env.RIDGE_LIFECYCLE_KERNEL_SOURCE||'target/debug/ridge-kernel.exe'),kernelBinary);
const kernelLog=await open(join(root,'kernel.log'),'a');
const kernel=spawn(kernelBinary,[],{env,stdio:['ignore',kernelLog.fd,kernelLog.fd],windowsHide:true});await kernelLog.close();
let endpoint,launcher,browser,desktop,lan,reopened,reopenVite;
async function request(path,body,method='POST'){
 const response=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
 assert.equal(response.status,200);return response.json();
}
const connection=randomUUID();
async function command(method,args={}){const response=await request('/v1/domain/commands',{method,args,native:true,roots:[],connectionId:connection});assert.equal(response.ok,true,`${method}: ${response.error?.message}`);return response.value;}
async function processes(){const {stdout}=await execute('powershell',['-NoProfile','-Command',"Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CreationDate | ConvertTo-Json -Compress"],{windowsHide:true,maxBuffer:4*1024*1024});return JSON.parse(stdout);}
async function setDesktopMinimized(minimized){
 assert.ok(desktop?.ProcessId,'owned desktop PID missing');
 const owned=(await processes()).find(process=>process.ProcessId===desktop.ProcessId);
 assert.equal(owned?.ExecutablePath?.toLowerCase(),desktopBinary.toLowerCase());
 const script=`Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class OwnedWindow { [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int command); [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h); }'; $ownedProcess=Get-Process -Id ${desktop.ProcessId} -ErrorAction Stop; $handle=$ownedProcess.MainWindowHandle; if ($handle -eq 0) { throw 'Owned desktop window missing' }; [OwnedWindow]::ShowWindow($handle,${minimized?6:9}) | Out-Null; Start-Sleep -Milliseconds 150; [OwnedWindow]::IsIconic($handle) | ConvertTo-Json`;
 const {stdout}=await execute('powershell',['-NoProfile','-Command',script],{windowsHide:true});
 assert.equal(JSON.parse(stdout.trim()),minimized,'owned desktop minimization state differs');
 return {pid:desktop.ProcessId,minimized,verifiedBy:'Win32 IsIconic'};
}
async function launcherChildren(){
 const inventory=await processes(),owned=new Set([launcher,reopened,reopenVite].filter(child=>child&&child.exitCode===null&&child.signalCode===null).map(child=>child.pid));let changed=true;
 while(changed){changed=false;for(const process of inventory){if(owned.has(process.ParentProcessId)&&!owned.has(process.ProcessId)){owned.add(process.ProcessId);changed=true;}}}
 return inventory.filter(process=>owned.has(process.ProcessId));
}
let rpcId=0;
async function desktopProjection(cdp){
 const version=await (await fetch(`http://127.0.0.1:${cdp}/json/version`)).json();
 assert.ok(version.webSocketDebuggerUrl?.startsWith(`ws://127.0.0.1:${cdp}/`),'unexpected CDP websocket');
 browser=await chromium.connectOverCDP(version.webSocketDebuggerUrl);let page;
 await until(async()=>{page=browser.contexts().flatMap(context=>context.pages())[0];return Boolean(page);},'owned desktop WebView missing');
 await until(async()=>page.evaluate(()=>Boolean(window.__TAURI_INTERNALS__)).catch(()=>false),'native OS bridge unavailable');
 let projected;
 await until(async()=>{
  try{projected=await page.evaluate(async()=>{
   const core=await import('/src/lib/transport/nativeRuntimeCore.ts');
   return core.invoke('list_workspaces');
  });return Array.isArray(projected);}catch{return false;}
 },'desktop Runtime bridge did not connect',60000).catch(async error=>{
  const state=await page.evaluate(()=>({url:location.href,text:document.body.innerText.slice(0,2000)}));
  console.error('Owned desktop bootstrap state:',JSON.stringify(state));throw error;
 });
 return {page,projected};
}
function lanCommand(method,params){
 const id=++rpcId;
 return new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>finish(new Error(`LAN ${method} timed out`)),10000);
  const finish=(error,value)=>{clearTimeout(timer);lan.off('message',message);error?reject(error):resolve(value);};
  const message=bytes=>{let response;try{response=JSON.parse(bytes.toString());}catch{return;}if(response.id===id)finish(response.error?new Error(JSON.stringify(response.error)):null,response.result);};
  lan.on('message',message);lan.send(JSON.stringify({jsonrpc:'2.0',id,method,params}));
 });
}
try{
 await until(async()=>{try{endpoint=JSON.parse(await readFile(join(data,'kernel.json'),'utf8'));return endpoint.pid===kernel.pid;}catch{return false;}},'fixture Kernel did not start');
 const workspace=(await request('/v1/domain/workspaces',{name:'Original desktop session'})).workspace_id;
 const pane=randomUUID();await request(`/v1/domain/workspaces/${workspace}/topology`,{pane_tree:{root:{Leaf:pane},panes:{[pane]:{id:pane,mode:'Terminal',cwd:root}}}},'PUT');
 const spawned=await request('/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:'cmd.exe',args:['/d','/q','/k'],cwd:root,env:{},role:'shell',cols:80,rows:24});assert.equal(spawned.ok,true);
 const before=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);assert.ok(before?.child_pid);
 await command('set_remote_enabled',{enabled:true,port:0});const info=await command('get_remote_info');assert.equal(info.remoteEnabled,true);
 lan=new WebSocket(`wss://127.0.0.1:${info.port}/ws?code=${encodeURIComponent(info.totpCode)}&device=owned-lifecycle`,{ca:await readFile(join(root,'tls','ca.pem'))});await once(lan,'open');
 const log=await open(join(root,'desktop.log'),'a');
 if(pinnedDesktop){
  const viteLog=await open(join(root,'initial-vite.log'),'a');
  reopenVite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',pinnedDevUrl.port,'--strictPort'],{env,stdio:['ignore',viteLog.fd,viteLog.fd],windowsHide:true});await viteLog.close();
  await until(async()=>{assert.equal(reopenVite.exitCode,null,'pinned dev server exited');try{return (await fetch(pinnedDevUrl,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}},'pinned dev server did not start');
  launcher=spawn(desktopBinary,[],{env:{...env,RIDGE_DISABLE_SINGLE_INSTANCE:'1',RIDGE_CDP_NATIVE_DATA_DIR:join(root,'native-data'),RIDGE_KERNEL_HOST_BINARY:kernelBinary,
   RIDGE_REMOTE_UI_ROOT:resolve('remote-dist'),WEBVIEW2_USER_DATA_FOLDER:root,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*'},stdio:['ignore',log.fd,log.fd],windowsHide:true});
 }else launcher=spawn(process.execPath,['scripts/tauri-dev-cdp.mjs'],{env,stdio:['ignore',log.fd,log.fd],windowsHide:true});
 await log.close();
 let cdp;
 await until(async()=>{
  assert.equal(launcher.exitCode,null,'desktop launcher exited before CDP');
  try{cdp=Number((await readFile(pinnedDesktop?join(root,'EBWebView','DevToolsActivePort'):join(root,'cdp-port.txt'),'utf8')).split('\n')[0].trim());return cdp>0;}catch{return false;}
 },'owned desktop CDP did not start',600000);
 desktop=(await launcherChildren()).find(process=>process.ExecutablePath?.toLowerCase()===desktopBinary.toLowerCase());
 assert.ok(desktop,'exact owned desktop PID could not be attested');assert.notEqual(desktop.ProcessId,endpoint.pid);
 const {page,projected}=await desktopProjection(cdp);
 assert.ok(projected.some(row=>row.id===workspace),'desktop Runtime client omitted original workspace');
 const boundaries=await page.evaluate(async()=>{
  const core=await import('/src/lib/transport/nativeRuntimeCore.ts');
  let oldBusinessRejected=false;
  try{await window.__TAURI_INTERNALS__.invoke('get_theme_data');}catch{oldBusinessRejected=true;}
  const theme=await core.invoke('get_theme_data');
  let inactiveCloudRejected=false;
  try{await core.invoke('enter_deep_root_mode');}catch(error){inactiveCloudRejected=String(error).includes('NO_ACTIVE_CLOUD_REMOTE');}
  return {oldBusinessRejected,theme,inactiveCloudRejected};
 });
 assert.equal(boundaries.oldBusinessRejected,true);
 assert.equal(boundaries.inactiveCloudRejected,true);
 assert.ok(Array.isArray(boundaries.theme?.themes),'shared theme response omitted catalog');
 assert.deepEqual(boundaries.theme,await command('get_theme_data'),'desktop theme differs from shared Runtime');
 report.checks.push('old desktop business IPC rejected; shared theme and Runtime Cloud guard remain functional');
 const graph=await request(`/v1/domain/workspaces/${workspace}`,undefined,'GET');assert.ok(graph.panes.includes(pane),'desktop bootstrap replaced original workspace graph');
 report.checks.push('desktop bootstrap retains pre-existing workspace/pane');
 report.hostReconnectUi=await verifyRuntimeHostReconnectUi({page,root,command});
 report.checks.push(...report.hostReconnectUi.checks);
 // Real interactive CLI inside an owned ConPTY; it connects through LAN to the original shell.
 const cliPane=randomUUID();
 const cliInfo=await command('get_remote_info');
 const cliLaunch=await request('/v1/domain/ptys',{pty_id:cliPane,program:cliBinary,args:['connect',`127.0.0.1:${info.port}`,'--code',cliInfo.totpCode],cwd:root,env:{RIDGE_TEST_TLS_DIR:join(root,'tls')},role:'shell',cols:80,rows:24});
 assert.equal(cliLaunch.ok,true,'real CLI launch failed');
 const cliBefore=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===cliPane);
 assert.ok(cliBefore?.child_pid);
 assert.equal((await processes()).find(process=>process.ProcessId===cliBefore.child_pid)?.ExecutablePath?.toLowerCase(),cliBinary.toLowerCase(),'CLI executable identity differs from the owned artifact');
 await until(async()=>{
  const sessions=await command('list_remote_sessions');
  return sessions.some(session=>session.deviceId==='ridge-cli');
 },'CLI did not authenticate to the shared LAN Host');
 const cliInput=async(label)=>{
  const path=join(root,`${label}.txt`),data=`echo ${label} > "${path.replaceAll('\\','/')}"\r\n`;
  for(let offset=0;offset<data.length;offset+=8){
   const result=await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from(data.slice(offset,offset+8)).toString('base64')});
   assert.equal(result.ok,true);await delay(10);
  }
  await until(async()=>{try{return (await readFile(path,'utf8')).trim()===label;}catch{return false;}},'CLI input did not execute in original child');
  const visible=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
  assert.equal(visible.ok,true);assert.ok(Buffer.from(visible.data_b64,'base64').length,'CLI had no output');
 };
 // Authentication precedes pane subscription; wait for actual original shell prompt in CLI output.
 await until(async()=>{
  const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
  return Buffer.from(output.data_b64??'','base64').toString().includes(root);
 },'CLI did not display original shell output');
 await cliInput('CLI_BEFORE_DESKTOP_EXIT');
 report.checks.push('real interactive CLI and desktop operate the original child concurrently');
 if(process.env.RIDGE_RUNTIME_PERFORMANCE==='1'){
  const devUrl=JSON.parse(await readFile(join(root,'tauri-dev-cdp.config.json'),'utf8')).build.devUrl;
  const fresh=await command('get_remote_info');
  report.performance=await measureRuntimeClients({page,root,workspace,pane,cliPane,port:info.port,code:fresh.totpCode,request,command,devUrl,processes,setDesktopMinimized,diagnostics:env.RIDGE_KERNEL_TRACE==='1',binaries:{kernel:kernelBinary,cli:cliBinary,desktop:desktopBinary,terminalWasm:resolve('packages/ridge-term/pkg/ridge_term_bg.wasm'),rendererWasm:resolve('packages/ridge-term-render/pkg/ridge_term_render_bg.wasm'),fontWasm:resolve('packages/ridge-term-font/pkg/ridge_term_font_bg.wasm')}});
  const trace=(await readFile(join(root,'kernel.log'),'utf8')).replace(/\x1b\[[0-?]*[ -/]*[@-~]/g,'').split('\n').filter(line=>line.includes(pane)&&line.includes('parse_feed_us'));
  report.performance.kernelParseFeedUs=trace.map(line=>Number(line.match(/parse_feed_us[=\s]+(\d+)/)?.[1])).filter(Number.isFinite);
  report.performance.kernelParseFeedSummaryUs=distribution(report.performance.kernelParseFeedUs);
  await writeFile(join(root,'performance.json'),JSON.stringify(report.performance,null,2));
  assertRuntimePerformance(report.performance);
  if(lan.readyState!==WebSocket.OPEN){
   const resumed=await command('get_remote_info');
   lan=new WebSocket(`wss://127.0.0.1:${resumed.port}/ws?code=${encodeURIComponent(resumed.totpCode)}&device=owned-lifecycle`,{ca:await readFile(join(root,'tls','ca.pem'))});await once(lan,'open');
  }
 }
 const terminatedDesktopProcesses=await launcherChildren();
 assert.ok(terminatedDesktopProcesses.some(process=>process.ProcessId===desktop.ProcessId));
 assert.ok(terminatedDesktopProcesses.every(process=>process.ProcessId!==kernel.pid),'desktop cleanup included the independent Runtime');
 await execute('powershell',['-NoProfile','-Command',`Stop-Process -Id ${terminatedDesktopProcesses.map(process=>process.ProcessId).join(',')} -Force -ErrorAction SilentlyContinue`],{windowsHide:true});
 await until(async()=>!(await processes()).some(process=>terminatedDesktopProcesses.some(original=>original.ProcessId===process.ProcessId&&original.CreationDate===process.CreationDate)),'owned desktop process tree did not exit');
 report.checks.push('entire attested desktop/WebView/dev-server process tree exits before profile reuse');
 assert.equal((await request('/v1/health',undefined,'GET')).pid,endpoint.pid);assert.equal((await command('get_remote_info')).remoteEnabled,true);
 const after=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
 assert.equal(after.child_pid,before.child_pid);assert.equal(after.child_start_instance,before.child_start_instance);
 report.checks.push('forced desktop death preserves exact Runtime, PTY instance and LAN listener');
 await cliInput('CLI_AFTER_DESKTOP_EXIT');
 const cliAfter=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===cliPane);
 assert.equal(cliAfter.child_pid,cliBefore.child_pid);assert.equal(cliAfter.child_start_instance,cliBefore.child_start_instance);
 report.cliPid=cliAfter.child_pid;
 report.checks.push('real interactive CLI writes to the same child after desktop exit');
 const output=join(root,'after-desktop.txt');await lanCommand('write_to_pty',{workspaceId:workspace,paneId:pane,data:`echo AFTER_DESKTOP_EXIT > "${output.replaceAll('\\','/')}"\r\n`});
 await until(async()=>{try{return (await readFile(output,'utf8')).includes('AFTER_DESKTOP_EXIT');}catch{return false;}},'original child did not reply after desktop death');
 assert.equal(lan.readyState,WebSocket.OPEN);report.checks.push('LAN writes to same child after desktop exit');
 try{await browser.close();}catch{}browser=undefined;
 const originalHelpers=await launcherChildren();assert.ok(originalHelpers.every(process=>process.ProcessId!==kernel.pid));
 if(originalHelpers.length)await execute('powershell',['-NoProfile','-Command',`Stop-Process -Id ${originalHelpers.map(process=>process.ProcessId).join(',')} -Force -ErrorAction SilentlyContinue`],{windowsHide:true}).catch(()=>{});
 const devUrl=new URL(JSON.parse(await readFile(join(root,'tauri-dev-cdp.config.json'),'utf8')).build.devUrl);
 assert.equal(devUrl.hostname,'127.0.0.1');assert.equal(devUrl.protocol,'http:');assert.ok(Number(devUrl.port)>0);
 const frontendLive=async()=>{try{return (await fetch(devUrl,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}};
 await until(async()=>!await frontendLive(),'original owned dev server did not stop');
 const viteLog=await open(join(root,'reopen-vite.log'),'a');
 reopenVite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',devUrl.port,'--strictPort'],{env,stdio:['ignore',viteLog.fd,viteLog.fd],windowsHide:true});await viteLog.close();
 await until(async()=>{assert.equal(reopenVite.exitCode,null,'reopen dev server exited');return frontendLive();},'reopen dev server did not start');
 const reopenLog=await open(join(root,'reopen.log'),'a');
 reopened=spawn(desktopBinary,[],{env:{...env,RIDGE_DISABLE_SINGLE_INSTANCE:'1',
  RIDGE_CDP_NATIVE_DATA_DIR:join(root,'native-data'),RIDGE_KERNEL_HOST_BINARY:kernelBinary,
  WEBVIEW2_USER_DATA_FOLDER:root,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*'},
  stdio:['ignore',reopenLog.fd,reopenLog.fd],windowsHide:true});await reopenLog.close();
 await until(async()=>{assert.equal(reopened.exitCode,null,'reopened desktop exited');try{cdp=Number((await readFile(join(root,'EBWebView','DevToolsActivePort'),'utf8')).split('\n')[0]);return cdp>0&&(await fetch(`http://127.0.0.1:${cdp}/json/version`,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}},'reopened desktop CDP missing');
 const restored=await desktopProjection(cdp);
 assert.ok(restored.projected.some(row=>row.id===workspace),'reopened desktop omitted original workspace');
 const layout=await restored.page.evaluate(async workspaceId=>{
  const core=await import('/src/lib/transport/nativeRuntimeCore.ts');return core.invoke('get_pane_layout_for',{workspaceId});
 },workspace);
 const leaves=node=>node.type==='leaf'?[node.id]:node.children.flatMap(leaves);
 assert.ok(leaves(layout).includes(pane),'reopened desktop did not bind original pane');
 const resumed=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
 assert.equal(resumed.child_pid,before.child_pid);assert.equal(resumed.child_start_instance,before.child_start_instance);
 assert.equal((await request('/v1/health',undefined,'GET')).pid,endpoint.pid);
 report.checks.push('reopened real desktop binds original workspace/pane and exact live child');
 report.runtimePid=endpoint.pid;report.desktopPid=desktop.ProcessId;report.childPid=after.child_pid;
 await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));console.log(`PASS owned desktop lifecycle: ${join(root,'result.json')}`);
}finally{
 lan?.close();try{await browser?.close();}catch{}
 const helpers=await launcherChildren();
 if(helpers.length){assert.ok(helpers.every(process=>process.ProcessId!==kernel.pid));await execute('powershell',['-NoProfile','-Command',`Stop-Process -Id ${helpers.map(process=>process.ProcessId).join(',')} -Force -ErrorAction SilentlyContinue`],{windowsHide:true}).catch(()=>{});}
 if(endpoint){try{await request('/v1/shutdown',{});}catch{}}
 if(kernel.exitCode===null&&kernel.signalCode===null){await Promise.race([once(kernel,'exit'),delay(6000)]);if(kernel.exitCode===null&&kernel.signalCode===null){kernel.kill();}}
 if(launcher&&launcher.exitCode===null&&launcher.signalCode===null){launcher.kill();}
}
