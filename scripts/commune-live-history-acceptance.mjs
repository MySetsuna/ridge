// Production components with controlled delayed reads/events. Real Kernel IO is
// separately covered by commune-global-ui-acceptance.mjs; this is no WAN/device claim.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import { spawn, execFileSync } from 'node:child_process';
import ts from 'typescript';
import { chromium, expect } from '@playwright/test';

const dir=path.resolve('artifacts/commune087');fs.mkdirSync(dir,{recursive:true});
const evidence={scope:'Current production history/roster browser components; explicit controlled async IO/events, separate real Kernel regression',checks:[],result:'RUNNING'};
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const sourceUrl=file=>'/@fs/'+path.resolve(file).replaceAll('\\','/');
let vite,browser,page;
try {
  const port=await new Promise(resolve=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const p=server.address().port;server.close(()=>resolve(p));});});
  const log=fs.openSync(path.join(dir,'vite.log'),'a');
  vite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--config','vite.remote.config.js'],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,RIDGE_REMOTE_DEV_PORT:String(port)}});fs.closeSync(log);
  const url=`http://127.0.0.1:${port}`;let ready=false;
  for(let i=0;i<150;i++){if(await fetch(url,{signal:AbortSignal.timeout(1000)}).then(r=>r.ok,()=>false)){ready=true;break;}await delay(100);}
  assert.ok(ready,'owned Vite readiness');
  browser=await chromium.launch({headless:true});page=await browser.newPage({viewport:{width:412,height:915}});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.route('**/__commune_live_probe',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><main id="component"></main>'}));
  await page.goto(url+'/__commune_live_probe');
  await page.evaluate(async({clientUrl,historyUrl,bridgeUrl,rosterUrl})=>{
    const [client,{default:History},{bridge},{default:Roster}]=await Promise.all([import(clientUrl),import(historyUrl),import(bridgeUrl),import(rosterUrl)]);
    const listeners=new Set();bridge.attach({sendControl(){},onControl(fn){listeners.add(fn);return()=>listeners.delete(fn);},sendPaneBytes(){},onPaneBytes(){return()=>{};},connect(){},close(){},state:()=> 'connected',onStateChange:()=>()=>{},authState:()=> 'authorized',onAuthChange:()=>()=>{}});
    const h={calls:0,active:0,peak:0,delay:0,gates:{},hold:'',pending:{},wrongText:false,newest:false};
    const row=(workspaceId,id,text,timestamp)=>({workspaceId,id,memberId:'member',sessionId:'session',sequence:timestamp,timestamp,agent:'Owned',kind:'agent_message_chunk',title:'assistant',text,cwd:''});
    const all=workspaceId=>Array.from({length:240},(_,i)=>({...row(workspaceId,String(i),i===0?'NEW_NEEDLE':i===1?'OLD_NEEDLE':'HISTORY_'+i,1000-i),...(i===0?{textCursor:'next-text'}:{})}));
    const load=async request=>{
      h.calls++;h.active++;h.peak=Math.max(h.peak,h.active);
      const data=all(request.workspaceId);if(h.newest)data.unshift(row(request.workspaceId,'live','LIVE_NEW',2000));
      try {
        if(h.hold===(request.textCursor??request.query)&&h.hold){h.pending[h.hold]=true;await new Promise(resolve=>h.gates[h.hold]=resolve);}
        if(h.delay)await new Promise(resolve=>setTimeout(resolve,h.delay));
        if(request.textCursor)return [{...data.find(item=>item.id==='0'),...(h.wrongText?{workspaceId:'forbidden-source',id:'wrong'}:{}),text:'LONG_TEXT_PAGE',textCursor:undefined}];
        const filtered=request.query?data.filter(item=>item.text.includes(request.query)):data;
        const offset=Number(request.cursor??0),limit=request.limit??50,page=filtered.slice(offset,offset+limit).map(item=>({...item}));
        if(offset+limit<filtered.length)page.at(-1).cursor=String(offset+limit);return page;
      }finally{h.active--;}
    };
    const mountHistory=(scope='a')=>client.createClassComponent({component:History,target:document.getElementById('component'),props:{workspaceIds:[scope],visible:true,load}});
    window.probe={h,client,Roster,app:mountHistory(),mountHistory,emit:(name,workspaceId='a')=>{for(const fn of listeners)fn({type:'event',name,payload:{workspaceId}});}};
  },{clientUrl:sourceUrl('scripts/commune-permission-ui-client.mjs'),historyUrl:sourceUrl('src/lib/teammate/CommuneHistory.svelte'),bridgeUrl:sourceUrl('src/lib/transport/tauriShim/bridge.ts'),rosterUrl:sourceUrl('src/remote/lib/CommuneWorkspaceRoster.svelte')});
  const history=()=>page.getByTestId('commune-history'),rows=()=>history().locator('li'),search=()=>history().getByRole('searchbox');
  const refresh=()=>history().getByRole('button',{name:/Refresh|刷新/}).first().click();
  await expect(rows()).toHaveCount(100);
  await page.evaluate(()=>{window.probe.h.hold='OLD_NEEDLE';});await search().fill('OLD_NEEDLE');await page.waitForFunction(()=>window.probe.h.pending.OLD_NEEDLE);
  await search().fill('NEW_NEEDLE');await page.evaluate(()=>{window.probe.h.hold='';window.probe.h.gates.OLD_NEEDLE();});
  await expect(history().locator('pre').filter({hasText:/^OLD_NEEDLE$/})).toHaveCount(0);await expect(rows()).toHaveCount(1);await expect(rows().locator('pre')).toHaveText('NEW_NEEDLE');
  evidence.checks.push('History search fences a late old-query response during the debounce window');
  await page.evaluate(()=>window.probe.h.wrongText=true);await history().getByRole('button',{name:/^(Later text|Next text|后文|下一段|下一页文本)/}).click();
  await expect(history().getByRole('alert')).toContainText('identity mismatch');await expect(rows().locator('pre')).toHaveText('NEW_NEEDLE');
  await page.evaluate(()=>window.probe.h.wrongText=false);await history().getByRole('button',{name:/^(Later text|Next text|后文|下一段|下一页文本)/}).click();
  await expect(rows().locator('pre')).toHaveText('LONG_TEXT_PAGE');const textCalls=await page.evaluate(()=>window.probe.h.calls);
  await page.evaluate(()=>window.probe.emit('commune-session-update'));await expect(history().getByRole('button',{name:/new|新/i})).toBeVisible();await delay(250);
  assert.equal(await page.evaluate(()=>window.probe.h.calls),textCalls);await expect(rows().locator('pre')).toHaveText('LONG_TEXT_PAGE');
  await refresh();await expect(rows().locator('pre')).toHaveText('NEW_NEEDLE');
  await page.evaluate(()=>{window.probe.h.wrongText=false;window.probe.h.hold='next-text';});
  await history().getByRole('button',{name:/^(Later text|Next text|后文|下一段|下一页文本)/}).click();await page.waitForFunction(()=>window.probe.h.pending['next-text']);
  await search().fill('OLD_NEEDLE');await page.evaluate(()=>{window.probe.h.hold='';window.probe.h.gates['next-text']();});
  await expect(rows().locator('pre')).toHaveText('OLD_NEEDLE');await expect(history()).not.toContainText('LONG_TEXT_PAGE');
  evidence.checks.push('Text pagination rejects another event/source, retains its read position across events and cannot overwrite a new search');
  await search().fill('');await expect(rows()).toHaveCount(100);await history().getByRole('button',{name:/^(Earlier|更早)/}).click();await expect(rows()).toHaveCount(200);
  const before=await page.evaluate(()=>window.probe.h.calls);await page.evaluate(()=>{window.probe.h.newest=true;window.probe.emit('commune-session-update');});
  await expect(history().getByRole('button',{name:/new|新/i})).toBeVisible();await delay(400);assert.equal(await page.evaluate(()=>window.probe.h.calls),before);
  await expect(rows()).toHaveCount(200);await expect(history()).not.toContainText('LIVE_NEW');await refresh();await expect(history()).toContainText('LIVE_NEW');await expect(rows()).toHaveCount(100);
  evidence.checks.push('New events preserve earlier-history browsing without requests; explicit refresh returns to latest');
  await page.evaluate(()=>{const p=window.probe;p.h.delay=400;p.h.active=0;p.h.peak=0;p.h.newest=false;});await refresh();
  await page.evaluate(()=>{window.probe.storm=setInterval(()=>window.probe.emit('commune-session-update'),40);});
  await expect(history()).not.toContainText('LIVE_NEW',{timeout:1600});assert.equal(await page.evaluate(()=>window.probe.h.peak),1);
  await page.evaluate(()=>{clearInterval(window.probe.storm);window.probe.h.delay=0;});await page.waitForFunction(()=>window.probe.h.active===0);await delay(250);
  evidence.checks.push('Continuous live events allow a slow history read to commit, with one active source read');
  await page.evaluate(()=>{delete window.probe.h.pending.OLD_NEEDLE;window.probe.h.hold='OLD_NEEDLE';});await search().fill('OLD_NEEDLE');await page.waitForFunction(()=>window.probe.h.pending.OLD_NEEDLE);
  await page.evaluate(()=>{window.probe.app.$set({workspaceIds:['b'],canLocate:row=>row.workspaceId==='b'});window.probe.h.hold='';window.probe.h.gates.OLD_NEEDLE();});await expect(rows().locator('pre')).toHaveText('OLD_NEEDLE');
  await expect(history().getByRole('button',{name:/定位终端|Locate terminal/i})).toBeVisible();
  await search().fill('PENDING_UNMOUNT');const unmountCalls=await page.evaluate(()=>{window.probe.app.$destroy();return window.probe.h.calls;});await delay(350);assert.equal(await page.evaluate(()=>window.probe.h.calls),unmountCalls);
  evidence.checks.push('Source change fences old work; unmount cancels pending search timers');
  await page.evaluate(()=>{
    const p=window.probe,r={name:'initial',calls:0,active:0,peak:0,completed:[],snapshots:[]};const messages=new Set();
    const link={hasCapability:()=>true,onMessage:fn=>{messages.add(fn);return()=>messages.delete(fn);},onRawBytes:()=>()=>{},onReconnect:fn=>{p.reconnect=fn;return()=>{};},onCapabilitiesChanged:fn=>{fn();return()=>{};},
      getTeammateTopology:async()=>{r.calls++;r.active++;r.peak=Math.max(r.peak,r.active);const name=r.name;try{if(r.hold){r.pending=true;await new Promise(resolve=>r.release=resolve);}return{roster:[{id:'member',agentId:'member',paneId:'pane',sessionId:'session',workspaceId:'a',name,status:'idle',online:true}],leaderId:null,edges:[]};}finally{r.active--;r.completed.push({name,time:performance.now()}); }},
      listAgentHistory:async()=>[],listHitlPending:async()=>[],getOrchestrationHealth:async()=>({suspendedAgents:0,pendingHitl:0})};
    p.r=r;p.rosterEvent=(name,workspaceId='a')=>{for(const fn of messages)fn({type:'event',name,payload:{workspaceId}});};
    const queryClient=new p.client.QueryClient({defaultOptions:{queries:{retry:false}}});
    p.mountRoster=()=>p.client.createClassComponent({component:p.Roster,target:document.getElementById('component'),props:{ws:link,workspaceId:'a',globalView:true,queryClient,onSnapshot:snapshot=>r.snapshots.push({name:snapshot.roster[0]?.name,time:performance.now()})}});
    p.app=p.mountRoster();
  });
  const member=()=>page.locator('.member-card .name[title="member"]');await expect(member()).toHaveText('initial');
  const warmReads=await page.evaluate(()=>window.probe.r.calls);await page.evaluate(()=>{window.probe.app.$destroy();window.probe.app=window.probe.mountRoster();});await expect(member()).toHaveText('initial');
  assert.equal(await page.evaluate(()=>window.probe.r.calls),warmReads);evidence.checks.push('Warm roster remount uses the real QueryClient cache without another topology read');
  await page.evaluate(()=>{window.probe.r.name='reconnected';window.probe.reconnect();});await expect(member()).toHaveText('reconnected');
  assert.equal(await page.evaluate(()=>window.probe.r.calls),warmReads+1);
  await page.evaluate(()=>{window.probe.r.name='quiet-member';window.probe.rosterEvent('commune-members-changed');});await expect(member()).toHaveText('quiet-member');
  const count=await page.evaluate(()=>window.probe.r.calls);await page.evaluate(()=>window.probe.rosterEvent('commune-session-update','unrelated'));await delay(350);assert.equal(await page.evaluate(()=>window.probe.r.calls),count);
  evidence.checks.push('Quiet membership changes refresh narrow roster; unrelated source events cause no reads');
  await page.evaluate(()=>{const p=window.probe;p.r.hold=true;p.r.name='slow-but-visible';p.rosterEvent('commune-members-changed');p.storm=setInterval(()=>p.rosterEvent('commune-session-update'),50);});
  await page.waitForFunction(()=>window.probe.r.pending);await delay(500);await page.evaluate(()=>{window.probe.r.hold=false;window.probe.r.release();});
  await expect(member()).toHaveText('slow-but-visible');assert.equal(await page.evaluate(()=>window.probe.r.peak),1);
  await page.evaluate(()=>{clearInterval(window.probe.storm);window.probe.app.$destroy();});
  const callsAfterDestroy=await page.evaluate(()=>window.probe.r.calls);await delay(800);assert.equal(await page.evaluate(()=>window.probe.r.calls),callsAfterDestroy);
  evidence.checks.push('Slow roster reads commit during event floods, remain single-flight and stop after teardown');
  assert.deepEqual(errors,[]);
  const compile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  const Before=(await import('data:text/javascript;base64,'+Buffer.from(compile(execFileSync('git',['show','937f4093:src/lib/teammate/communeHistoryPages.ts'],{encoding:'utf8'}))).toString('base64'))).CommuneHistoryPages;
  const After=(await import('data:text/javascript;base64,'+Buffer.from(compile(fs.readFileSync('src/lib/teammate/communeHistoryPages.ts','utf8'))).toString('base64'))).CommuneHistoryPages;
  const measure=async Pages=>{let reads=0;const ids=Array.from({length:32},(_,i)=>String(i)),total=6400;const start=performance.now();
    const pages=new Pages(ids,async({workspaceId,cursor,limit})=>{reads++;const offset=Number(cursor??0),index=Number(workspaceId);const rows=Array.from({length:Math.min(limit,200-offset)},(_,i)=>({workspaceId,id:String(offset+i),memberId:'m',sequence:offset+i,timestamp:total-((offset+i)*32+index)}));if(offset+limit<200)rows.at(-1).cursor=String(offset+limit);return rows;});
    const result=[];while(pages.hasMore)result.push(...await pages.next(100));assert.equal(result.length,total);assert.equal(new Set(result.map(row=>row.workspaceId+':'+row.id)).size,total);assert.ok(result.every((row,i)=>i===0||result[i-1].timestamp>row.timestamp));assert.equal(reads,128);return{ms:performance.now()-start,reads,rows:result.length};};
  await measure(Before);await measure(After);evidence.paginationBenchmark={sources:32,rows:6400,pageSize:100,before:[],after:[]};
  for(let i=0;i<6;i++){evidence.paginationBenchmark.before.push(await measure(Before));evidence.paginationBenchmark.after.push(await measure(After));}
  evidence.result='PASS';
}catch(error){evidence.result='FAIL';evidence.error=String(error);process.exitCode=1;if(page){evidence.debug=await page.evaluate(()=>({history:window.probe?.h,roster:window.probe?.r})).catch(()=>undefined);await page.screenshot({path:path.join(dir,'failure.png')}).catch(()=>{});}}
finally{await browser?.close();if(vite?.pid)await new Promise(resolve=>{const child=process.platform==='win32'?spawn('taskkill',['/PID',String(vite.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'}):spawn('kill',[String(vite.pid)],{stdio:'ignore'});child.once('exit',resolve);});fs.writeFileSync(path.join(dir,'live-history.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify({result:evidence.result,error:evidence.error,evidence:path.join(dir,'live-history.json')}));}
