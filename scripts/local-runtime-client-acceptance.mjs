// Owned Kernel and real Chromium consumer of the production runtimeClient module.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-070-client-cdp-'));
const binary=path.resolve(process.env.RIDGE_TEST_KERNEL_BINARY||'.webview2-dev-cdp/cli-target/debug/ridge-kernel.exe');
const nativeRoute=process.env.RIDGE_TEST_NATIVE_ROUTE==='1';
const devUrl=process.env.RIDGE_TEST_DEV_URL||JSON.parse(fs.readFileSync('.webview2-dev-cdp/tauri-dev-cdp.config.json','utf8')).build.devUrl;
const kernel=spawn(binary,[],{windowsHide:true,env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_AUTH_FILE:path.join(root,'auth.json')},stdio:'ignore'});
const evidence={scope:'Owned Kernel + real Chromium + production presentation client/shared RPC/WASM; OS grant issuance supplied by owned fixture',nativeRoute,checks:[],completeChange:false};
let endpoint,browser,grant;
const until=async fn=>{const end=Date.now()+15000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,25));}throw Error('owned fixture readiness timeout');};
const call=async(method,url,body)=>{const response=await fetch(`http://127.0.0.1:${endpoint.port}${url}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(6000)});assert.ok(response.ok,`Kernel HTTP ${response.status}`);return response.json();};
try {
 await until(()=>{assert.equal(kernel.exitCode,null,'owned Kernel exited');if(!fs.existsSync(path.join(root,'kernel.json')))return false;endpoint=JSON.parse(fs.readFileSync(path.join(root,'kernel.json')));return true;});assert.equal(endpoint.pid,kernel.pid);
 const workspace=await call('POST','/v1/domain/workspaces',{name:'070-CDP-owned'});const workspaceId=workspace.workspace_id;const paneId=crypto.randomUUID();
 await call('POST','/v1/domain/ptys',{pty_id:paneId,workspace_id:workspaceId,program:process.platform==='win32'?'cmd.exe':'/bin/sh',args:process.platform==='win32'?['/d','/q','/k']:[],env:{},role:'shell',cols:80,rows:24});
 browser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});const page=await browser.newPage();
 await page.route('**/__local_runtime_client',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><title>Owned Runtime client acceptance</title><body>Runtime presentation client</body>'}));
 await page.goto(devUrl+'/__local_runtime_client');
 const origin=new URL(devUrl).origin;grant=await call('POST','/v1/local/clients',{deviceId:'owned-browser-window',origin});assert.notEqual(grant.token,endpoint.token);
 const socketUrls=[];page.on('websocket',socket=>socketUrls.push(socket.url()));
 await page.exposeFunction('fixtureGrant',()=>grant);
 await page.exposeFunction('fixtureWorkspaceMutation',()=>call('POST','/v1/domain/workspaces'));
 const result=await page.evaluate(async({workspaceId,paneId,nativeRoute})=>{
  let nativeCore,bridge;const osCalls=[];
  if(nativeRoute){
   window.__TAURI_INTERNALS__={invoke:async command=>{osCalls.push(command);if(command==='bootstrap_runtime_client')return window.fixtureGrant();throw Error('Unexpected native business IPC: '+command);},transformCallback:()=>{throw Error('Unexpected native Channel');}};
   [nativeCore,{bridge}]=await Promise.all([import('/src/lib/transport/nativeRuntimeCore.ts'),import('/src/lib/transport/tauriShim/bridge.ts')]);
  }
  const [{connectPresentationRuntime},{RpcClient},core]=await Promise.all([import('/src/lib/transport/runtimeClient.ts'),import('/packages/remote/src/shared/transport/rpcClient.ts'),import('/packages/ridge-term/pkg/ridge_term.js')]);
  const connection=await connectPresentationRuntime(nativeRoute?undefined:()=>window.fixtureGrant());const rpc=new RpcClient(connection.transport);await core.default();const mirror=new core.TerminalKernel(24,80,2000);let frames=0;
  const onFrame=bytes=>{mirror.applyRemoteFrame(bytes,workspaceId,paneId,9);frames++;};
  const stop=nativeRoute?()=>bridge.detach():connection.connection.onTerminalFrame(onFrame);
  const until=async fn=>{const end=performance.now()+10000;while(performance.now()<end){if(fn())return;await new Promise(r=>setTimeout(r,25));}throw Error('presentation state timeout');};
  try {
   if(nativeRoute)bridge.attach(connection.transport,{useGlobalWorkspace:false});
   const theme=nativeRoute?await nativeCore.invoke('get_theme_data'):await rpc.request('get_theme_data',{});if(theme.version!==1)throw Error('shared theme RPC failed');
   if(nativeRoute){
    const events=await import('/src/lib/transport/nativeRuntimeEvent.ts');let updates=0;
    const stopEvents=await events.listen('workspace-list-changed',event=>{if(!event.payload?.initial)updates++;});
    try {await window.fixtureWorkspaceMutation();await until(()=>updates===1);}
    finally {stopEvents();}
    const created=await nativeCore.invoke('create_workspace',{name:'Browser metadata'});
    if(typeof created!=='string')throw Error('Native workspace creation contract changed');
    await nativeCore.invoke('rename_workspace',{workspaceId:created,name:'Browser renamed'});
    let rows=await nativeCore.invoke('list_workspaces');const fromIndex=rows.findIndex(row=>row.id===created);
    if(fromIndex<0||rows[fromIndex].name!=='Browser renamed')throw Error('Runtime workspace name projection failed');
    const label=rows[fromIndex].displaySeq;
    await nativeCore.invoke('reorder_workspaces',{fromIndex,toIndex:0});rows=await nativeCore.invoke('list_workspaces');
    if(rows[0].id!==created||rows[0].displaySeq!==label)throw Error('Runtime workspace order/label changed');
    await nativeCore.invoke('close_workspace',{workspaceId:created});
    if(await nativeCore.invoke('get_active_workspace_id')!==workspaceId)throw Error('Retired client workspace view did not recover');
   }
   if(nativeRoute){const channel=new nativeCore.Channel();channel.onmessage=onFrame;await nativeCore.invoke('register_pane_delta_channel',{workspaceId,paneId,activationId:9,channel});}
   else connection.connection.subscribePane({workspaceId,paneId},{activationId:9,active:true});
   await until(()=>!!mirror.terminalInputContext());
   const marker='BROWSER_'+crypto.randomUUID();connection.connection.sendStdin({workspaceId,paneId},new TextEncoder().encode('echo '+marker+'\r'),mirror.terminalInputContext());
   await until(()=>mirror.dumpVisibleText().some(line=>line.trim()===marker));
   if(nativeRoute&&JSON.stringify(osCalls)!==JSON.stringify(['bootstrap_runtime_client']))throw Error('Business traffic leaked into OS IPC');
   return {frames,checks:['Production client factory connects over origin-bound local websocket','Existing LAN adapter and shared RPC client serve real business query','Canonical generated WASM receives and executes real PTY input',...(nativeRoute?['Native routing facade uses OS IPC solely for bootstrap; business RPC and terminal Channel use Runtime','Production native event facade receives a real Kernel workspace commit','Production facade creates, renames, reorders and closes a workspace with stable labels and scoped view recovery']:[])],storage:JSON.stringify({local:{...localStorage},session:{...sessionStorage}}),location:location.href};
  } finally {stop();rpc.dispose();connection.disconnect();mirror.free();}
 },{workspaceId,paneId,nativeRoute});
 assert.ok(socketUrls.length>0);assert.ok(socketUrls.every(url=>!url.includes(grant.token)&&!url.includes(endpoint.token)));assert.ok(!result.storage.includes(grant.token)&&!result.storage.includes(endpoint.token));
 evidence.frames=result.frames;evidence.checks=result.checks;evidence.checks.push('No client or Kernel credential in browser URLs or storage');
 const ptys=await call('GET','/v1/domain/ptys');assert.ok(ptys.ptys.some(p=>p.id===paneId));evidence.checks.push('Presentation detach preserves the owned PTY');evidence.result='PASS';
} catch(error){evidence.result='FAIL';evidence.error=error.message;process.exitCode=1;}
finally {await browser?.close();if(endpoint&&endpoint.pid===kernel.pid)await call('POST','/v1/shutdown').catch(()=>{});fs.writeFileSync(`.webview2-dev-cdp/070-${nativeRoute?'native-route':'local-client'}-cdp.json`,JSON.stringify(evidence,null,2)+'\n');}
console.log(JSON.stringify(evidence,null,2));
