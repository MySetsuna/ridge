// Owned real Codex CLI + local model fixture. No user profile, vendor JSONL
// inspection, installed application, external model request or installer action.
import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdtemp,mkdir,writeFile,readFile,copyFile,open,appendFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {createServer as httpServer} from 'node:http';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
import {ownedAgentDesktop} from './agent-desktop-lifetime.mjs';
const binary=process.env.RIDGE_REAL_APP_SERVER_BIN;
const execute=promisify(execFile);
assert.ok(binary,'Set RIDGE_REAL_APP_SERVER_BIN to the existing Codex CLI executable.');
const root=await mkdtemp(join(process.env.RIDGE_ACCEPTANCE_TEMP||'D:/ridge-070-acceptance/','app-server-terminal-'));
const home=join(root,'home'),data=join(root,'runtime');await mkdir(home);await mkdir(data);
const report={root,completeChange:false,checks:[],providerRequests:0};
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(check,message,ms=30000){const end=Date.now()+ms;while(Date.now()<end){if(await check())return;await delay(100);}throw Error(message);}
const provider=httpServer(async(req,res)=>{
 let body='';for await(const chunk of req)body+=chunk;
 if(!req.url.endsWith('/responses')){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({data:[]}));return;}
 report.providerRequests++;
 const prompt=JSON.parse(body),text=JSON.stringify(prompt).includes('SIDEBAR_MARKER')?'SIDEBAR_REPLY':'TERMINAL_REPLY';
 const id=randomUUID(),item={type:'message',id:randomUUID(),role:'assistant',status:'completed',content:[{type:'output_text',text,annotations:[]}]};
 res.writeHead(200,{'content-type':'text/event-stream','cache-control':'no-cache'});
 const event=(type,extra)=>res.write(`event: ${type}\ndata: ${JSON.stringify({type,...extra})}\n\n`);
 event('response.created',{response:{id,status:'in_progress',output:[]}});
 if(JSON.stringify(prompt).includes('CANCEL_WAIT')) {report.cancelProviderStarted=true;return;}
 event('response.output_item.added',{output_index:0,item:{...item,status:'in_progress',content:[]}});
 event('response.output_text.delta',{output_index:0,item_id:item.id,content_index:0,delta:text});
 event('response.output_item.done',{output_index:0,item});
 event('response.completed',{response:{id,status:'completed',output:[item],usage:{input_tokens:1,output_tokens:1,total_tokens:2}}});res.end();
});await new Promise(r=>provider.listen(0,'127.0.0.1',r));
await writeFile(join(home,'config.toml'),`model_provider = "owned"\nmodel = "owned-fixture"\n[features]\ncode_mode_host = false\nshell_snapshot = false\nremote_plugin = false\n[model_providers.owned]\nname = "Owned local fixture"\nbase_url = "http://127.0.0.1:${provider.address().port}/v1"\nwire_api = "responses"\nsupports_websockets = false\n`);
const listen=createServer();await new Promise(r=>listen.listen(0,'127.0.0.1',r));const port=listen.address().port;await new Promise(r=>listen.close(r));
const url=`ws://127.0.0.1:${port}`;
const kernelBinary=join(root,'owned-kernel.exe');await copyFile(resolve(process.env.RIDGE_ACCEPTANCE_KERNEL_BINARY||'target/debug/ridge-kernel.exe'),kernelBinary);
const log=await open(join(root,'kernel.log'),'a');
const kernel=spawn(kernelBinary,[],{env:{...process.env,RIDGE_KERNEL_DATA_DIR:data,RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'},stdio:['ignore',log.fd,log.fd],windowsHide:true});await log.close();
let endpoint,socket,desktopLifetime;const connectionId=randomUUID();
async function request(path,body,method='POST'){
 const response=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(40000)});
 assert.equal(response.status,200,`${method} ${path}`);return response.json();
}
async function command(method,args){const reply=await request('/v1/domain/commands',{method,args,native:true,roots:[root],connectionId});assert.equal(reply.ok,true,`${method}: ${JSON.stringify(reply)}`);return reply.value;}
try {
 await until(async()=>{assert.equal(kernel.exitCode,null);try{endpoint=JSON.parse(await readFile(join(data,'kernel.json'),'utf8'));return endpoint.pid===kernel.pid;}catch{return false;}},'Runtime did not start');
 const workspace=(await request('/v1/domain/workspaces',{name:'owned terminal app-server'})).workspace_id;
 const pane=(await request(`/v1/domain/workspaces/${workspace}`,undefined,'GET')).panes[0];report.workspace=workspace;report.pane=pane;
 if(process.argv.includes('--plain-terminal')) {
  await appendFile(join(home,'config.toml'),`\n[projects.${JSON.stringify(root)}]\ntrust_level = "trusted"\n`);
  assert.equal((await request('/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:binary,args:['--no-alt-screen','--dangerously-bypass-approvals-and-sandbox','TERMINAL_MARKER'],env:{CODEX_HOME:home,HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost'},cwd:root,rows:40,cols:120})).ok,true);
  let candidate,view;
  await until(async()=>{candidate=(await command('get_teammate_topology',{workspaceId:workspace})).discovered.find(row=>row.name==='codex');return Boolean(candidate);},'plain Codex CLI was not discovered');
  const read=()=>command('get_agent_terminal_view',{workspaceId:workspace,paneId:pane,process:candidate.process,terminalEpoch:candidate.terminalEpoch});
  await until(async()=>{view=await read();report.terminalBefore=view.text;return view.text.includes('TERMINAL_REPLY');},'ordinary terminal Codex did not complete initial local-provider turn',60000);
  const before=await request('/v1/domain/ptys',undefined,'GET');
  if(process.argv.includes('--desktop-lifetime')) {
   desktopLifetime=await ownedAgentDesktop({root,kernelBinary,data,workspace,pane,kernelPid:kernel.pid});
   await desktopLifetime.kill();assert.equal((await request('/v1/health',undefined,'GET')).pid,kernel.pid);
   report.checks.push('actual desktop rendered original external Codex; forced exit of attested desktop/WebView tree preserves Runtime');
  }
  const calls=report.providerRequests;for(let i=0;i<3;i++)await read();assert.equal(report.providerRequests,calls,'terminal reads cannot invoke model');
  assert.equal(candidate.protocolConnected,false,'ordinary CLI must not fabricate protocol attachment');
  const text='SIDEBAR_MARKER',paste=view.bracketedPaste?`\x1b[200~${text}\x1b[201~`:text;
  const input={process:candidate.process,input:{pane:{workspaceId:workspace,paneId:pane},terminalEpoch:candidate.terminalEpoch,activationId:1,sourceId:randomUUID(),sequence:1,bytes:[...Buffer.from(paste)]}};
  assert.equal((await command('write_agent_terminal_input',input)).terminalAccepted,true);
  assert.equal((await command('write_agent_terminal_input',input)).alreadyApplied,true);
  await delay(300);
  input.input.sequence=2;input.input.bytes=view.keyBytes.Enter;
  assert.equal((await command('write_agent_terminal_input',input)).terminalAccepted,true);
  await until(async()=>{report.terminalAfter=(await read()).text;return report.terminalAfter.includes('SIDEBAR_REPLY');},'sidebar input failed to reach same plain terminal Codex',60000);
  const after=await request('/v1/domain/ptys',undefined,'GET');
  const identities=inventory=>inventory.ptys.map(p=>({id:p.pty_id,pid:p.child_pid,start:p.child_start_instance,workspace:p.workspace_id,program:p.program}));
  assert.deepEqual(identities(after),identities(before));
  if(desktopLifetime) {
   await desktopLifetime.reopen('SIDEBAR_REPLY');
   assert.deepEqual(identities(await request('/v1/domain/ptys',undefined,'GET')),identities(before));
   report.checks.push('same external Codex PID/start instance replies after desktop death; reopened desktop renders that same live reply and pane');
  }
  assert.deepEqual(await command('read_agent_recent_replies',{workspaceId:workspace,limit:100}),[],'terminal observation must not invent protocol history');
  report.checks.push('ordinary standalone Codex CLI, no shared app-server or descriptor; sidebar input receives reply in original PTY; duplicate ACK executes once; reads do not invoke model or invent protocol history');
 } else {
 const quote=value=>`'${value.replaceAll("'","''")}'`;
 const launcher=join(root,'terminal.ps1'),pidFile=join(root,'server.json'),threadFile=join(root,'thread-id.txt');
 await writeFile(launcher,`$ErrorActionPreference='Stop'\n$agent=Start-Process -FilePath ${quote(binary)} -ArgumentList @('app-server','--listen',${quote(url)}) -PassThru -WindowStyle Hidden -RedirectStandardOutput ${quote(join(root,'server.out'))} -RedirectStandardError ${quote(join(root,'server.err'))}\n@{pid=$agent.Id;startInstance=('win:'+ $agent.StartTime.ToUniversalTime().ToFileTimeUtc().ToString('x16'))}|ConvertTo-Json -Compress|Set-Content -LiteralPath ${quote(pidFile)}\nwhile(!(Test-Path -LiteralPath ${quote(threadFile)})){Start-Sleep -Milliseconds 100}\n$thread=(Get-Content -LiteralPath ${quote(threadFile)} -Raw).Trim()\n& ${quote(binary)} --remote ${quote(url)} resume $thread\n`);
 assert.equal((await request('/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:'C:/Program Files/PowerShell/7/pwsh.exe',args:['-NoProfile','-File',launcher],env:{CODEX_HOME:home,HTTP_PROXY:'',HTTPS_PROXY:'',ALL_PROXY:'',NO_PROXY:'127.0.0.1,localhost'},cwd:root,rows:40,cols:120})).ok,true);
 let processIdentity;await until(async()=>{try{processIdentity=JSON.parse((await readFile(pidFile,'utf8')).replace(/^\uFEFF/,''));return true;}catch{return false;}},'terminal server process missing');
 await until(async()=>{try{socket=new WebSocket(url);await new Promise((r,j)=>{socket.onopen=r;socket.onerror=j;});return true;}catch{return false;}},'owned app-server did not listen');
 let id=0;const pending=new Map(),notifications=[];report.notifications=notifications;
 socket.onmessage=e=>{const value=JSON.parse(e.data);if(value.id!==undefined&&pending.has(value.id)){pending.get(value.id)(value);pending.delete(value.id);}else notifications.push(value);};
 async function call(method,params){const i=++id;let timer;const response=new Promise((r,j)=>{pending.set(i,v=>{clearTimeout(timer);r(v);});timer=setTimeout(()=>j(Error(`timeout ${method}`)),15000);});socket.send(JSON.stringify({id:i,method,params}));const reply=await response;assert.equal(reply.error,undefined,`${method}: ${JSON.stringify(reply)}`);return reply.result;}
 await call('initialize',{clientInfo:{name:'owned_terminal_fixture',version:'0.1'},capabilities:{experimentalApi:true}});socket.send(JSON.stringify({method:'initialized'}));
 const started=await call('thread/start',{cwd:root,approvalPolicy:'never'});report.started=started;
 const thread=started.thread.id;report.thread=thread;
 async function terminalText(){
  const response=await fetch(`http://127.0.0.1:${endpoint.port}/api/v1/mcp`,{method:'POST',headers:{authorization:`Bearer ${endpoint.token}`,'content-type':'application/json'},body:JSON.stringify({jsonrpc:'2.0',id:1,method:'tools/call',params:{name:'ridge_capture_pane',arguments:{workspace_id:workspace,target_pane_id:pane,lines:100}}})});
  const reply=await response.json();return reply.result?.content?.[0]?.text??JSON.stringify(reply);
 }
 await call('turn/start',{threadId:thread,input:[{type:'text',text:'TERMINAL_MARKER',text_elements:[]}]});
 await until(async()=>notifications.some(n=>n.method==='turn/completed'),'seed turn did not complete');
 await writeFile(threadFile,thread);
 await until(async()=>{report.terminalBefore=await terminalText();return report.terminalBefore.includes('TERMINAL_REPLY');},'the terminal TUI did not attach and display the existing thread');
 const descriptor={protocol:'app-server',process:processIdentity,endpointOwner:processIdentity,endpoint:`127.0.0.1:${port}`,sessionId:thread,cwd:root};
 const descriptorFile=join(root,`connection-${processIdentity.pid}.json`);await writeFile(descriptorFile,JSON.stringify(descriptor));
 // The running party supplies an exact descriptor; catalog opt-in activates the
 // real Runtime supervisor. No direct register RPC is used in this acceptance.
 await command('save_agent_profile_overrides',{overrides:[{id:'codex',processNames:['codex'],executable:'codex',resumeArgv:[],yoloArgs:[],connection:{protocol:'app-server',descriptorFile:join(root,'connection-{pid}.json'),autoConnect:true}}]});
 let member;await until(async()=>{const topology=await command('get_teammate_topology',{workspaceId:workspace});member=topology.roster.find(m=>m.online&&m.sessionId===thread);return Boolean(member);},'automatic protocol attachment failed');
 report.member=member;report.checks.push('Runtime automatically discovers and attaches the exact running party descriptor');
 let history=await command('read_agent_recent_replies',{workspaceId:workspace,limit:100});
 assert.ok(history.some(row=>row.text==='TERMINAL_REPLY'),JSON.stringify(history));report.checks.push('pre-attachment history replays through app-server protocol');
 const before=await request('/v1/domain/ptys',undefined,'GET');
 let accepted;
 if(process.argv.includes('--official-a2a')) {
  const configFile=join(root,'a2a-client-input.json');
  await writeFile(configFile,JSON.stringify({baseUrl:`http://127.0.0.1:${endpoint.port}`,token:endpoint.token}),{mode:0o600});
  const result=await execute(process.env.RIDGE_A2A_SDK_PYTHON||'D:/ridge-070-acceptance/a2a-sdk-1.2.2/Scripts/python.exe',['scripts/commune-a2a-sdk-acceptance.py',configFile],{windowsHide:true,timeout:60000});
  report.officialSdk=JSON.parse(await readFile(join(root,'a2a-sdk-result.json'),'utf8'));
  assert.equal(report.officialSdk.pass,true);assert.equal(report.cancelProviderStarted,true);
  report.checks.push(...report.officialSdk.checks);accepted={deliveryId:report.officialSdk.taskId};
 } else accepted=await command('send_agent_message',{workspaceId:workspace,paneId:pane,agentId:member.agentId,generation:member.generation,lease:member.lease,message:'SIDEBAR_MARKER',idempotency_key:randomUUID()});
 report.accepted=accepted;
 let receipt;await until(async()=>{receipt=await command('get_agent_message_delivery_status',{workspaceId:workspace,agentId:member.agentId,generation:member.generation,lease:member.lease,deliveryId:accepted.deliveryId});report.receipt=receipt;return ['turn_ended','failed','rejected','unknown'].includes(receipt.execution?.state);},'sidebar turn did not reach a real terminal result');
 assert.equal(receipt.execution.state,'turn_ended',JSON.stringify(receipt));assert.equal(receipt.execution.response.stopReason,'end_turn');
 const turns=await call('thread/turns/list',{threadId:thread,limit:50,sortDirection:'asc',itemsView:'full'});
 assert.ok(JSON.stringify(turns).includes('SIDEBAR_REPLY'));assert.equal(turns.data.length,process.argv.includes('--official-a2a')?3:2,'sidebar must not create a different thread');
 await until(async()=>{report.terminalAfter=await terminalText();return report.terminalAfter.includes('SIDEBAR_REPLY');},'sidebar reply did not appear in the same terminal TUI');
 history=await command('read_agent_recent_replies',{workspaceId:workspace,limit:100});
 assert.equal(history.filter(row=>row.text==='SIDEBAR_REPLY').length,1,JSON.stringify(history));
 const after=await request('/v1/domain/ptys',undefined,'GET');
 const identities=inventory=>inventory.ptys.map(p=>({id:p.pty_id,pid:p.child_pid,start:p.child_start_instance,workspace:p.workspace_id,program:p.program}));
 assert.deepEqual(identities(after),identities(before),'registration and dispatch cannot replace the terminal process');
 report.checks.push('sidebar sends to the same loaded thread; actual completion and one history item observed; same PTY process retained');
 const calls=report.providerRequests;for(let i=0;i<3;i++)await command('read_agent_recent_replies',{workspaceId:workspace,limit:100});assert.equal(report.providerRequests,calls,'history reads must not call the model');
 report.checks.push('history reads do not resume, create conversations, or invoke a model');
 }
 report.pass=true;await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({pass:true,root,checks:report.checks}));
} catch(error){report.error=error.stack;await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));throw error;}
finally {
 await desktopLifetime?.close();
 socket?.close();
 if(endpoint)await request('/v1/shutdown',{}).catch(()=>{});
 if(kernel.exitCode===null)await Promise.race([new Promise(r=>kernel.once('exit',r)),delay(5000)]);
 provider.closeAllConnections();provider.close();
}
