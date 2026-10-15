import assert from 'node:assert/strict';
import {chromium} from '@playwright/test';
import {cpus,release} from 'node:os';
import {createReadStream} from 'node:fs';
import {createHash,randomUUID} from 'node:crypto';
import {readFile,writeFile,unlink} from 'node:fs/promises';
import {join} from 'node:path';
import {createRuntimeReplyObserver} from './runtimeReplyObserver.mjs';

export async function performanceEnvironment(binaries){
 const hashes={};
 for(const [name,path] of Object.entries(binaries)){
  const hash=createHash('sha256');for await(const bytes of createReadStream(path))hash.update(bytes);hashes[name]=hash.digest('hex');
 }
 return {platform:process.platform,osRelease:release(),node:process.version,cpu:cpus()[0]?.model,logicalCpus:cpus().length,binarySha256:hashes};
}

export function distribution(samples){
 const sorted=samples.filter(Number.isFinite).sort((a,b)=>a-b);
 const rank=p=>sorted.length?sorted[Math.max(0,Math.ceil(sorted.length*p)-1)]:null;
 const mean=sorted.length?sorted.reduce((sum,value)=>sum+value,0)/sorted.length:null;
 return {count:sorted.length,p50:rank(.5),p95:rank(.95),p99:rank(.99),max:sorted.at(-1)??null,std:sorted.length?Math.sqrt(sorted.reduce((sum,value)=>sum+(value-mean)**2,0)/sorted.length):null};
}

export function parseCliStages(text,enabled,pid){
 const limit=65536,stages={},errors=[];let samples=0,truncated=false;
 const names=['decode','semantic_apply','render','input_queue','socket_send','output_queue','stdout','control_rpc'];
 for(const name of names)stages[name]={samples:[],queueDepthMax:0,bytesTotal:0};
 // The live file may end midway through a formatter write. Only complete records count.
 for(const line of text.slice(0,text.lastIndexOf('\n')+1).split('\n')){
  const marker=line.indexOf('RIDGE_CLI_STAGE ');if(marker<0)continue;
  assert.equal(enabled,true,'CLI emitted stage diagnostics while disabled');
  const value=JSON.parse(line.slice(marker+'RIDGE_CLI_STAGE '.length));
  assert.equal(value.pid,pid,'CLI diagnostic process differs from attested child');
  if(value.event==='truncated'){assert.equal(value.limit,limit);truncated=true;continue;}
  assert.ok(++samples<=limit,'CLI diagnostic sample bound exceeded');
  if(value.event==='error'){assert.match(value.code,/^[A-Z0-9_]{1,128}$/);errors.push(value.code);continue;}
  assert.equal(value.event,'sample');assert.ok(names.includes(value.stage),'unknown CLI diagnostic stage');
  assert.ok(Number.isFinite(value.durationMs)&&value.durationMs>=0);
  assert.ok(Number.isSafeInteger(value.bytes)&&value.bytes>=0);
  const capacity=value.stage==='output_queue'?32:['input_queue','socket_send','control_rpc'].includes(value.stage)?64:0;
  assert.ok(Number.isSafeInteger(value.queueDepth)&&value.queueDepth>=0&&value.queueDepth<=capacity,'CLI queue depth exceeds its capacity');
  const stage=stages[value.stage];stage.samples.push(value.durationMs);stage.queueDepthMax=Math.max(stage.queueDepthMax,value.queueDepth);stage.bytesTotal+=value.bytes;
 }
 return {enabled,pid,samples,limit,truncated,errors,scope:'in-process CPU/queue admission and stdout flush; excludes terminal host screen presentation; diagnostic logging overhead excluded from individual stages',
  stages:Object.fromEntries(Object.entries(stages).map(([name,value])=>[name,{latencyMs:distribution(value.samples),queueDepthMax:value.queueDepthMax,bytesTotal:value.bytesTotal}]))};
}

