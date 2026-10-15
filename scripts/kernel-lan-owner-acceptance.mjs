// Real Runtime + TLS LAN + presentation detach; all credentials remain in an owned temp root.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { randomUUID } from 'node:crypto';
import https from 'node:https';
import { once } from 'node:events';
import WebSocket from 'ws';

const root=await mkdtemp(join(tmpdir(),'ridge-lan-owner-'));
await mkdir(join(root,'tls'));
const blacklist=join(root,'remote-blacklist.json');
const original=JSON.stringify([{id:'legacy',device_id:'legacy-blocked',ip:null,label:'legacy',added_at:1,metadata:{note:'retain newer fields'}}]);
await writeFile(blacklist,original);
const child=spawn(resolve('target/debug/ridge-kernel'+(process.platform==='win32'?'.exe':'')),[],{
  env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,
    RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_TLS_DIR:join(root,'tls'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'},
  stdio:'ignore',windowsHide:true,
});
let cli;
let endpoint;
const sockets=[];
const delay=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message) {
  const deadline=Date.now()+12000;
  while(Date.now()<deadline) {if(await check())return;await delay(50);}
  throw new Error(message);
}
async function privateRequest(path,body) {
  const response=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{
    method:'POST',headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:JSON.stringify(body),
    signal:AbortSignal.timeout(12000),
  });
  assert.equal(response.status,200);
  return response.json();
}
const connectionId=randomUUID();
async function command(method,args={}) {
  const reply=await privateRequest('/v1/domain/commands',{method,args,native:true,roots:[],connectionId});
  assert.equal(reply.ok,true,`Runtime command failed: ${method}`);
  return reply.value;
}
function secureRequest(port,ca,path,body) {
  return new Promise((done,fail)=>{
    const request=https.request({hostname:'127.0.0.1',port,path,ca,method:body?'POST':'GET',
      headers:body?{'content-type':'application/x-www-form-urlencoded','content-length':Buffer.byteLength(body)}:{},timeout:5000},response=>{
      const chunks=[];response.on('data',chunk=>chunks.push(chunk));
      response.on('end',()=>{try{done({status:response.statusCode,value:JSON.parse(Buffer.concat(chunks))});}catch(error){fail(error);}});
    });
    request.on('error',fail);request.on('timeout',()=>request.destroy(new Error('TLS request deadline')));request.end(body);
  });
}
async function connect(url,options={},protocols=[]) {
  const socket=new WebSocket(url,protocols,options);sockets.push(socket);
  const pending=new Map();let id=0;
  socket.on('message',(data,binary)=>{
    if(binary)return;
    const message=JSON.parse(data.toString());
    if(pending.has(message.id)){pending.get(message.id)(message);pending.delete(message.id);}
  });
  await once(socket,'open');
  socket.rpc=(method,params={})=>new Promise((done,fail)=>{
    const current=++id;const timer=setTimeout(()=>{pending.delete(current);fail(new Error(`LAN RPC deadline: ${method}`));},6000);
    pending.set(current,message=>{clearTimeout(timer);done(message);});
    socket.send(JSON.stringify({jsonrpc:'2.0',id:current,method,params}));
  });
  return socket;
}
try {
  await until(async()=>{
    if(child.exitCode!==null)throw new Error('owned Runtime exited during startup');
    try{endpoint=JSON.parse(await readFile(join(root,'kernel.json'),'utf8'));return endpoint.pid===child.pid;}catch{return false;}
  },'Runtime discovery deadline');
  assert.equal(await command('get_remote_enabled'),false);
  assert.equal((await command('list_blacklist'))[0].id,'legacy');
  assert.equal(await readFile(blacklist,'utf8'),original,'read must preserve legacy blacklist bytes');
  await command('set_remote_enabled',{enabled:true,port:0});
  const info=await command('get_remote_info');
  assert.equal(info.ready,true);assert.equal(info.tls,true);assert.ok(info.port>0);
  await command('set_remote_enabled',{enabled:true,port:0});
  assert.equal((await command('get_remote_info')).port,info.port,'idempotent enable must reuse listener');
  cli=spawn(resolve(process.env.RIDGE_CLI_TEST_BINARY || 'target/070-cli-controller/debug/ridge'+(process.platform==='win32'?'.exe':'')),['host','--port',String(info.port)],{
    env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,
      RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_REMOTE_HOST_REGISTRY:join(root,'host.json')},stdio:'ignore',windowsHide:true,
  });
  await until(async()=>{
    if(cli.exitCode!==null)throw new Error('CLI Host exited during shared listener attach');
    try{const registry=JSON.parse(await readFile(join(root,'host.json'),'utf8'));return registry.pid===child.pid&&registry.port===info.port;}catch{return false;}
  },'CLI did not reuse Runtime LAN owner');
  const cliExit=once(cli,'exit');cli.kill();await cliExit;
  assert.equal((await command('get_remote_info')).ready,true,'killing CLI launcher must preserve Runtime listener');
  const ca=await readFile(join(root,'tls','ca.pem'));
  const publicInfo=await secureRequest(info.port,ca,'/info');
  assert.equal(publicInfo.status,200);assert.equal(publicInfo.value.ready,true);
  assert.equal(publicInfo.value.totpCode,undefined);assert.equal(publicInfo.value.otpauthUri,undefined);
  const device=randomUUID();
  const authenticate=async(name=device)=>(await secureRequest(info.port,ca,'/verify',`code=${(await command('get_remote_info')).totpCode}&device=${name}`)).value;
  assert.equal((await authenticate('legacy-blocked')).success,false);
  const authenticated=await authenticate();assert.equal(authenticated.success,true);
  const remote=await connect(`wss://127.0.0.1:${info.port}/ws?token=${authenticated.token}&device=${device}`,{ca});
  const capabilities=['pane','invoke','fs','git','search','workspace','theme','teammate'];
  const protocolSource=await readFile('packages/ridge-term/src/terminal_protocol_generated.rs','utf8');
  const terminalProtocolVersion=Number(protocolSource.match(/pub const PROTOCOL_VERSION:\s*\w+\s*=\s*(\d+)/)?.[1]);
  assert.ok(Number.isSafeInteger(terminalProtocolVersion),'canonical terminal protocol version missing');
  const helloParams={protocolVersion:1,terminalProtocolVersion,capabilities};
  const hello=await remote.rpc('$/hello',helloParams);
  assert.deepEqual(hello.result.capabilities,capabilities,'LAN must advertise full shared Runtime capabilities');
  const forbidden=await remote.rpc('get_remote_info');assert.ok(forbidden.error);assert.equal(forbidden.result,undefined);
  const grant=await privateRequest('/v1/local/clients',{deviceId:randomUUID(),origin:'http://127.0.0.1:5173'});
  const presentation=await connect(`${grant.wsUrl}?deviceId=${grant.deviceId}`,{headers:{origin:grant.origin}},['ridge-client',`auth-${grant.token}`]);
  const localHello=await presentation.rpc('$/hello',helloParams);
  assert.deepEqual(localHello.result,hello.result,'local presentation and LAN must negotiate the same capability contract');
  presentation.close();await once(presentation,'close');
  assert.equal((await secureRequest(info.port,ca,'/info')).value.ready,true,'LAN survives presentation detach');
  assert.ok((await remote.rpc('list_workspaces')).error===undefined);
  let session;
  await until(async()=>{session=(await command('list_remote_sessions')).find(session=>session.deviceId===device);return Boolean(session);},'remote connection registration deadline');
  const closed=once(remote,'close');
  assert.equal(await command('add_to_blacklist',{id:session.id}),true);
  await closed;
  assert.equal((await authenticate()).success,false,'blacklisted device must not regain authentication');
  const entries=await command('list_blacklist');assert.ok(entries.some(entry=>entry.id==='legacy'));
  assert.equal(JSON.parse(await readFile(blacklist,'utf8')).find(entry=>entry.id==='legacy').metadata.note,'retain newer fields');
  const added=entries.find(entry=>entry.device_id===device);assert.ok(added);
  assert.equal(await command('remove_from_blacklist',{id:added.id}),true);
  const again=await authenticate();assert.equal(again.success,true);
  const second=await connect(`wss://127.0.0.1:${info.port}/ws?token=${again.token}&device=${device}`,{ca});
  await until(async()=>{session=(await command('list_remote_sessions')).find(session=>session.deviceId===device);return Boolean(session);},'replacement connection deadline');
  const kicked=once(second,'close');assert.equal(await command('disconnect_session',{id:session.id}),true);await kicked;
  assert.equal((await secureRequest(info.port,ca,`/session?token=${again.token}`)).value.valid,false);
  await command('set_remote_enabled',{enabled:false});
  assert.equal(await command('get_remote_enabled'),false);assert.equal((await command('get_remote_info')).port,0);
  await until(async()=>{try{await secureRequest(info.port,ca,'/info');return false;}catch{return true;}},'disabled LAN listener remained reachable');
  console.log('PASS: Runtime-owned TLS LAN, legacy blacklist, client detach, access denial, blacklist and kick, listener stop.');
} finally {
  if(cli&&cli.exitCode===null&&cli.signalCode===null){const exited=once(cli,'exit');cli.kill();await exited;}
  for(const socket of sockets)socket.terminate();
  if(endpoint) {try{await privateRequest('/v1/shutdown',{});}catch{}}
  if(child.exitCode===null&&child.signalCode===null) {
    await Promise.race([once(child,'exit'),delay(6000)]);
    if(child.exitCode===null&&child.signalCode===null){const exited=once(child,'exit');child.kill();await exited;}
  }
}
