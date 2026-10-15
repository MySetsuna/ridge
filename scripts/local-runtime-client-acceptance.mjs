// Owned Kernel and real Chromium consumer of the production runtimeClient module.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';
import net from 'node:net';
import { chromium } from '@playwright/test';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-070-client-cdp-'));
const binary=path.resolve(process.env.RIDGE_TEST_KERNEL_BINARY||'target/debug/ridge-kernel.exe');
const nativeRoute=process.env.RIDGE_TEST_NATIVE_ROUTE==='1';
let devUrl=process.env.RIDGE_TEST_DEV_URL;
let vite;
const kernel=spawn(binary,[],{windowsHide:true,env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_AUTH_FILE:path.join(root,'auth.json')},stdio:'ignore'});
const evidence={scope:'Owned Kernel + real Chromium + production presentation client/shared RPC/WASM; OS grant issuance supplied by owned fixture',nativeRoute,checks:[],completeChange:false};
let endpoint,browser,grant;
const until=async fn=>{const end=Date.now()+15000;while(Date.now()<end){if(await fn())return;await new Promise(r=>setTimeout(r,25));}throw Error('owned fixture readiness timeout');};
const call=async(method,url,body,timeoutMs=6000)=>{const response=await fetch(`http://127.0.0.1:${endpoint.port}${url}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(timeoutMs)});assert.ok(response.ok,`Kernel HTTP ${response.status}`);return response.json();};
try {
 if(!devUrl){
  const port=await new Promise((resolve,reject)=>{const server=net.createServer();server.on('error',reject);server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port));});});
  devUrl=`http://127.0.0.1:${port}`;
  const log=fs.openSync(path.join(root,'vite.log'),'a');
  vite=spawn(process.execPath,['node_modules/vite/bin/vite.js'],{windowsHide:true,env:{...process.env,RIDGE_WEB_REMOTE:'',RIDGE_NATIVE_RUNTIME:nativeRoute?'1':'',RIDGE_DEV_SERVER_PORT:String(port)},stdio:['ignore',log,log]});
  fs.closeSync(log);
  await until(async()=>{assert.equal(vite.exitCode,null,'owned Vite exited');return fetch(devUrl+'/@vite/client',{signal:AbortSignal.timeout(1000)}).then(response=>response.ok,()=>false);});
 }
 assert.ok(['localhost','127.0.0.1'].includes(new URL(devUrl).hostname),'owned acceptance requires a local server');

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
 await page.exposeFunction('fixtureCatalogMutation',overrides=>call('POST','/v1/domain/commands',{method:'save_agent_profile_overrides',native:true,roots:[],connectionId:crypto.randomUUID(),args:{overrides}}));
 await page.exposeFunction('fixtureGatewayApproval',args=>call('POST','/v1/domain/commands',{method:'request_gateway_approval',native:true,roots:[],connectionId:crypto.randomUUID(),args},130000));
 const projectPath=path.join(root,'browser-project');fs.mkdirSync(projectPath);
 const result=await page.evaluate(async({workspaceId,paneId,nativeRoute,projectPath})=>{
  let nativeCore,bridge;const osCalls=[];
  if(nativeRoute){
   window.isTauri=true;
   window.__TAURI_INTERNALS__={invoke:async command=>{osCalls.push(command);if(command==='bootstrap_runtime_client')return window.fixtureGrant();throw Error('Unexpected native business IPC: '+command);},transformCallback:()=>{throw Error('Unexpected native Channel');}};
   [nativeCore,{bridge}]=await Promise.all([import('/src/lib/transport/nativeRuntimeCore.ts'),import('/src/lib/transport/tauriShim/bridge.ts')]);
  }
  const [{connectPresentationRuntime},{RpcClient},core]=await Promise.all([import('/src/lib/transport/runtimeClient.ts'),import('/packages/remote/src/shared/transport/rpcClient.ts'),import('/packages/ridge-term/pkg/ridge_term.js')]);
  let connection=await connectPresentationRuntime(nativeRoute?undefined:()=>window.fixtureGrant());const rpc=new RpcClient(connection.transport);await core.default();const mirror=new core.TerminalKernel(24,80,2000);let frames=0;
  const onFrame=bytes=>{mirror.applyRemoteFrame(bytes,workspaceId,paneId,9);frames++;};
  const stop=nativeRoute?()=>bridge.detach():connection.connection.onTerminalFrame(onFrame);
  const until=async fn=>{const end=performance.now()+10000;while(performance.now()<end){if(fn())return;await new Promise(r=>setTimeout(r,25));}throw Error('presentation state timeout');};
  try {
   if(nativeRoute)bridge.attach(connection.transport,{useGlobalWorkspace:false});
   const theme=nativeRoute?await nativeCore.invoke('get_theme_data'):await rpc.request('get_theme_data',{});if(theme.version!==1)throw Error('shared theme RPC failed');
   if(nativeRoute){
    if(await nativeCore.invoke('get_pty_runtime_identity',{workspaceId,paneId})!==null)throw Error('Ordinary shell acquired fabricated Agent identity');
    const foreground=await nativeCore.invoke('get_pane_foreground_process',{workspaceId,paneId});
    if(foreground!==null&&typeof foreground!=='string')throw Error('Runtime foreground projection failed');
    const catalogBefore=await nativeCore.invoke('read_agent_profile_overrides');
    if(!Array.isArray(catalogBefore))throw Error('Agent catalog did not return the saved raw projection');
    const customProfile={id:'owned-browser-agent',processNames:['owned-browser-cli','cmd','sh'],executable:'owned-browser-cli',resumeArgv:[],yoloArgs:[],protocol:'acp',capabilities:{prompt:true,history:true},futureMetadata:{protocol:'acp'}};
    await nativeCore.invoke('save_agent_profile_overrides',{overrides:[customProfile]});
    if(await nativeCore.invoke('get_pty_runtime_identity',{workspaceId,paneId})!==null)throw Error('Catalog process/protocol/capability declarations fabricated a registered Agent identity');
    const catalogRaw=await nativeCore.invoke('read_agent_profile_overrides');
    if(catalogRaw[0]?.futureMetadata?.protocol!=='acp')throw Error('Agent catalog metadata lost through the production facade');
    const catalogPreview=await nativeCore.invoke('list_agent_profiles');
    if(!catalogPreview.some(profile=>profile.id==='owned-browser-agent'&&profile.processNames[0]==='owned-browser-cli'))throw Error('Agent catalog persisted preview missing');

    const [{default:SettingsPanel},{mount,unmount}]=await Promise.all([import('/src/lib/components/SettingsPanel.svelte'),import('/scripts/commune-permission-ui-client.mjs')]);
    const {initThemeSystem}=await import('/src/lib/stores/themes.ts');await initThemeSystem();
    const legacyDraft='{"unapplied":"historical-draft"}';localStorage.setItem('ridge.agentProfiles.overrides',legacyDraft);
    const settingsTarget=document.createElement('section');document.body.append(settingsTarget);
    const settingsPanel=mount(SettingsPanel,{target:settingsTarget,props:{open:true,onClose:()=>{}}});
    [...settingsTarget.querySelectorAll('button')].find(button=>button.textContent.trim()==='智能体').click();
    await until(()=>settingsTarget.querySelector('[aria-label="Agent 清单 JSON"]')?.value.includes('owned-browser-agent'));
    const editor=settingsTarget.querySelector('[aria-label="Agent 清单 JSON"]');
    await window.fixtureCatalogMutation([{...customProfile,id:'remote-revision'}]);
    await until(()=>editor.value.includes('remote-revision'));
    const draft=JSON.stringify([{...customProfile,id:'unsaved-draft'}],null,2);
    editor.value=draft;editor.dispatchEvent(new Event('input',{bubbles:true}));
    await window.fixtureCatalogMutation([{...customProfile,id:'peer-newer'}]);
    await until(()=>settingsTarget.textContent.includes('当前草稿保留'));
    if(editor.value!==draft)throw Error('Peer catalog commit overwrote the unsaved production editor');
    if((await nativeCore.invoke('read_agent_profile_overrides'))[0].id!=='peer-newer')throw Error('Production editor silently saved its draft');
    if(localStorage.getItem('ridge.agentProfiles.overrides')!==legacyDraft)throw Error('Legacy draft was overwritten during hydration');
    await unmount(settingsPanel);settingsTarget.remove();
    await nativeCore.invoke('save_agent_profile_overrides',{overrides:catalogBefore});
    const {default:ApprovalModal}=await import('/src/lib/teammate/HitlApprovalModal.svelte');
    const approvalTarget=document.createElement('section');document.body.append(approvalTarget);
    const approvalModal=mount(ApprovalModal,{target:approvalTarget});
    await nativeCore.invoke('set_hitl_enabled',{enabled:true});
    const approval=window.fixtureGatewayApproval({workspaceId,paneId,initiator:'owned-browser-gateway',action:'rm -rf / # classification-only-never-executed'});
    await until(()=>approvalTarget.querySelector('[role="alertdialog"]'));
    [...approvalTarget.querySelectorAll('button')].find(button=>button.textContent.trim()==='修改').click();
    await until(()=>approvalTarget.querySelector('[aria-label="修改命令"]'));
    const replacement=approvalTarget.querySelector('[aria-label="修改命令"]');
    replacement.value='echo approved browser replacement';replacement.dispatchEvent(new Event('input',{bubbles:true}));
    [...approvalTarget.querySelectorAll('button')].find(button=>button.textContent.trim()==='改后执行').click();
    const decision=await approval;
    if(!decision.ok||decision.value.verdict!=='modify'||decision.value.replacement!=='echo approved browser replacement')throw Error('Production gateway modal did not resolve the original Runtime waiter');
    await until(()=>!approvalTarget.querySelector('[role="alertdialog"]'));
    if(await nativeCore.invoke('get_pending_hitl_count')!==0)throw Error('Gateway approval remained pending after the real click');
    await unmount(approvalModal);approvalTarget.remove();
    await nativeCore.invoke('set_hitl_enabled',{enabled:false});

    const memoryOriginal=await nativeCore.invoke('read_workspace_memory_document',{workspaceId});
    await nativeCore.invoke('set_workspace_memory',{workspaceId,goal:'owned browser goal',constraints:['preserve peer sections'],tasks:[{id:'owned-task',status:'open'}]});
    const memory=await nativeCore.invoke('get_workspace_memory',{workspaceId});
    if(memory.goal!=='owned browser goal'||memory.tasks[0].id!=='owned-task')throw Error('Production memory facade did not commit and reread the same Runtime document');
    let staleMemoryRefused=false;
    try{await nativeCore.invoke('commit_workspace_memory_document',{workspaceId,expected:memoryOriginal,value:{goal:'stale proposal'}});}catch{staleMemoryRefused=true;}
    if(!staleMemoryRefused)throw Error('Production memory facade overwrote a peer commit from a stale snapshot');
    const memoryCurrent=await nativeCore.invoke('read_workspace_memory_document',{workspaceId});
    await nativeCore.invoke('commit_workspace_memory_document',{workspaceId,expected:memoryCurrent,value:memoryOriginal});
    const project=await nativeCore.invoke('open_project',{path:projectPath});
    if(project.path!==projectPath||await nativeCore.invoke('get_current_project')!==projectPath)throw Error('Runtime project selection failed');
    if(!(await nativeCore.invoke('get_recent_projects')).some(record=>record.id===project.id))throw Error('Runtime project history missing');
    await nativeCore.invoke('remove_project',{projectId:project.id});
    if((await nativeCore.invoke('get_recent_projects')).some(record=>record.id===project.id))throw Error('Runtime project history removal failed');
    const saved=await nativeCore.invoke('save_user_theme',{entry:{id:'',label:'Browser Runtime theme',type:'dark',loader:{primary:'#fff',secondary:'#000'},colors:{bg:'#010203'}}});
    await nativeCore.invoke('set_active_theme',{themeId:saved.id});
    if((await nativeCore.invoke('get_active_theme_entry')).id!==saved.id)throw Error('Runtime active theme did not persist');
    if(!(await nativeCore.invoke('get_theme_data')).themes.some(entry=>entry.id===saved.id))throw Error('Runtime custom theme missing from catalog');
    const image=await nativeCore.invoke('save_theme_bg_image',{bytes:[1,2,3],ext:'png'});
    const assets=await nativeCore.invoke('get_theme_assets_dir');
    if(!image.endsWith('.png')||!assets.endsWith('theme-assets'))throw Error('Runtime theme asset contract failed');
    await nativeCore.invoke('delete_user_theme',{id:saved.id});
    if((await nativeCore.invoke('get_theme_data')).themes.some(entry=>entry.id===saved.id))throw Error('Runtime theme deletion did not commit');
    const events=await import('/src/lib/transport/nativeRuntimeEvent.ts');let updates=0;
    const stopEvents=await events.listen('workspace-list-changed',event=>{if(!event.payload?.initial)updates++;});
    try {await window.fixtureWorkspaceMutation();await until(()=>updates===1);}
    finally {stopEvents();}
    const paneState=await import('/src/lib/stores/paneTree.ts');
    let selected;let projected=[];
    const stopSelection=paneState.activeWorkspaceId.subscribe(value=>selected=value);
    const stopProjection=paneState.workspacesList.subscribe(value=>projected=value);
    try {
      await paneState.refreshWorkspaces();
      if(selected!==workspaceId)throw Error('Native production store did not use Runtime client selection');
      await paneState.createWorkspace();
      const createdView=selected;
      if(!createdView||createdView===workspaceId||!projected.some(row=>row.id===createdView))throw Error('Native production store failed to create/select its workspace');
      if(!await paneState.switchWorkspace(workspaceId)||selected!==workspaceId)throw Error('Native production store failed shared workspace switch');
      await nativeCore.invoke('close_workspace',{workspaceId:createdView});
    } finally {stopSelection();stopProjection();}
    const created=await nativeCore.invoke('create_workspace',{name:'Browser metadata'});
    if(typeof created!=='string')throw Error('Native workspace creation contract changed');
    await nativeCore.invoke('rename_workspace',{workspaceId:created,name:'Browser renamed'});
    let rows=await nativeCore.invoke('list_workspaces');const fromIndex=rows.findIndex(row=>row.id===created);
    if(fromIndex<0||rows[fromIndex].name!=='Browser renamed')throw Error('Runtime workspace name projection failed');
    const label=rows[fromIndex].displaySeq;
    let historyUpdates=0;
    const stopHistory=await events.listen('workspace-history-changed',event=>{if(event.payload!==null)throw Error('History event leaked content');historyUpdates++;});
    try {
      const historyId=await nativeCore.invoke('save_workspace',{workspaceId:created,name:'Browser history'});
      await until(()=>historyUpdates>=1);
      let history=await nativeCore.invoke('list_saved_workspaces');
      const savedHistory=history.find(row=>row.id===historyId);
      if(savedHistory?.workspace_id!==created||typeof savedHistory.pane_tree_json!=='string'||typeof savedHistory.saved_at!=='string')throw Error('Runtime historical DTO contract failed');
      await nativeCore.invoke('rename_saved_workspace',{historyId,name:'Browser renamed history'});
      history=await nativeCore.invoke('list_saved_workspaces');
      if(history.find(row=>row.id===historyId)?.name!=='Browser renamed history')throw Error('Runtime history rename failed');
      if(await nativeCore.invoke('restore_workspace',{historyId})!==created)throw Error('Runtime history restoration changed existing identity');
      if(await nativeCore.invoke('get_active_workspace_id')!==created)throw Error('History restore did not select caller workspace');
      await nativeCore.invoke('delete_saved_workspace',{historyId});
      if((await nativeCore.invoke('list_saved_workspaces')).some(row=>row.id===historyId))throw Error('Runtime history deletion failed');
    } finally {stopHistory();}
    const workspaceFile=projectPath+'/browser-workspace.ridge';
    if(await nativeCore.invoke('save_workspace_to_file',{workspaceId:created,name:'Browser saved',path:workspaceFile})!==workspaceFile)throw Error('Runtime workspace save failed');
    if(await nativeCore.invoke('get_last_opened_workspace_path')!==workspaceFile)throw Error('Runtime last-opened pointer did not commit');
    if(!(await nativeCore.invoke('list_recent_workspaces')).includes(workspaceFile))throw Error('Runtime workspace history missing');
    await nativeCore.invoke('delete_workspace_file',{workspaceId:created});
    if((await nativeCore.invoke('get_workspace_save_info',{workspaceId:created})).file_path!==null)throw Error('Runtime association remained after archive');
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
   if(nativeRoute){
    const before=frames;
    bridge.detach({preserveListeners:true});connection.disconnect();
    connection=await connectPresentationRuntime();
    bridge.attach(connection.transport,{useGlobalWorkspace:false});
    await until(()=>frames>before&&!!mirror.terminalInputContext());
    const resumed='RECONNECTED_'+crypto.randomUUID();
    connection.connection.sendStdin({workspaceId,paneId},new TextEncoder().encode('echo '+resumed+'\r'),mirror.terminalInputContext());
    await until(()=>mirror.dumpVisibleText().some(line=>line.trim()===resumed));
   }

   if(nativeRoute&&JSON.stringify(osCalls)!==JSON.stringify(['bootstrap_runtime_client','bootstrap_runtime_client']))throw Error('Business traffic leaked into OS IPC');
   return {frames,checks:['Production client factory connects over origin-bound local websocket','Existing LAN adapter and shared RPC client serve real business query','Canonical generated WASM receives and executes real PTY input',...(nativeRoute?['Production memory facade commits and rereads the Runtime document and rejects a stale full-document proposal','Production approval modal modifies one Runtime-owned ordinary-command permission without executing the dangerous classification text','A catalog matching the ordinary shell and declaring ACP/prompt/history cannot fabricate a registered Agent identity','Production SettingsPanel loads the service catalog over a stale legacy draft, refreshes a peer commit and preserves unsaved edits on another commit','Production native facade reads, saves and previews the same Agent catalog while preserving unknown metadata','Replacing the authenticated native transport retains the exact terminal Channel and activation; real PTY output and input resume without registering another channel','Native routing facade uses OS IPC solely for bootstrap; business RPC and terminal Channel use Runtime','Production facade opens, selects, lists and removes project history through Runtime','Production native paneTree store refreshes, creates/selects and switches using the shared per-client workspace RPCs','Production facade persists, selects, rereads and deletes custom themes and saves assets through Runtime','Production native event facade receives a real Kernel workspace commit','Production facade commits, observes, renames, restores and deletes historical snapshots with the existing workspace identity','Production facade creates, renames, reorders and closes a workspace with stable labels and scoped view recovery']:[])],storage:JSON.stringify({local:{...localStorage},session:{...sessionStorage}}),location:location.href};
  } finally {stop();rpc.dispose();connection.disconnect();mirror.free();}
 },{workspaceId,paneId,nativeRoute,projectPath});
 assert.ok(socketUrls.length>0);assert.ok(socketUrls.every(url=>!url.includes(grant.token)&&!url.includes(endpoint.token)));assert.ok(!result.storage.includes(grant.token)&&!result.storage.includes(endpoint.token));
 evidence.frames=result.frames;evidence.checks=result.checks;evidence.checks.push('No client or Kernel credential in browser URLs or storage');
 const ptys=await call('GET','/v1/domain/ptys');assert.ok(ptys.ptys.some(p=>p.id===paneId));evidence.checks.push('Presentation detach preserves the owned PTY');evidence.result='PASS';
} catch(error){evidence.result='FAIL';evidence.error=error.message;process.exitCode=1;}
finally {await browser?.close();if(vite&&vite.exitCode===null){if(process.platform==='win32')await new Promise(resolve=>spawn('taskkill',['/PID',String(vite.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'}).once('exit',resolve));else vite.kill('SIGTERM');}if(endpoint&&endpoint.pid===kernel.pid)await call('POST','/v1/shutdown').catch(()=>{});fs.writeFileSync(`.webview2-dev-cdp/070-${nativeRoute?'native-route':'local-client'}-cdp.json`,JSON.stringify(evidence,null,2)+'\n');}
console.log(JSON.stringify(evidence,null,2));
