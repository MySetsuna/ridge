import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, copyFile, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import http from 'node:http';
import { WebSocketServer } from 'ws';

const root=await mkdtemp(join(tmpdir(),'ridge-cli-reconnect-'));
console.log(`Fixture: ${root}`);
const data=join(root,'data');await mkdir(data);
const kernelPath=join(root,'fixture-kernel.exe'),cliPath=join(root,'fixture-cli.exe');
await copyFile(resolve('target/debug/ridge-kernel.exe'),kernelPath);
await copyFile(resolve('target/070-cli-controller/debug/ridge.exe'),cliPath);
const kernel=spawn(kernelPath,[],{windowsHide:true,stdio:'ignore',env:{...process.env,
  RIDGE_KERNEL_DATA_DIR:data,RIDGE_KERNEL_FS_ROOT:root,RIDGE_AUTH_FILE:join(root,'auth.json'),
  RIDGE_WORKSPACE_SAVE_DIR:root,RIDGE_TEST_TLS_DIR:join(root,'tls'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'}});
let endpoint;
const delay=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message){const end=Date.now()+15000;while(Date.now()<end){if(await check())return;await delay(50);}throw Error(message);}
async function request(path,body,method='POST'){
  const response=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{method,
    headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},
    body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
  assert.equal(response.status,200);return response.json();
}
const connectionId=randomUUID();
async function command(method,args={}){const response=await request('/v1/domain/commands',{method,args,native:true,roots:[],connectionId});assert.equal(response.ok,true,JSON.stringify(response.error));return response.value;}
async function ptys(){return (await request('/v1/domain/ptys',undefined,'GET')).ptys;}
async function headlessScenario(){
  const children=[];
  const env={...process.env,RIDGE_KERNEL_DATA_DIR:data,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_BASE_DOMAIN:'localhost:9'};
  function launch(args,extra={}){const child=spawn(cliPath,args,{env:{...env,...extra},cwd:root,windowsHide:true,stdio:'ignore'});children.push(child);return child;}
  async function exited(child){await until(()=>child.exitCode!==null||child.signalCode!==null,'owned headless CLI did not exit');}
  async function receipt(path){try{return JSON.parse(await readFile(path,'utf8'));}catch{return null;}}
  async function missing(path){try{await readFile(path);return false;}catch(error){return error.code==='ENOENT';}}
  try{
    await command('set_remote_enabled',{enabled:true,port:0});
    const before=await command('get_remote_info');
    const requested=before.port===65526?65525:65526,statusPath=join(root,'headless-status.json');
    const beforePanes=new Set((await ptys()).map(pty=>pty.pty_id));
    const headless=launch(['tui','--lan-host','--port',String(requested),'--shell','cmd.exe','--cwd',root],{RIDGE_LAN_STATUS_FILE:statusPath});
    await until(async()=>{const state=await receipt(statusPath);return state?.ready===true&&state.port===before.port&&state.pid===headless.pid;},'headless CLI did not report the existing Runtime listener actual port');
    const original=(await ptys()).find(pty=>!beforePanes.has(pty.pty_id));assert.ok(original?.child_pid);
    await command('set_remote_enabled',{enabled:false});await exited(headless);assert.notEqual(headless.exitCode,0);
    assert.ok(await missing(statusPath),'headless CLI retained a stale ready receipt');
    const output=join(root,'headless-child.txt');
    await request(`/v1/domain/ptys/${original.pty_id}/write`,{data_b64:Buffer.from(`echo ORIGINAL_CHILD_SURVIVES > "${output.replaceAll('\\','/')}"\r\n`).toString('base64')});
    await until(async()=>{try{return (await readFile(output,'utf8')).trim()==='ORIGINAL_CHILD_SURVIVES';}catch{return false;}},'headless client failure terminated its Runtime child');
    const after=(await ptys()).find(pty=>pty.pty_id===original.pty_id);assert.equal(after.child_pid,original.child_pid);assert.equal(after.child_start_instance,original.child_start_instance);
    await command('set_remote_enabled',{enabled:true,port:0});
    const resumed=await command('get_remote_info'),registryPath=join(root,'host-registry.json');
    let host=launch(['host','--port',String(requested)],{RIDGE_REMOTE_HOST_REGISTRY:registryPath});
    await until(async()=>{const state=await receipt(registryPath);return state?.pid===kernel.pid&&state.port===resumed.port;},'host CLI did not attest the Runtime listener');
    host.kill();await exited(host);
    assert.equal((await command('get_remote_info')).port,resumed.port);assert.equal(await command('get_remote_enabled'),true,'killing the host CLI stopped Runtime LAN');
    const secondRegistry=join(root,'second-host-registry.json');
    host=launch(['host','--port',String(requested)],{RIDGE_REMOTE_HOST_REGISTRY:secondRegistry});
    await until(async()=>(await receipt(secondRegistry))?.port===resumed.port,'second host CLI did not attach');
    await command('set_remote_enabled',{enabled:false});await exited(host);assert.notEqual(host.exitCode,0);assert.ok(await missing(secondRegistry),'host CLI retained stale service readiness');
    const invalidStatus=join(root,'invalid-start-status.json');
    const rejected=launch(['tui','--lan-host','--port','65535','--shell','cmd.exe','--cwd',root],{RIDGE_LAN_STATUS_FILE:invalidStatus});
    await exited(rejected);assert.notEqual(rejected.exitCode,0);assert.ok(await missing(invalidStatus));assert.equal(await command('get_remote_enabled'),false);
    await writeFile(join(root,'result.json'),JSON.stringify({completeChange:false,mode:'Runtime-attested headless LAN',runtimePid:kernel.pid,checks:['headless CLI uses the existing Runtime actual port rather than probing its requested port','external LAN stop terminates CLI with failure and clears stale readiness','original child identity survives headless client failure','host CLI registry attests Runtime PID and actual port','force-killing host CLI leaves Runtime LAN alive','host CLI detects external LAN stop and clears readiness','rejected startup exits with failure and never publishes ready']},null,2));
    console.log(`PASS Runtime-attested headless LAN: ${join(root,'result.json')}`);
  }finally{for(const child of children){if(child.exitCode===null&&child.signalCode===null){child.kill();await exited(child);}}}
}
async function dashboardScenario(){
  const {default:init,TerminalKernel}=await import('../packages/ridge-term/pkg/ridge_term.js');
  await init({module_or_path:await readFile('packages/ridge-term/pkg/ridge_term_bg.wasm')});
  async function visible(pane){
    const output=await request(`/v1/domain/ptys/${pane}?max_bytes=65536`,undefined,'GET');
    const terminal=new TerminalKernel(36,120,256);
    try{terminal.feed(Buffer.from(output.data_b64??'','base64'));return terminal.dumpVisibleText().join('\n');}finally{terminal.free();}
  }
  const server=http.createServer((request,response)=>{
    assert.equal(request.url,'/api/v1/ice-servers');
    response.setHeader('content-type','application/json');response.end(JSON.stringify({ok:true,data:{iceServers:[]}}));
  });
  await new Promise(done=>server.listen(0,'127.0.0.1',done));
  const port=server.address().port,relay=new WebSocketServer({server});
  relay.on('connection',(socket,request)=>{
    const url=new URL(request.url,'http://localhost');
    assert.equal(url.searchParams.get('role'),'host');assert.equal(url.searchParams.get('token'),'owned-device');
    socket.send(JSON.stringify({t:'welcome',room:'owned',role:'host',peerPresent:false}));
  });
  await writeFile(join(root,'auth.json'),JSON.stringify({token:'owned-device',device_name:'desktop',username:'owner'}));
  const cliEnv={RIDGE_KERNEL_DATA_DIR:data,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_BASE_DOMAIN:`localhost:${port}`,RIDGE_CLOUD_DEV_PLAINTEXT:'1'};
  async function launch(){
    const pane=randomUUID();
    assert.equal((await request('/v1/domain/ptys',{pty_id:pane,program:cliPath,args:[],cwd:root,env:cliEnv,role:'shell',cols:120,rows:36})).ok,true);
    await until(async()=>(await visible(pane)).includes('Show TOTP QR code'),'Dashboard did not render');
    const process=(await ptys()).find(pty=>pty.pty_id===pane);assert.ok(process?.child_pid);
    return {pane,pid:process.child_pid};
  }
  async function send(cli,text){await request(`/v1/domain/ptys/${cli.pane}/write`,{data_b64:Buffer.from(text).toString('base64')});}
  async function quit(cli){await send(cli,'q');await until(()=>{try{process.kill(cli.pid,0);return false;}catch{return true;}},'Dashboard did not detach');}
  try{
    await command('set_remote_enabled',{enabled:true,port:0});
    const originalLan=await command('get_remote_info');
    let cli=await launch();
    await until(async()=>{const text=await visible(cli.pane);return text.includes('Running')&&text.includes(`:${originalLan.port}`);},'Dashboard did not project the existing Runtime LAN listener');
    await quit(cli);
    assert.equal((await command('get_remote_info')).port,originalLan.port);assert.equal(await command('get_remote_enabled'),true,'Dashboard exit stopped the shared LAN listener');
    cli=await launch();await send(cli,'jj\r');
    await until(async()=>!(await command('get_remote_enabled')),'Dashboard stop LAN did not reach Runtime');
    await send(cli,'j\r');
    await until(async()=>(await command('get_cloud_host_state')).state==='online','Dashboard start public Remote did not reach the owned relay');
    await quit(cli);
    assert.equal((await command('get_cloud_host_state')).state,'online','Dashboard exit stopped Runtime public Remote');
    cli=await launch();await send(cli,'jjjj\r');
    await until(async()=>(await command('get_cloud_host_state')).state==='offline','reopened Dashboard stop public Remote did not stop the Runtime owner');
    await until(async()=>(await visible(cli.pane)).includes('Public Remote stopped'),'Dashboard stop result was not projected');
    await quit(cli);
    await writeFile(join(root,'result.json'),JSON.stringify({completeChange:false,mode:'Runtime-owned Dashboard controls',runtimePid:kernel.pid,checks:['existing Runtime LAN state and actual port are projected','Dashboard exit preserves the original LAN listener','explicit LAN stop reaches Runtime','public Remote starts against the owned signaling relay','Dashboard exit preserves Runtime public Remote','reopened Dashboard explicitly stops public Remote without PID files or process killing']},null,2));
    console.log(`PASS Runtime-owned Dashboard: ${join(root,'result.json')}`);
  }finally{
    await command('cloud_host_offline').catch(()=>{});
    for(const socket of relay.clients)socket.terminate();
    await new Promise(done=>server.close(done));
  }
}
async function localScenario(){
  const pager=process.env.RIDGE_CLI_TEST_PAGER==='1';
  const cliPane=randomUUID();
  assert.equal(await command('get_remote_enabled'),false);
  assert.equal((await request('/v1/domain/ptys',{pty_id:cliPane,program:cliPath,args:['tui','--shell','cmd.exe',...(pager?['--sessions','2']:['--cwd',root])],cwd:root,
    env:{RIDGE_KERNEL_DATA_DIR:data,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_CLI_PERF_TRACE:process.env.RIDGE_CLI_PERF_TRACE==='1'?'1':'0'},role:'shell',cols:80,rows:24})).ok,true);
  let original;
  await until(async()=>{original=(await ptys()).find(pty=>pty.pty_id!==cliPane&&pty.workspace_id);return Boolean(original?.child_pid);},'local TUI did not create a Runtime-owned workspace terminal');
  const cli=(await ptys()).find(pty=>pty.pty_id===cliPane);assert.ok(cli?.child_pid);
  let originals=[original];
  if(pager){
    await until(async()=>{originals=(await ptys()).filter(pty=>pty.workspace_id===original.workspace_id);return originals.length===2;},'Pager did not split the Runtime workspace');
    for(const [index,pty] of originals.entries())await request(`/v1/domain/ptys/${pty.pty_id}/write`,{data_b64:Buffer.from(`set RIDGE_TEST_PANE=${index+1}\r\n`).toString('base64')});
  }
  await until(async()=>{
    const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    return Buffer.from(output.data_b64??'','base64').toString().includes(`${root}>`);
  },'local TUI canonical snapshot missing');
  const output=join(root,'local-canonical.txt'),text=`echo LOCAL_CANONICAL > "${output.replaceAll('\\','/')}"\r`;
  for(let offset=0;offset<text.length;offset+=8){await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from(text.slice(offset,offset+8)).toString('base64')});await delay(10);}
  await until(async()=>{try{return (await readFile(output,'utf8')).trim()==='LOCAL_CANONICAL';}catch{return false;}},'local TUI input did not reach the canonical terminal');
  if(pager){
    const send=async text=>{for(let offset=0;offset<text.length;offset+=8){await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from(text.slice(offset,offset+8)).toString('base64')});await delay(10);}};
    const first=join(root,'pager-first.txt'),second=join(root,'pager-second.txt');
    await send(`echo %RIDGE_TEST_PANE% > "${first.replaceAll('\\','/')}"\r`);
    await until(async()=>{try{return ['1','2'].includes((await readFile(first,'utf8')).trim());}catch{return false;}},'Pager first pane input failed');
    const firstIndex=Number((await readFile(first,'utf8')).trim())-1;
    await send('\x1b[1;6C');
    await until(async()=>{const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');const text=Buffer.from(output.data_b64??'','base64').toString();return text.lastIndexOf(`${root}>`)>text.lastIndexOf('[2]*shell*')&&text.includes('[2]*shell*');},'Pager switched pane snapshot missing');
    await send(`echo %RIDGE_TEST_PANE% > "${second.replaceAll('\\','/')}"\r`);
    await until(async()=>{try{return ['1','2'].includes((await readFile(second,'utf8')).trim());}catch{return false;}},'Pager second pane input failed');
    assert.notEqual((await readFile(first,'utf8')).trim(),(await readFile(second,'utf8')).trim(),'Pager routed both inputs to the same child');
    await request(`/v1/domain/ptys/${originals[firstIndex].pty_id}/write`,{data_b64:Buffer.from('echo HIDDEN_FIRST_MARKER\r\n').toString('base64')});
    await until(async()=>{const output=await request(`/v1/domain/ptys/${originals[firstIndex].pty_id}?max_bytes=65536`,undefined,'GET');return Buffer.from(output.data_b64??'','base64').toString().includes('HIDDEN_FIRST_MARKER');},'inactive child did not emit the marker');
    await delay(100);
    const hidden=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    assert.equal(Buffer.from(hidden.data_b64??'','base64').toString().includes('HIDDEN_FIRST_MARKER'),false,'inactive pane leaked into Pager');
    await request(`/v1/domain/ptys/${originals[firstIndex].pty_id}/write`,{data_b64:Buffer.from('for /L %i in (1,1,400) do @echo RIDGE_HISTORY_%i\r\n').toString('base64')});
    await until(async()=>{const output=await request(`/v1/domain/ptys/${originals[firstIndex].pty_id}?max_bytes=65536`,undefined,'GET');return Buffer.from(output.data_b64??'','base64').toString().includes('RIDGE_HISTORY_400');},'inactive child history did not finish');
    await send('\x1b[1;6D');
    await until(async()=>{const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');return Buffer.from(output.data_b64??'','base64').toString().includes('RIDGE_HISTORY_400');},'reactivated pane did not recover its canonical snapshot');
    const beforeHistory=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    assert.equal(Buffer.from(beforeHistory.data_b64??'','base64').toString().includes('RIDGE_HISTORY_320'),false,'history fixture already visible in live snapshot');
    for(let page=0;page<4;page++){await send('\x1b[5;6~');await delay(250);}
    await until(async()=>{const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');return Buffer.from(output.data_b64??'','base64').toString().includes('RIDGE_HISTORY_320');},'Pager did not page history beyond its initial canonical snapshot');
    await send('\x1b[6;6~');await delay(250);
    const historyInput=join(root,'pager-history-input.txt');
    await send(`echo HISTORY_INPUT_RECOVERED > "${historyInput.replaceAll('\\','/')}"\r`);
    await until(async()=>{try{return (await readFile(historyInput,'utf8')).trim()==='HISTORY_INPUT_RECOVERED';}catch{return false;}},'typing from historical view did not return to the original live terminal');
    const otherWorkspace=(await request('/v1/domain/workspaces',{name:'PAGER_OTHER_WORKSPACE'})).workspace_id,otherPane=randomUUID();
    await request(`/v1/domain/workspaces/${otherWorkspace}/topology`,{pane_tree:{root:{Leaf:otherPane},panes:{[otherPane]:{id:otherPane,mode:'Terminal',cwd:root}}}},'PUT');
    await request('/v1/domain/ptys',{pty_id:otherPane,workspace_id:otherWorkspace,program:'cmd.exe',args:['/d','/q','/k'],cwd:root,env:{},role:'shell',cols:80,rows:22});
    await request(`/v1/domain/ptys/${otherPane}/write`,{data_b64:Buffer.from('set RIDGE_TEST_PANE=OTHER_WORKSPACE\r\n').toString('base64')});
    const catalog=await request('/v1/domain/workspaces',undefined,'GET'),globalActive=catalog.active;
    const functionKey=number=>number<=4?`\x1b[1;5${'PQRS'[number-1]}`:`\x1b[${[15,17,18,19,20,21,23,24][number-5]};5~`;
    const otherNumber=catalog.workspaces.indexOf(otherWorkspace)+1,ownNumber=catalog.workspaces.indexOf(original.workspace_id)+1;
    assert.ok(otherNumber>0&&otherNumber<=12&&ownNumber>0&&ownNumber<=12);
    await send(functionKey(otherNumber));
    await until(async()=>{const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');const text=Buffer.from(output.data_b64??'','base64').toString();return text.includes('PAGER_OTHER_WORKSPACE')&&text.lastIndexOf(`${root}>`)>text.lastIndexOf('PAGER_OTHER_WORKSPACE');},'Pager workspace shortcut did not restore the other workspace snapshot');
    const workspaceOutput=join(root,'pager-workspace.txt');
    await send(`echo %RIDGE_TEST_PANE% > "${workspaceOutput.replaceAll('\\','/')}"\r`);
    await until(async()=>{try{return (await readFile(workspaceOutput,'utf8')).trim()==='OTHER_WORKSPACE';}catch{return false;}},'Pager workspace shortcut routed input to the wrong workspace');
    assert.equal((await request('/v1/domain/workspaces',undefined,'GET')).active,globalActive,'Pager changed another client global selection');
    await send(functionKey(ownNumber));
    await until(async()=>{const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');const text=Buffer.from(output.data_b64??'','base64').toString();return text.lastIndexOf(`WS:${ownNumber}`)>text.lastIndexOf('PAGER_OTHER_WORKSPACE')&&text.lastIndexOf(`${root}>`)>text.lastIndexOf(`WS:${ownNumber}`);},'Pager could not return to its original workspace');
    const returned=join(root,'pager-returned.txt');
    await send(`echo %RIDGE_TEST_PANE% > "${returned.replaceAll('\\','/')}"\r`);
    await until(async()=>{try{return ['1','2'].includes((await readFile(returned,'utf8')).trim());}catch{return false;}},'Pager original workspace input did not recover');
    originals.push((await ptys()).find(pty=>pty.pty_id===otherPane));
  }
  const workspaces=await request('/v1/domain/workspaces',undefined,'GET');
  assert.ok(workspaces.workspaces.includes(original.workspace_id));
  await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from([29]).toString('base64')});
  await until(async()=>{try{process.kill(cli.child_pid,0);return false;}catch{return true;}},'local TUI did not detach');
  const resumed=(await ptys()).find(pty=>pty.pty_id===original.pty_id);
  assert.equal(resumed.child_pid,original.child_pid);assert.equal(resumed.child_start_instance,original.child_start_instance);
  const after=join(root,'local-after-detach.txt');
  await request(`/v1/domain/ptys/${original.pty_id}/write`,{data_b64:Buffer.from(`echo LOCAL_AFTER_DETACH > "${after.replaceAll('\\','/')}"\r\n`).toString('base64')});
  await until(async()=>{try{return (await readFile(after,'utf8')).trim()==='LOCAL_AFTER_DETACH';}catch{return false;}},'original child did not execute after local TUI detached');
  assert.equal(await command('get_remote_enabled'),false);
  const allAfter=await ptys();for(const before of originals){const after=allAfter.find(pty=>pty.pty_id===before.pty_id);assert.equal(after.child_pid,before.child_pid);assert.equal(after.child_start_instance,before.child_start_instance);}
  const {parseCliStages}=await import('./lib/runtimePerformance.mjs');
  const cliStages=parseCliStages(await readFile(join(root,'ridge.log'),'utf8'),process.env.RIDGE_CLI_PERF_TRACE==='1',cli.child_pid);
  if(cliStages.enabled){
    assert.equal(cliStages.truncated,false);assert.deepEqual(cliStages.errors,[]);
    for(const stage of ['decode','semantic_apply','render','input_queue','socket_send','output_queue','stdout',...(pager?['control_rpc']:[])])assert.ok(cliStages.stages[stage].latencyMs.count>0,`CLI ${stage} diagnostic stage missing`);
  }
  await writeFile(join(root,'result.json'),JSON.stringify({completeChange:false,mode:'local canonical TUI',runtimePid:kernel.pid,childPid:resumed.child_pid,cliStages,
    pager,checks:['local TUI receives canonical snapshot and writes via the shared Host','workspace is visible to other Runtime clients','CLI detach preserves the exact original child','LAN listener stays disabled',...(pager?['two panes share the authoritative Runtime workspace','switch routes input to different original children','inactive output stays hidden and appears after canonical reactivation','history pages beyond the initial snapshot and typing returns to the original live child','workspace shortcuts select other Runtime workspaces without changing global selection','returning to the original workspace restores input and exact original children']:[])]},null,2));
  console.log(`PASS local canonical TUI: ${join(root,'result.json')}`);
}
try{
  await until(async()=>{assert.equal(kernel.exitCode,null);try{endpoint=JSON.parse(await readFile(join(data,'kernel.json'),'utf8'));return endpoint.pid===kernel.pid;}catch{return false;}},'owned Runtime startup failed');
  if(process.env.RIDGE_CLI_TEST_HEADLESS==='1')await headlessScenario();
  else if(process.env.RIDGE_CLI_TEST_DASHBOARD==='1')await dashboardScenario();
  else if(process.env.RIDGE_CLI_TEST_LOCAL==='1')await localScenario();
  else {
  const workspace=(await request('/v1/domain/workspaces',{name:'CLI reconnect owner'})).workspace_id;
  const pane=randomUUID();
  await request(`/v1/domain/workspaces/${workspace}/topology`,{pane_tree:{root:{Leaf:pane},panes:{[pane]:{id:pane,mode:'Terminal',cwd:root}}}},'PUT');
  assert.equal((await request('/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:'cmd.exe',args:['/d','/q','/k'],cwd:root,env:{},role:'shell',cols:80,rows:24})).ok,true);
  const original=(await ptys()).find(pty=>pty.pty_id===pane);assert.ok(original?.child_pid);
  await command('set_remote_enabled',{enabled:true,port:0});const info=await command('get_remote_info');
  const cliPane=randomUUID();
  assert.equal((await request('/v1/domain/ptys',{pty_id:cliPane,program:cliPath,args:['connect',`127.0.0.1:${info.port}`,'--code',info.totpCode],cwd:root,env:{RIDGE_TEST_TLS_DIR:join(root,'tls')},role:'shell',cols:80,rows:24})).ok,true);
  const cliBefore=(await ptys()).find(pty=>pty.pty_id===cliPane);assert.ok(cliBefore?.child_pid);
  await until(async()=>{
    const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    return Buffer.from(output.data_b64??'','base64').toString().includes(`${root}>`);
  },'CLI did not display the original shell');
  async function input(label){
    const output=join(root,`${label}.txt`),text=`echo ${label} > "${output.replaceAll('\\','/')}"\r\n`;
    for(let offset=0;offset<text.length;offset+=8){assert.equal((await request(`/v1/domain/ptys/${cliPane}/write`,{data_b64:Buffer.from(text.slice(offset,offset+8)).toString('base64')})).ok,true);await delay(10);}
    try { await until(async()=>{try{return (await readFile(output,'utf8')).trim()===label;}catch{return false;}},'original child did not execute CLI input'); }
    catch(error){
      const visible=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
      const consoleText=Buffer.from(visible.data_b64??'','base64').toString().replaceAll(info.totpCode,'[fixture credential]');
      await writeFile(join(root,'cli-failure.txt'),consoleText);
      console.error(`CLI input failure evidence: ${join(root,'cli-failure.txt')}`);throw error;
    }
  }
  await input('CLI_BEFORE_LISTENER_INTERRUPTION');
  await command('set_remote_enabled',{enabled:false});
  await command('set_remote_enabled',{enabled:true,port:info.port});
  await until(async()=>(await command('list_remote_sessions')).some(session=>session.deviceId==='ridge-cli'),'same CLI did not reconnect without a new TOTP');
  await until(async()=>{
    const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    const text=Buffer.from(output.data_b64??'','base64').toString();
    const interruption=text.lastIndexOf('Reconnecting...');
    return interruption>=0&&text.slice(interruption).includes(`${root}>`);
  },'CLI did not receive a fresh canonical snapshot after reconnect');
  await input('CLI_AFTER_LISTENER_INTERRUPTION');
  const after=await ptys(),cliAfter=after.find(pty=>pty.pty_id===cliPane),originalAfter=after.find(pty=>pty.pty_id===pane);
  for(const [before,resumed] of [[original,originalAfter],[cliBefore,cliAfter]]){
    assert.equal(resumed.child_pid,before.child_pid);assert.equal(resumed.child_start_instance,before.child_start_instance);
  }
  const session=(await command('list_remote_sessions')).find(session=>session.deviceId==='ridge-cli');assert.ok(session);
  assert.equal(await command('disconnect_session',{id:session.id}),true);
  await until(async()=>{
    const output=await request(`/v1/domain/ptys/${cliPane}?max_bytes=65536`,undefined,'GET');
    return Buffer.from(output.data_b64??'','base64').toString().includes('LAN reconnect exhausted');
  },'revoked CLI credential did not stop bounded reconnect');
  assert.equal((await command('list_remote_sessions')).some(session=>session.deviceId==='ridge-cli'),false);
  await writeFile(join(root,'result.json'),JSON.stringify({completeChange:false,checks:['same interactive CLI resumes authenticated LAN session without new TOTP','input executes in same original child before and after interruption','revoked credential exhausts bounded reconnect without reauthenticating'],runtimePid:kernel.pid,cliPid:cliAfter.child_pid,childPid:originalAfter.child_pid},null,2));
  console.log(`PASS CLI LAN reconnect: ${join(root,'result.json')}`);
  }
}catch(error){
  if(endpoint){
    for(const pty of await ptys().catch(()=>[])){
      const output=await request(`/v1/domain/ptys/${pty.pty_id}?max_bytes=65536`,undefined,'GET').catch(()=>null);
      if(output)await writeFile(join(root,`${pty.pty_id}.console.txt`),Buffer.from(output.data_b64??'','base64'));
    }
  }
  throw error;
}finally{
  if(endpoint)await request('/v1/shutdown',{}).catch(()=>{});
  if(kernel.exitCode===null){await Promise.race([once(kernel,'exit'),delay(6000)]);if(kernel.exitCode===null)kernel.kill();}
}