export const RUNTIME_PERFORMANCE_SCENARIOS=['idle','sustained-output','desktop-minimized','switch','history','reconnect','host-disconnected'];

export function assertRuntimePerformance(receipt){
 const cli=receipt.clientStages.cli;
 assert.ok(cli,'CLI stage receipt missing; rerun the current fixture');
 assert.equal(cli.enabled,receipt.diagnostics===true,'CLI diagnostics mode differs from receipt');
 assert.equal(cli.truncated,false,'CLI stage capture exceeded its bound');assert.equal(cli.errors.length,0,'CLI semantic/transport diagnostics reported errors');
 if(cli.enabled)for(const stage of ['decode','semantic_apply','render','input_queue','socket_send','output_queue','stdout'])assert.ok(cli.stages[stage].latencyMs.count>0,`CLI ${stage} stage not measured`);
 if(receipt.observerStats){
  assert.equal(receipt.observerStats.scrollbackCapacity,256,'reference observer capacity changed');
  assert.ok(receipt.observerStats.maxCachedRows<=256,'reference observer cache exceeded its bound');
 }
 for(const scenario of RUNTIME_PERFORMANCE_SCENARIOS)for(const client of ['desktop','cli','remote']){
  const key=`${scenario}/${client}`,group=receipt.groups[key];
  assert.ok(group,`missing performance group ${key}`);
  assert.equal(group.latencyMs.count,receipt.sampleCountPerClientPerScenario,`${key} incomplete samples`);
  assert.equal(group.timeouts,0,`${key} timeout`);
  assert.equal(group.failures.length,0,`${key} failure`);
  if(scenario==='sustained-output'){
   assert.equal(group.loadActive.length,receipt.sampleCountPerClientPerScenario,`${key} missing load observations`);
   assert.ok(group.loadActive.every(Boolean),`${key} lost sustained load`);
  }
 }
 for(const client of ['desktop','remote'])assert.equal(receipt.clientStages[client].errors.length,0,`${client} canonical frame errors`);
 assert.deepEqual(receipt.windowStates.map(state=>state.minimized),[true,false],'minimized window was not verified and restored');
 for(const client of ['desktop','remote']){
  const history=receipt.history[client];
  assert.equal(history.samples.length,receipt.sampleCountPerClientPerScenario,`${client} incomplete history samples`);
  assert.ok(history.samples.every(sample=>sample.pages>0&&sample.rows>0&&sample.cachedRows<=256&&sample.contentVerified),`${client} invalid or unbounded history`);
 }
 for(const kind of ['switch/desktop','switch/remote','history/cli','reconnect/remote','reconnect/cli','host-disconnected/runtime'])assert.equal(receipt.transitions[kind].latencyMs.count,receipt.sampleCountPerClientPerScenario,`${kind} transition samples missing`);
}

