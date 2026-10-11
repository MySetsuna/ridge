// Owned Kernel and real Chromium consumer of the production runtimeClient module.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-070-client-cdp-'));
const binary=path.resolve('.webview2-dev-cdp/cli-target/debug/ridge-kernel.exe');
const devUrl=JSON.parse(fs.readFileSync('.webview2-dev-cdp/tauri-dev-cdp.config.json','utf8')).build.devUrl;
const kernel=spawn(binary,[],{windowsHide:true,env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_AUTH_FILE:path.join(root,'auth.json')},stdio:'ignore'});
const evidence={scope:'Owned Kernel + real Chromium + production presentation client/shared RPC/WASM',checks:[],completeChange:false};
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
 const result=await page.evaluate(async({workspaceId,paneId})=>{
  const [{connectPresentationRuntime},{RpcClient},core]=await Promise.all([import('/src/lib/transport/runtimeClient.ts'),import('/packages/remote/src/shared/transport/rpcClient.ts'),import('/packages/ridge-term/pkg/ridge_term.js')]);
  const connection=await connectPresentationRuntime(()=>window.fixtureGrant());const rpc=new RpcClient(connection.transport);await core.default();const mirror=new core.TerminalKernel(24,80,2000);let frames=0;
  const stop=connection.connection.onTerminalFrame(bytes=>{mirror.applyRemoteFrame(bytes,workspaceId,paneId,9);frames++;});
  const until=async fn=>{const end=performance.now()+10000;while(performance.now()<end){if(fn())return;await new Promise(r=>setTimeout(r,25));}throw Error('presentation state timeout');};
  try {
   const theme=await rpc.request('get_theme_data',{});if(theme.version!==1)throw Error('shared theme RPC failed');
   connection.connection.subscribePane({workspaceId,paneId},{activationId:9,active:true});await until(()=>!!mirror.terminalInputContext());
   const marker='BROWSER_'+crypto.randomUUID();connection.connection.sendStdin({workspaceId,paneId},new TextEncoder().encode('echo '+marker+'\r'),mirror.terminalInputContext());
   await until(()=>mirror.dumpVisibleText().some(line=>line.trim()===marker));
   return {frames,checks:['Production client factory connects over origin-bound local websocket','Existing LAN adapter and shared RPC client serve real business query','Canonical generated WASM receives and executes real PTY input'],storage:JSON.stringify({local:{...localStorage},session:{...sessionStorage}}),location:location.href};
  } finally {stop();rpc.dispose();connection.disconnect();mirror.free();}
 },{workspaceId,paneId});
 assert.ok(socketUrls.length>0);assert.ok(socketUrls.every(url=>!url.includes(grant.token)&&!url.includes(endpoint.token)));assert.ok(!result.storage.includes(grant.token)&&!result.storage.includes(endpoint.token));
 evidence.frames=result.frames;evidence.checks=result.checks;evidence.checks.push('No client or Kernel credential in browser URLs or storage');
 const ptys=await call('GET','/v1/domain/ptys');assert.ok(ptys.ptys.some(p=>p.id===paneId));evidence.checks.push('Presentation detach preserves the owned PTY');evidence.result='PASS';
} catch(error){evidence.result='FAIL';evidence.error=error.message;process.exitCode=1;}
finally {await browser?.close();if(endpoint&&endpoint.pid===kernel.pid)await call('POST','/v1/shutdown').catch(()=>{});fs.writeFileSync('.webview2-dev-cdp/070-local-client-cdp.json',JSON.stringify(evidence,null,2)+'\n');}
console.log(JSON.stringify(evidence,null,2));
