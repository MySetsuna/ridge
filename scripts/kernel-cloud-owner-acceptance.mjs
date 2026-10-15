// Owned relay/API fixture; real browser RTC, E2EE, Runtime commands and PTY.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { randomUUID } from 'node:crypto';
import http from 'node:http';
import { WebSocketServer, WebSocket } from 'ws';
import { chromium } from '@playwright/test';
import { createRequire } from 'node:module';
const {build}=createRequire(import.meta.resolve('vite'))('esbuild');

const root=await mkdtemp(join(tmpdir(),'ridge-cloud-owner-'));
let bundle='';let host;let sharedWorkspace;const peers=new Map();let cid=0;
const server=http.createServer((request,response)=>{
  response.setHeader('access-control-allow-origin','*');response.setHeader('access-control-allow-headers','authorization,content-type');
  if(request.method==='OPTIONS'){response.writeHead(204);response.end();return;}
  if(request.url==='/client.js'){response.setHeader('content-type','text/javascript');response.end(bundle);return;}
  if(request.url==='/api/v1/ice-servers'){response.setHeader('content-type','application/json');response.end(JSON.stringify({ok:true,data:{iceServers:[{urls:'stun:127.0.0.1:9'}]}}));return;}
  if(request.url==='/api/v1/workspace-shares/verify-access'){
    let body='';request.on('data',bytes=>body+=bytes);request.on('end',()=>{
      if(request.headers.authorization!=='Bearer owned-device'||JSON.parse(body).token!=='owned-share'){response.writeHead(403);response.end('{}');return;}
      response.setHeader('content-type','application/json');response.end(JSON.stringify({ok:true,data:{grantId:'owned-grant',granteeUserId:'guest',ownerUserId:'owner',deviceName:'desktop',workspaceId:sharedWorkspace,role:'operator',delegable:false}}));
    });return;
  }
  response.setHeader('content-type','text/html');response.end('<script type="module" src="/client.js"></script>');
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));const port=server.address().port;
const relay=new WebSocketServer({server});
const send=(socket,message)=>{if(socket?.readyState===1)socket.send(JSON.stringify(message));};
relay.on('connection',(socket,request)=>{
  const url=new URL(request.url,'http://localhost');const role=url.searchParams.get('role');
  if(role==='host'?url.searchParams.get('token')!=='owned-device':!['owned-controller','owned-share'].includes(url.searchParams.get('token'))){socket.close();return;}
  let id;
  if(role==='host'){host=socket;send(socket,{t:'welcome',room:'owned',role:'host',peerPresent:peers.size>0});for(const peer of peers.values())send(peer,{t:'peer-join',role:'host'});}
  else {id='owned-'+(++cid);peers.set(id,socket);if(url.searchParams.get('token')==='owned-share')send(host,{t:'workspace-scope',cid:id,token:'owned-share'});send(socket,{t:'welcome',room:'owned',role:'controller',cid:id,peerPresent:Boolean(host)});send(host,{t:'peer-join',role:'controller',cid:id});}
  socket.on('message',bytes=>{
    let message;try{message=JSON.parse(bytes);}catch{return;}
    if(role==='controller'){if(['offer','ice','e2ee-pubkey'].includes(message.t))send(host,{...message,cid:id});}
    else if(message.t==='kick')peers.get(message.cid)?.close();
    else if(['answer','ice','e2ee-pubkey'].includes(message.t))send(peers.get(message.cid),message);
  });
  socket.on('close',()=>{if(role==='host'){if(host===socket)host=undefined;for(const peer of peers.values())send(peer,{t:'peer-leave',role:'host'});}else{peers.delete(id);send(host,{t:'peer-leave',role:'controller',cid:id});}});
});
const built=await build({stdin:{contents:`
  import {ControllerCloudProvider} from './packages/remote/src/shared/cloud/controllerCloudProvider';
  import {RemoteConnection} from './packages/remote/src/shared/transport/wsRemote';
  import {WsDataProvider} from './src/lib/transport/ws';
  import {encodeJsonFrame,encodeControlFrame} from './packages/remote/src/shared/transport/cloudMux';
  import {computeBindTag,bytesToBase64} from './packages/remote/src/shared/cloud/e2ee';
  window.fixture={async connect(code,shared=false){
    let provider;let sequence=0;const pending=new Map();let state='disconnected';let terminalFrames=0;let terminalBytes=[];const failures=[];const events=[];
    let authenticated;const authentication=new Promise((done,fail)=>{authenticated={done,fail};});
    provider=new ControllerCloudProvider({userToken:shared?'owned-share':'owned-controller',username:'owner',baseDomain:'localhost:${port}'},{
      onState(value){state=value;if(value==='connected'){const transcript=provider.getBindTranscript();if(!transcript){authenticated.fail(new Error('signed host binding required'));return;}if(shared){authenticated.done();return;}provider.sendFrame(encodeControlFrame({t:'totp-bind',tag:bytesToBase64(computeBindTag(code,transcript))}));}},
      onError(message){failures.push(message);},
      onFrame(bytes){if(bytes[0]===0x13){terminalFrames++;terminalBytes=Array.from(bytes);return;}if(bytes[0]===0x11||bytes[0]===0x12){const value=JSON.parse(new TextDecoder().decode(bytes.subarray(1)));if(value.type==='event')events.push(value);if(value.t==='totp-result'){value.ok?authenticated.done():authenticated.fail(new Error('TOTP failed'));}if(pending.has(value.id)){pending.get(value.id)(value);pending.delete(value.id);}}}
    });
    provider.connect('desktop');await Promise.race([authentication,new Promise((_,fail)=>setTimeout(()=>fail(new Error('RTC authentication deadline: '+state+' '+failures.join(';'))),20000))]);
    return {get state(){return state;},get frames(){return terminalFrames;},get terminalBytes(){return terminalBytes;},get failures(){return failures;},get events(){return events;},provider,
      rpc(method,params={}){const id=++sequence;return new Promise((done,fail)=>{const timer=setTimeout(()=>{pending.delete(id);fail(new Error('RPC deadline: '+method));},10000);pending.set(id,value=>{clearTimeout(timer);done(value);});provider.sendFrame(encodeJsonFrame({jsonrpc:'2.0',id,method,params}));});},
      subscribe(pane){provider.sendFrame(encodeJsonFrame({jsonrpc:'2.0',method:'subscribe-pane',params:pane}));},close(){provider.disconnect();}
    };
  },async project(grant,hostId){
    const url=new URL(grant.wsUrl);url.pathname='/v1/local/host-link/'+encodeURIComponent(hostId);url.searchParams.set('deviceId',grant.deviceId);
    const connection=new RemoteConnection(()=>new WebSocket(url,['ridge-client','auth-'+grant.token]));
    await new Promise((done,fail)=>{const timer=setTimeout(()=>fail(new Error('real projection connection deadline')),15000);const stop=connection.onStateChange(state=>{if(state==='connected'){clearTimeout(timer);stop();done();}if(state==='error'){clearTimeout(timer);stop();fail(new Error('real projection failed'));}});connection.connect('127.0.0.1',Number(url.port),grant.token,'token',false);});
    connection.fixtureProvider=new WsDataProvider(connection);return connection;
  }};
`,resolveDir:resolve('.')},bundle:true,write:false,format:'esm',platform:'browser',alias:{'@ridge/remote':resolve('packages/remote/src/index.ts')},define:{'import.meta.env':JSON.stringify({RIDGE_CLOUD_BASE_DOMAIN:`localhost:${port}`,RIDGE_CLOUD_DEV_PLAINTEXT:'1'})}});
bundle=built.outputFiles[0].text;
const child=spawn(resolve('target/debug/ridge-kernel'+(process.platform==='win32'?'.exe':'')),[],{env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_TLS_DIR:join(root,'tls'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'},stdio:'ignore',windowsHide:true});
let endpoint;let browser;
const delay=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message){const deadline=Date.now()+15000;while(Date.now()<deadline){if(await check())return;await delay(50);}throw new Error(message);}
async function request(path,body,method='POST'){
  const result=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
  assert.equal(result.status,200);return result.json();
}
const connectionId=randomUUID();
async function command(method,args={}){const reply=await request('/v1/domain/commands',{method,args,native:true,roots:[],connectionId});assert.equal(reply.ok,true,`${method}: ${reply.error?.message??''}`);return reply.value;}
try {
  await until(async()=>{if(child.exitCode!==null||child.signalCode!==null)throw new Error('owned Runtime exited');try{endpoint=JSON.parse(await readFile(join(root,'kernel.json'),'utf8'));return endpoint.pid===child.pid;}catch{return false;}},'Runtime startup deadline');
  const workspace=(await request('/v1/domain/workspaces',{name:'Owned Cloud'})).workspace_id;sharedWorkspace=workspace;const pane=randomUUID();
  assert.equal((await request(`/v1/domain/workspaces/${workspace}/topology`,{pane_tree:{root:{Leaf:pane},panes:{[pane]:{id:pane,mode:'Terminal',cwd:root}}}},'PUT')).ok,true);
  assert.equal((await request('/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:process.platform==='win32'?'cmd.exe':'/bin/sh',args:process.platform==='win32'?['/d','/q','/k']:[],cwd:root,env:{},role:'shell',cols:80,rows:24})).ok,true);
  await command('cloud_host_online',{deviceToken:'owned-device',deviceName:'desktop',username:'owner',baseDomain:`localhost:${port}`,plaintext:true});
  await until(async()=>(await command('get_cloud_host_state')).state==='online','Cloud listener did not reach relay welcome');
  await command('set_remote_enabled',{enabled:true,port:0});
  const info=await command('get_remote_info');
  const ownHost=await command('connect_host',{kind:'remote',addr:`https://127.0.0.1:${info.port}`,token:info.totpCode});
  const foreign=await command('attach_host_session',{hostId:ownHost,sessionId:pane,remoteWorkspaceId:workspace,workspaceId:workspace});
  const code=(await command('get_remote_info')).totpCode;
  browser=await chromium.launch({headless:true});
  const first=await browser.newPage();await first.goto(`http://127.0.0.1:${port}`);await first.waitForFunction(()=>window.fixture);
  await first.evaluate(async code=>{window.client=await window.fixture.connect(code);},code);
  const second=await browser.newPage();await second.goto(`http://127.0.0.1:${port}`);await second.waitForFunction(()=>window.fixture);
  await second.evaluate(async code=>{window.client=await window.fixture.connect(code);},code);
  const denied=await second.evaluate(()=>window.client.rpc('get_remote_info'));assert.ok(denied.error);
  const listed=await second.evaluate(()=>window.client.rpc('list_workspaces'));assert.ok(listed.result.some(entry=>entry.id===workspace));
  await second.evaluate(args=>window.client.subscribe(args),{workspaceId:workspace,paneId:pane,activationId:17});
  await second.waitForFunction(()=>window.client.frames>0);
  const snapshot=await second.evaluate(args=>window.client.rpc('get_terminal_snapshot',args),{workspaceId:workspace,paneId:pane,activationId:17});assert.equal(typeof snapshot.result,'string');
  await first.evaluate(args=>window.client.subscribe(args),{workspaceId:workspace,paneId:foreign,activationId:21});
  await first.waitForFunction(()=>window.client.frames>0);
  const encoded=await first.evaluate(args=>window.client.rpc('get_terminal_snapshot',args),{workspaceId:workspace,paneId:foreign,activationId:21});
  assert.equal(encoded.error,undefined,JSON.stringify(encoded.error));
  // Read only the generated Postcard frame header; Rust integration tests decode the full body.
  const bytes=Buffer.from(encoded.result,'base64');let offset=1;
  const integer=()=>{let value=0,factor=1;for(let count=0;count<8;count++){const byte=bytes[offset++];assert.notEqual(byte,undefined);value+=(byte&127)*factor;if(!(byte&128))return value;factor*=128;}throw new Error('invalid terminal header integer');};
  const string=()=>{const size=integer();const value=bytes.subarray(offset,offset+size).toString('utf8');offset+=size;return value;};
  assert.equal(bytes[0],0x13);integer();assert.equal(string(),workspace);assert.equal(string(),foreign);const epoch=string();assert.equal(integer(),21);
  const replyPath=join(root,'cloud-foreign-reply.txt');const dataForeign=`echo CLOUD_FOREIGN_SHARED_REPLY >> "${replyPath.replaceAll('\\','/')}"\r\n`;
  const input={pane:{workspaceId:workspace,paneId:foreign},terminalEpoch:epoch,activationId:21,sourceId:'owned-cloud-foreign',sequence:1,bytes:Array.from(Buffer.from(dataForeign))};
  const applied=await first.evaluate(input=>window.client.rpc('write_terminal_input',{input}),input);assert.equal(applied.result?.alreadyApplied,false,JSON.stringify(applied));
  const replay=await first.evaluate(input=>window.client.rpc('write_terminal_input',{input}),input);assert.equal(replay.result?.alreadyApplied,true,JSON.stringify(replay));
  await until(async()=>{try{return (await readFile(replyPath,'utf8')).includes('CLOUD_FOREIGN_SHARED_REPLY');}catch{return false;}},'Cloud foreign input did not reach original child');
  assert.equal((await readFile(replyPath,'utf8')).split('CLOUD_FOREIGN_SHARED_REPLY').length-1,1);
  const status=await command('get_cloud_host_state');assert.equal(status.sessions.length,2);assert.ok(status.sessions.every(peer=>peer.connectedAt>0));
  const scoped=await browser.newPage();await scoped.goto(`http://127.0.0.1:${port}`);await scoped.waitForFunction(()=>window.fixture);
  await scoped.evaluate(async()=>{window.client=await window.fixture.connect('',true);});
  const scopedRows=await scoped.evaluate(()=>window.client.rpc('list_workspaces'));assert.deepEqual(scopedRows.result.map(row=>row.id),[workspace]);
  const escaped=await scoped.evaluate(()=>window.client.rpc('create_workspace',{name:'must not create'}));assert.ok(escaped.error);
  for(const method of ['list_native_sessions','new_headless_session','terminate_native_session','summon_native_session']) {
    const denied=await scoped.evaluate(method=>window.client.rpc(method,{socket:'headless',target:'must-not-access',name:'must-not-create'}),method);
    assert.ok(denied.error,`workspace grant must not access host-global ${method}`);
  }
  const hop=await scoped.evaluate(args=>window.client.rpc('get_terminal_snapshot',args),{workspaceId:workspace,paneId:foreign,activationId:21});assert.ok(hop.error,'workspace grant must deny Foreign second hop');
  const scopeAlive=await scoped.evaluate(()=>window.client.rpc('get_active_workspace_id'));assert.equal(scopeAlive.result,workspace);
  const wrongScope=await scoped.evaluate(()=>window.client.rpc('read_file',{path:'C:/outside-granted-root/private.txt'}));assert.ok(wrongScope.error);
  const other=(await request('/v1/domain/workspaces',{name:'Outside shared event scope'})).workspace_id;
  const otherPane=(await request(`/v1/domain/workspaces/${other}`,undefined,'GET')).panes[0];
  await command('toggle_mode',{workspaceId:other,paneId:otherPane,mode:'Terminal'});
  await command('toggle_mode',{workspaceId:workspace,paneId:pane,mode:'Terminal'});
  await scoped.waitForFunction(workspace=>window.client.events.some(event=>event.name==='pane-mode-changed'&&event.payload.workspaceId===workspace),workspace);
  const scopedEvents=await scoped.evaluate(()=>window.client.events);assert.ok(!scopedEvents.some(event=>event.payload?.workspaceId===other),'workspace grant received another workspace event');
  await scoped.close();
  await until(async()=>(await command('get_cloud_host_state')).sessions.length===2,'scoped peer did not release');
  await first.close();await until(async()=>(await command('get_cloud_host_state')).sessions.length===1,'closing one controller did not release its peer');
  const script=join(root,process.platform==='win32'?'reply.cmd':'reply.sh');await writeFile(script,process.platform==='win32'?'@echo off\r\necho CLOUD_RUNTIME_OWNER_REPLY\r\n':"printf 'CLOUD_RUNTIME_OWNER_REPLY\\n'\n");
  const data=process.platform==='win32'?`call "${script}"\r\n`:`sh '${script.replaceAll("'","'\\''")}'\n`;
  const ack=await second.evaluate(args=>window.client.rpc('write_to_pty',args),{workspaceId:workspace,paneId:pane,data});assert.equal(ack.error,undefined);
  await until(async()=>{const output=await request(`/v1/domain/ptys/${pane}`,undefined,'GET');return Buffer.from(output.data_b64,'base64').toString().includes('CLOUD_RUNTIME_OWNER_REPLY');},'RTC input did not reach the real child');
  await second.close();await until(async()=>(await command('get_cloud_host_state')).sessions.length===0,'browser peers did not release');
  const config={userToken:'owned-controller',username:'owner',deviceName:'desktop',baseDomain:`localhost:${port}`,plaintext:true,code:(await command('get_remote_info')).totpCode,legacyFingerprint:'1111-2222-3333-4444'};
  await command('initialize_cloud_controller_identity',{legacySeed:Array(32).fill(9)});
  const cloudId=await command('connect_host',{kind:'remote',label:'Kernel controller',addr:'desktop',cloud:config});
  const cloudRecord=(await command('host_list_snapshot')).find(record=>record.id===cloudId);
  assert.equal(cloudRecord?.status,'connected',cloudRecord?.detail);assert.equal(cloudRecord.runtimeManaged,true);
  assert.ok(cloudRecord.detail.includes('设备指纹变化'),'legacy pin change must remain visible with warn-only policy');
  const pinsPath=join(root,'cloud-device-fingerprints.json');const pinsBefore=await readFile(pinsPath,'utf8');
  assert.equal(JSON.parse(pinsBefore).version,1);assert.equal(JSON.parse(pinsBefore).pins['desktop-owner'],config.legacyFingerprint);
  assert.ok((await command('list_host_sessions',{hostId:cloudId})).some(session=>session.id===pane));
  const before=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);
  await command('disconnect_host',{hostId:cloudId});
  await command('connect_host',{kind:'remote',label:'Kernel controller',addr:'desktop',cloud:{...config,code:'000000',legacyFingerprint:'AAAA-BBBB-CCCC-DDDD'}});
  assert.equal((await command('host_list_snapshot')).find(record=>record.id===cloudId)?.status,'connected','stable Runtime identity must resume its grant without another TOTP');
  assert.equal(await readFile(pinsPath,'utf8'),pinsBefore,'reconnect must retain the original canonical pin');
  await until(async()=>peers.size===1,'previous Cloud associations did not release');const managedCid=[...peers.keys()][0];
  const grant=await request('/v1/local/clients',{deviceId:'owned-cloud-projection',origin:`http://127.0.0.1:${port}`});
  const actual=await browser.newPage();await actual.goto(`http://127.0.0.1:${port}`);await actual.waitForFunction(()=>window.fixture);
  await actual.evaluate(async({grant,cloudId,workspace})=>{window.fixtureWorkspace=workspace;window.realProjection=await window.fixture.project(grant,cloudId);window.projectedEvents=[];window.realProjection.onMessage(event=>{if(event.type==='event')window.projectedEvents.push(event);});},{grant,cloudId,workspace});
  const realRows=await actual.evaluate(()=>window.realProjection.listWorkspaces());assert.ok(realRows.workspaces.some(row=>row.id===workspace));
  const realPanes=await actual.evaluate(id=>window.realProjection.listWorkspacePanes(id),workspace);assert.ok(realPanes.some(row=>row.id===pane));
  const tree=await actual.evaluate(root=>window.realProjection.fixtureProvider.getFileTree(root,1),root);assert.ok(tree,'real data-request frame must reach shared filesystem service');
  const deniedProjection=await actual.evaluate(async()=>{try{await window.realProjection.fixtureProvider.pathExists('C:/outside-granted-root/private.txt');return false;}catch(error){return String(error);}});assert.match(String(deniedProjection),/sandbox|root|denied|allowed/i,'native projection must report upstream sandbox rejection, not a timeout or missing API');
  await command('rename_workspace',{workspaceId:workspace,name:'Cloud event from common Runtime'});
  await actual.waitForFunction(()=>window.projectedEvents.some(event=>event.name==='workspace-list-changed'&&!event.payload?.initial));
  const eventRows=await actual.evaluate(()=>window.projectedEvents);assert.ok(eventRows.every(event=>event.runtimeEpoch&&Number.isSafeInteger(event.sequence)));
  send(peers.get(managedCid),{t:'peer-leave',role:'host'});
  await until(async()=>{const record=(await command('host_list_snapshot')).find(record=>record.id===cloudId);return record?.status==='connected'&&!peers.has(managedCid);},'Runtime did not restore the severed Cloud association');
  await until(async()=>{try{return await actual.evaluate(()=>window.realProjection.state()==='connected'&&window.realProjection.listWorkspaces().then(rows=>rows.workspaces.some(row=>row.id===window.fixtureWorkspace)));}catch{return false;}},'browser did not restore the Runtime projection');
  await actual.evaluate(()=>window.realProjection.disconnect());await actual.close();
  const localUrl=new URL(grant.wsUrl);localUrl.pathname=`/v1/local/host-link/${encodeURIComponent(cloudId)}`;localUrl.searchParams.set('deviceId',grant.deviceId);
  const projection=new WebSocket(localUrl,['ridge-client',`auth-${grant.token}`],{headers:{Origin:grant.origin}});
  const pending=new Map();let sequence=0;let canonical=0;
  projection.on('message',(bytes,binary)=>{if(binary){if(bytes[0]===0x13)canonical++;return;}const message=JSON.parse(bytes.toString());pending.get(message.id)?.(message);pending.delete(message.id);});
  await once(projection,'open');
  const rpc=(method,params={})=>new Promise((done,fail)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);fail(new Error(`Kernel Cloud RPC deadline: ${method}`));},10000);pending.set(id,message=>{clearTimeout(timer);message.error?fail(new Error(JSON.stringify(message.error))):done(message.result);});projection.send(JSON.stringify({jsonrpc:'2.0',id,method,params}));});
  assert.ok((await rpc('list_workspaces')).some(row=>row.id===workspace));
  projection.send(JSON.stringify({jsonrpc:'2.0',method:'subscribe-pane',params:{workspaceId:workspace,paneId:pane,activationId:29}}));
  await until(async()=>canonical>0,'Kernel Cloud projection omitted canonical terminal frames');
  const runtimeReply=join(root,'kernel-controller-reply.txt');await rpc('write_to_pty',{workspaceId:workspace,paneId:pane,data:`echo KERNEL_CLOUD_CONTROLLER_REPLY > "${runtimeReply.replaceAll('\\','/')}"\r\n`});
  await until(async()=>{try{return (await readFile(runtimeReply,'utf8')).includes('KERNEL_CLOUD_CONTROLLER_REPLY');}catch{return false;}},'Kernel-owned RTC did not reach original child');
  const after=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===pane);assert.equal(after.child_pid,before.child_pid);assert.equal(after.child_start_instance,before.child_start_instance);
  assert.ok(!(await readFile(join(root,'remote-hosts.json'),'utf8')).includes('owned-controller'),'Cloud user credential leaked into topology persistence');
  projection.close();await command('disconnect_host',{hostId:cloudId});
  await writeFile(join(root,'cloud-pins-before-failure.json'),pinsBefore);await writeFile(pinsPath,'{interrupted migration');
  await command('connect_host',{kind:'remote',label:'Kernel controller',addr:'desktop',cloud:config});
  const failed=(await command('host_list_snapshot')).find(record=>record.id===cloudId);assert.equal(failed.status,'error');assert.ok(failed.detail.includes('original preserved'));
  assert.equal(await readFile(pinsPath,'utf8'),'{interrupted migration','corrupt pin store must not reset to empty trust');
  await command('cloud_host_offline');assert.equal((await command('get_cloud_host_state')).state,'offline');
  await until(async()=>host===undefined,'explicit Cloud offline left relay connection alive');
  console.log('PASS: Kernel owns both Cloud roles; real browser/Rust RTC, signed E2EE, TOTP/trust resumption, loopback projection, canonical stream, same child reply, credential-free topology and explicit offline.');
} finally {
  await browser?.close();
  if(endpoint){try{await request('/v1/shutdown',{});}catch{}}
  if(child.exitCode===null&&child.signalCode===null){await Promise.race([once(child,'exit'),delay(6000)]);if(child.exitCode===null&&child.signalCode===null){const exited=once(child,'exit');child.kill();await exited;}}
  for(const socket of relay.clients)socket.terminate();
  await new Promise(done=>relay.close(done));await new Promise(done=>server.close(done));
}