/** Same command bytes, same child and common raw-output observer for all ingress paths. */
export async function measureRuntimeClients({page,root,workspace,pane,cliPane,port,code,request,command,devUrl,binaries,diagnostics=false,processes,setDesktopMinimized}){
 const remoteBrowser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
 const remote=await remoteBrowser.newPage({ignoreHTTPSErrors:true});
 await remote.route('**/__runtime_performance',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><title>Owned Remote performance</title>'}));
 await remote.goto(`${devUrl}/__runtime_performance`);
 const device=await remote.evaluate(async()=>(await import('/packages/remote/src/shared/transport/deviceId.ts')).getRemoteDeviceId());
 const verified=await (await remote.request.post(`https://127.0.0.1:${port}/verify`,{form:{code,device}})).json();
 assert.ok(verified.success&&typeof verified.token==='string','Remote benchmark session issuance failed');
 const setup=async(surface,native)=>surface.evaluate(async({native,workspace,pane,port,token,diagnostics})=>{
  globalThis.__RIDGE_REMOTE_PERF_TRACE=diagnostics;
  const perf=await import('/packages/remote/src/shared/transport/remotePerfTrace.ts');
  const [{RemoteConnection},{createLanWsTransport},{RpcClient},core,runtime]=await Promise.all([
   import('/packages/remote/src/shared/transport/wsRemote.ts'),import('/packages/remote/src/shared/transport/lanWsAdapter.ts'),
   import('/packages/remote/src/shared/transport/rpcClient.ts'),import('/packages/ridge-term/pkg/ridge_term.js'),
   import('/packages/remote/src/shared/terminal/wasmRuntime.ts'),
  ]);
  let owner,connection;
  if(native){owner=await (await import('/src/lib/transport/runtimeClient.ts')).connectPresentationRuntime();connection=owner.connection;}
  else{
   connection=new RemoteConnection();
   await new Promise((done,fail)=>{const timer=setTimeout(()=>fail(Error('Remote benchmark connect timeout')),15000);const stop=connection.onStateChange(state=>{if(state==='connected'){clearTimeout(timer);stop();done();}if(state==='error'){clearTimeout(timer);stop();fail(Error('Remote benchmark connection failed'));}});connection.connect('127.0.0.1',port,token,'token',true);});
   owner={transport:createLanWsTransport(connection),disconnect:()=>connection.disconnect()};
  }
  const rpc=new RpcClient(owner.transport);await core.default({module_or_path:'/packages/ridge-term/pkg/ridge_term_bg.wasm'});
  let mirror=new core.TerminalKernel(24,80,256),activation=native?71:72;
  const lifetimes=new Map();
  const render=await runtime.loadRendererRuntime();const fonts=await runtime.loadSelectedFontRuntime();
  await (await import('/packages/remote/src/shared/terminal/fontDataService.ts')).loadHostTerminalFonts('Consolas',fonts.installFontData,(method,args)=>rpc.request(method,args));
  render.installFontRasterizer((width,height)=>new fonts.FontRasterizer(width,height));
  const canvas=document.createElement('canvas');Object.assign(canvas.style,{position:'fixed',left:'0',top:'0',zIndex:'2147483647'});document.body.appendChild(canvas);
  const host=await render.SurfaceHostHandle.init(canvas);const renderer=await render.RenderHandle.newWithWebgpuFirst(canvas,host.clone());
  const [cw,ch]=renderer.configure('Consolas',15,1);canvas.style.width=`${cw*80}px`;canvas.style.height=`${ch*24}px`;host.resize(cw*80,ch*24,1);renderer.resize(cw*80,ch*24,1);
  let revision=0,frames=0;const feed=[],paint=[],seen=new Map(),errors=[];
  let connectionGeneration=0,readyGeneration=0;
  const resetMirror=()=>{mirror.free();mirror=new core.TerminalKernel(24,80,256);revision=0;};
  const offState=connection.onStateChange(state=>{if(state!=='connected'){connectionGeneration++;resetMirror();}});
  const offReconnect=connection.onReconnect(()=>connection.activatePane({workspaceId:workspace,paneId:pane},{activationId:activation,active:true}));
  const stop=connection.onTerminalFrame(bytes=>{
   try{
   const began=performance.now();const applied=mirror.applyRemoteFrame(bytes,workspace,pane,activation);feed.push(performance.now()-began);if(!applied)return;
   const context=mirror.terminalInputContext(),key=`${workspace}/${pane}`;
   if(context){if(lifetimes.has(key)&&lifetimes.get(key)!==context.terminalEpoch)throw Error('benchmark terminal lifetime changed');lifetimes.set(key,context.terminalEpoch);}
   readyGeneration=connectionGeneration;
   const painted=performance.now();const packet=mirror.presentationFrame(revision);if(packet)revision=renderer.applyPresentation(packet);renderer.render();paint.push(performance.now()-painted);frames++;
   for(const line of mirror.dumpVisibleText()){
    const match=line.trim().match(/^REPLY_(\d{8})$/);if(match&&!seen.has(match[1]))seen.set(match[1],performance.now());
   }
   }catch(error){errors.push(String(error));}
  });
  connection.subscribePane({workspaceId:workspace,paneId:pane},{activationId:activation,active:true});
  const end=Date.now()+15000;while(!mirror.terminalInputContext()&&Date.now()<end)await new Promise(done=>setTimeout(done,25));
  if(!mirror.terminalInputContext())throw Error('benchmark canonical snapshot missing');
  window.__runtimeBench={
   ready(){return connection.state()==='connected'&&Boolean(mirror.terminalInputContext());},
   connectionStatus(){return {generation:connectionGeneration,readyGeneration,ready:this.ready(),state:connection.state(),failure:connection.lastFailure(),errors:errors.slice(-8)};},
   async retarget(nextWorkspace,nextPane){
    const began=performance.now();
    workspace=nextWorkspace;pane=nextPane;activation++;resetMirror();
    connection.activatePane({workspaceId:workspace,paneId:pane},{activationId:activation,active:true});
    const end=Date.now()+15000;while(!mirror.terminalInputContext()&&Date.now()<end)await new Promise(done=>setTimeout(done,25));
    if(!mirror.terminalInputContext())throw Error('switch canonical snapshot missing');
    return performance.now()-began;
   },
   async historyPreview(rows){
    const preview=new core.TerminalKernel(24,80,256);
    try{const snapshot=await rpc.request('get_terminal_snapshot',{workspaceId:workspace,paneId:pane,activationId:activation});preview.applyRemoteFrame(Uint8Array.from(atob(snapshot),ch=>ch.charCodeAt(0)),workspace,pane,activation);preview.scrollUp(rows);return preview.dumpVisibleText().map(line=>line.trim()).find(line=>/^REPLY_\d{8}$/.test(line));}finally{preview.free();}
   },
   send(text){if(!connection.sendStdin({workspaceId:workspace,paneId:pane},text,mirror.terminalInputContext()))throw Error('input rejected by production scheduler');},
   snapshot(){return {frames,feed,paint,errors,backend:renderer.backendName(),seen:Object.fromEntries(seen),queueTrace:perf.remotePerfSnapshot()};},
   evidence(id){return {frames,errors:errors.slice(-8),replySeen:seen.has(id),visible:mirror.dumpVisibleText(),queueTrace:perf.remotePerfSnapshot()};},
   resetStages(){feed.length=0;paint.length=0;perf.resetRemotePerfTrace();},
   async history(){
    const historyMirror=new core.TerminalKernel(24,80,256);
    try{
     const snapshot=await rpc.request('get_terminal_snapshot',{workspaceId:workspace,paneId:pane,activationId:activation});
     if(typeof snapshot!=='string')throw Error('history benchmark snapshot missing');
     historyMirror.applyRemoteFrame(Uint8Array.from(atob(snapshot),character=>character.charCodeAt(0)),workspace,pane,activation);
     const initial=historyMirror.scrollbackLen(),began=performance.now();let pages=0,rows=0,bytes=0,applyMs=0;
     for(let index=0;index<4;index++){
      const request=historyMirror.terminalHistoryRequest(64);if(!request)break;
      const before=historyMirror.scrollbackLen(),frame=await connection.fetchTerminalHistory(request);
      if(frame.length>4*1024*1024)throw Error('history benchmark frame exceeds bound');
      const applied=performance.now();historyMirror.applyRemoteFrame(frame,workspace,pane,activation);applyMs+=performance.now()-applied;
      const added=historyMirror.scrollbackLen()-before;
      if(added<=0||added>64||historyMirror.remoteHistoryBefore()>=request.before)throw Error('history benchmark did not advance contiguously');
      pages++;rows+=added;bytes+=frame.length;
     }
     const cachedRows=historyMirror.scrollbackLen();
     if(cachedRows>256||cachedRows!==initial+rows)throw Error('history benchmark cache accounting failed');
     historyMirror.scrollUp(cachedRows);
     const contentVerified=historyMirror.dumpVisibleText().some(line=>/^(?:LOAD_\d+|REPLY_\d{8})$/.test(line.trim()));
     return {durationMs:performance.now()-began,applyMs,pages,rows,bytes,cachedRows,contentVerified};
    }finally{historyMirror.free();}
   },
   close(){offReconnect();offState();stop();renderer.free();host.free();mirror.free();owner.disconnect();canvas.remove();},
  };
 },{native,workspace,pane,port,token:verified.token,diagnostics});
 let lease,stopObserver=false,observeTask,replyObserver;const waiting=new Map();const progress={completeChange:false};
 const observerStats={frames:0,feedTotalMs:0,maxCachedRows:0,semanticOnlyReplies:[]};
 const loadFlag=join(root,'output-load.flag'),loadPidFile=join(root,'output-load.pid');let loadStarted=false,loadProducer,outbound;
 try{
  await setup(page,true);await setup(remote,false);
  const observedPty=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
  assert.ok(observedPty?.rows>0&&observedPty?.cols>0,'reference observer needs actual PTY dimensions');
  replyObserver=await createRuntimeReplyObserver(observedPty.rows,observedPty.cols);
  observerStats.rows=observedPty.rows;observerStats.cols=observedPty.cols;observerStats.scrollbackCapacity=256;
  const alternateWorkspace=(await request('/v1/domain/workspaces',{name:'Owned performance switch'})).workspace_id;
  const alternatePane=randomUUID();
  await request(`/v1/domain/workspaces/${alternateWorkspace}/topology`,{pane_tree:{root:{Leaf:alternatePane},panes:{[alternatePane]:{id:alternatePane,mode:'Terminal',cwd:root}}}},'PUT');
  await request('/v1/domain/ptys',{pty_id:alternatePane,workspace_id:alternateWorkspace,program:'cmd.exe',args:['/d','/q','/k'],cwd:root,env:{},role:'shell',cols:80,rows:24});
  const transitions={};for(const key of ['switch/desktop','switch/remote','history/cli','reconnect/remote','reconnect/cli','host-disconnected/runtime'])transitions[key]=[];
  progress.transitions=transitions;
  lease=(await request(`/v1/domain/ptys/${pane}/output`,undefined)).lease_id;assert.ok(lease);
  let retained='',loadIndex=-1,lastLoadAt=0;
  observeTask=(async()=>{while(!stopObserver){
   const result=await request(`/v1/domain/ptys/${pane}/output/${lease}?timeout_ms=100&max_frames=64`,undefined,'GET');
   assert.equal(result.ok,true,'output observer request was rejected');
   if(result.kind==='lagged')throw Error('benchmark observer lagged');
   for(const frame of result.frames??[]){
    const bytes=Buffer.from(frame.data_b64,'base64'),began=performance.now();
    replyObserver.feed(bytes);observerStats.feedTotalMs+=performance.now()-began;observerStats.frames++;
    retained+=bytes.toString();
   }
   observerStats.maxCachedRows=Math.max(observerStats.maxCachedRows,replyObserver.cachedRows());
   retained=retained.slice(-65536);
   const indices=[...retained.matchAll(/LOAD_(\d+)/g)].map(match=>Number(match[1]));
   const latest=indices.length?Math.max(...indices):-1;
   if(latest>loadIndex){loadIndex=latest;lastLoadAt=performance.now();}
   for(const [id,entry] of waiting){
    const rawSeen=retained.includes(`REPLY_${id}`),semanticSeen=replyObserver.hasReply(id);
    if(rawSeen||semanticSeen){
     if(!rawSeen)observerStats.semanticOnlyReplies.push(id);
     waiting.delete(id);entry.done(performance.now()-entry.start);
    }
   }
  }})();
  const reply=(id,start)=>new Promise((done,fail)=>{const timer=setTimeout(()=>{waiting.delete(id);fail(Error(`child reply timeout: ${id}`));},5000);waiting.set(id,{start,done:value=>{clearTimeout(timer);done(value);}});});
  const groups={},windowStates=[];let sequence=0,minimized=false;
  progress.groups=groups;
  for(const scenario of ['warmup',...RUNTIME_PERFORMANCE_SCENARIOS]){
   progress.scenario=scenario;console.log(`Runtime performance phase: ${scenario}`);
   if(scenario==='switch'&&minimized){windowStates.push(await setDesktopMinimized(false));minimized=false;}
   if(scenario==='desktop-minimized'){
    await unlink(loadFlag);loadStarted=false;
    const stopped=()=>/(?:^|\r?\n)LOAD_STOPPED\r?\n/.test(retained);
    const deadline=Date.now()+5000;while(!stopped()&&Date.now()<deadline)await new Promise(done=>setTimeout(done,25));
    assert.ok(stopped(),'output producer did not stop before minimized idle samples');
    windowStates.push(await setDesktopMinimized(true));minimized=true;
   }
   if(scenario==='sustained-output'){
    await writeFile(loadFlag,'owned bounded output load');loadStarted=true;
    const quote=path=>path.replaceAll("'","''");
    const generator=`start "" /b powershell.exe -NoLogo -NoProfile -NonInteractive -Command "Set-Content -LiteralPath '${quote(loadPidFile)}' -Value $PID; $until=(Get-Date).AddSeconds(90); $i=0; while((Test-Path -LiteralPath '${quote(loadFlag)}') -and ((Get-Date) -lt $until)){[Console]::WriteLine(('LOAD_'+$i)); $i++; Start-Sleep -Milliseconds 10}; [Console]::WriteLine('LOAD_STOPPED')"\r\n`;
    await page.evaluate(text=>window.__runtimeBench.send(text),generator);
    const deadline=Date.now()+10000;while(loadIndex<5&&Date.now()<deadline)await new Promise(done=>setTimeout(done,25));
    assert.ok(loadIndex>=5,'sustained output producer never started');
    const loadPid=Number((await readFile(loadPidFile,'utf8')).trim());assert.ok(Number.isSafeInteger(loadPid)&&loadPid>0);
    const original=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
    const child=(await processes()).find(process=>process.ProcessId===loadPid);
    assert.equal(child?.ParentProcessId,original.child_pid,'load writer was not created by the owned original child');
    loadProducer={pid:loadPid,parentPid:child.ParentProcessId,targetLineIntervalMs:10,watchdogSeconds:90};
   }
   for(const client of ['desktop','cli','remote'])groups[`${scenario}/${client}`]={samples:[],timeouts:0,failures:[],failureEvidence:[],loadActive:[]};
   for(let round=0;round<(scenario==='warmup'?8:32);round++){
    if(scenario==='switch')for(const [client,surface] of [['desktop',page],['remote',remote]]){
     const away=await surface.evaluate(({workspace,pane})=>window.__runtimeBench.retarget(workspace,pane),{workspace:alternateWorkspace,pane:alternatePane});
     const back=await surface.evaluate(({workspace,pane})=>window.__runtimeBench.retarget(workspace,pane),{workspace,pane});transitions[`switch/${client}`].push(away+back);
    }
    if(scenario==='reconnect'){
     const before=await remote.evaluate(()=>window.__runtimeBench.connectionStatus());
     const cli=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===cliPane);
     const reconnectLease=(await request(`/v1/domain/ptys/${cliPane}/output?after_seq=${cli.next_seq-1}`,undefined)).lease_id;
     const observer=await createRuntimeReplyObserver(cli.rows,cli.cols),began=performance.now();
     try{
      await command('set_remote_enabled',{enabled:false});await command('set_remote_enabled',{enabled:true,port});
      await remote.waitForFunction(generation=>{const state=window.__runtimeBench.connectionStatus();return state.generation>generation&&state.readyGeneration===state.generation&&state.ready;},before.generation,{timeout:15000});
      transitions['reconnect/remote'].push(performance.now()-began);
      let status='';const end=Date.now()+15000;
      while(Date.now()<end){
       const output=await request(`/v1/domain/ptys/${cliPane}/output/${reconnectLease}?timeout_ms=50&max_frames=64`,undefined,'GET');assert.equal(output.kind==='lagged',false);
       for(const frame of output.frames??[]){const bytes=Buffer.from(frame.data_b64,'base64');observer.feed(bytes);status=(status+bytes.toString()).slice(-65536);}
       if(status.includes('Reconnecting...')&&observer.visibleText().some(line=>line.includes(`${root}>`)))break;
      }
      assert.ok(status.includes('Reconnecting...')&&observer.visibleText().some(line=>line.includes(`${root}>`)),'same CLI did not display the original terminal after reconnect');
      transitions['reconnect/cli'].push(performance.now()-began);
     }finally{observer.close();await request(`/v1/domain/ptys/${cliPane}/output/${reconnectLease}`,undefined,'DELETE');}
    }
    if(scenario==='host-disconnected'){
     const info=await command('get_remote_info');outbound=await command('connect_host',{kind:'remote',addr:`https://127.0.0.1:${port}`,token:info.totpCode});
     const began=performance.now();await command('disconnect_host',{hostId:outbound});
     assert.equal((await command('host_list_snapshot')).find(host=>host.id===outbound)?.status,'disconnected');transitions['host-disconnected/runtime'].push(performance.now()-began);
    }
    for(const client of ['desktop','cli','remote']){
    if(scenario==='history'){
     if(client!=='cli')await (client==='desktop'?page:remote).evaluate(()=>window.__runtimeBench.history());
     else{
      const expected=await remote.evaluate(()=>window.__runtimeBench.historyPreview(24));assert.ok(expected,'CLI history preview lacks a prior child reply');
      const state=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===cliPane);
      const historyLease=(await request(`/v1/domain/ptys/${cliPane}/output?after_seq=${state.next_seq-1}`,undefined)).lease_id;
      const observer=await createRuntimeReplyObserver(state.rows,state.cols),began=performance.now();
      try{
       await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from('\x1b[5;6~').toString('base64')});
       const end=Date.now()+10000;while(!observer.hasReply(expected.slice(6))&&Date.now()<end){
        const output=await request(`/v1/domain/ptys/${cliPane}/output/${historyLease}?timeout_ms=50&max_frames=64`,undefined,'GET');assert.equal(output.kind==='lagged',false);
        for(const frame of output.frames??[])observer.feed(Buffer.from(frame.data_b64,'base64'));
       }
       assert.ok(observer.hasReply(expected.slice(6)),'CLI history gesture did not render the actual prior rows');transitions['history/cli'].push(performance.now()-began);
      }finally{observer.close();await request(`/v1/domain/ptys/${cliPane}/output/${historyLease}`,undefined,'DELETE');}
     }
    }
    const id=String(++sequence).padStart(8,'0'),command=`echo R^E^P^L^Y_${id}\r\n`,group=groups[`${scenario}/${client}`];
    const start=performance.now(),loadBefore=loadIndex;const observed=reply(id,start);observed.catch(()=>{});
    try{
     for(let offset=0;offset<command.length;offset+=8){
      const text=command.slice(offset,offset+8);
      if(client==='cli'){const result=await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from(text).toString('base64')});assert.equal(result.ok,true);}
      else await (client==='desktop'?page:remote).evaluate(text=>window.__runtimeBench.send(text),text);
      await new Promise(done=>setTimeout(done,10));
     }
     group.samples.push(await observed);
     if(scenario==='sustained-output')group.loadActive.push(loadIndex>loadBefore&&performance.now()-lastLoadAt<250);
    }catch(error){
     group.timeouts+=Number(String(error).includes('timeout'));group.failures.push(String(error));await observed.catch(()=>{});
     if(group.failureEvidence.length<4)group.failureEvidence.push({id,outputTailBase64:Buffer.from(retained).toString('base64'),
      desktop:await page.evaluate(id=>window.__runtimeBench.evidence(id),id),remote:await remote.evaluate(id=>window.__runtimeBench.evidence(id),id)});
    }
   }
   }
   if(scenario==='warmup'){await page.evaluate(()=>window.__runtimeBench.resetStages());await remote.evaluate(()=>window.__runtimeBench.resetStages());}
  }
  if(minimized)windowStates.push(await setDesktopMinimized(false));
  const history={};
  for(const [client,surface] of [['desktop',page],['remote',remote]]){
   const samples=[];
   for(let round=0;round<32;round++)samples.push(await surface.evaluate(()=>window.__runtimeBench.history()));
   history[client]={samples,latencyMs:distribution(samples.map(sample=>sample.durationMs)),clientApplyMs:distribution(samples.map(sample=>sample.applyMs)),scope:'production history RPC plus WASM page application; fresh bounded mirror for each sample; excludes snapshot setup and GPU rendering'};
  }
  const clients={desktop:await page.evaluate(()=>window.__runtimeBench.snapshot()),remote:await remote.evaluate(()=>window.__runtimeBench.snapshot())};
  const original=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
  assert.equal(original.child_pid,observedPty.child_pid);assert.equal(original.child_start_instance,observedPty.child_start_instance);
  const cli=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===cliPane);assert.ok(cli?.child_pid,'CLI diagnostic child missing');
  const cliLog=await readFile(join(root,'ridge.log'),'utf8').catch(error=>{if(error.code==='ENOENT'&&!diagnostics)return '';throw error;});
  const cliStages=parseCliStages(cliLog,diagnostics,cli.child_pid);
  return {environment:await performanceEnvironment(binaries),diagnostics,loadProducer,windowStates,history,observerStats,transitions:Object.fromEntries(Object.entries(transitions).map(([key,samples])=>[key,{samples,latencyMs:distribution(samples)}])),scope:'same debug build, same live cmd.exe child, loopback; identical 8-byte chunks at 10 ms cadence',sampleCountPerClientPerScenario:32,warmupCountPerClient:8,
   limitations:['includes CDP/ConPTY injection, HTTP observation and bounded test-only VT observer overhead','render measures submission CPU time, not GPU completion or screen presentation','switch controls cover browser clients; CLI remains on its original pane while peer clients switch','reconnect covers restarting the owned LAN listener and existing credentials; local desktop transport remains attached','Host disconnect uses an explicitly disconnected owned loopback outbound Host','CLI stages require enabled diagnostics; stdout flush does not prove screen presentation; WAN/cellular remain unmeasured'],
   groups:Object.fromEntries(Object.entries(groups).map(([key,value])=>[key,{...value,latencyMs:distribution(value.samples)}])),
   clientStages:{...Object.fromEntries(Object.entries(clients).map(([key,value])=>[key,{frames:value.frames,errors:value.errors,queueTrace:value.queueTrace,backend:value.backend,clientApplyMs:distribution(value.feed),renderSubmitMs:distribution(value.paint)}])),cli:cliStages}};
 }catch(error){progress.error=String(error);progress.remote=await remote.evaluate(()=>window.__runtimeBench?.connectionStatus()).catch(()=>null);await writeFile(join(root,'performance-partial.json'),JSON.stringify(progress,null,2));throw error;}finally{
  if(outbound)await command('forget_host',{hostId:outbound}).catch(()=>{});
  if(loadStarted)await unlink(loadFlag).catch(()=>{});
  stopObserver=true;await observeTask?.catch(()=>{});
  replyObserver?.close();
  if(lease)await request(`/v1/domain/ptys/${pane}/output/${lease}`,undefined,'DELETE').catch(()=>{});
  await page.evaluate(()=>window.__runtimeBench?.close()).catch(()=>{});await remote.evaluate(()=>window.__runtimeBench?.close()).catch(()=>{});
  await remoteBrowser.close();
 }
}
