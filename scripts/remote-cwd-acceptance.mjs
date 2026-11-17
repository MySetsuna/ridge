// Production Remote, real isolated Kernel/PTYs; Cloud account/signaling are loopback fixtures.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHash,randomUUID,X509Certificate} from 'node:crypto';
import {once} from 'node:events';
import {createReadStream} from 'node:fs';
import {copyFile,mkdir,mkdtemp,open,readFile,stat,writeFile} from 'node:fs/promises';
import http from 'node:http';
import {tmpdir} from 'node:os';
import {basename,extname,join,resolve,sep} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {chromium,expect} from '@playwright/test';
import {WebSocketServer} from 'ws';
const evidenceDir=resolve(process.env.RIDGE_CWD_EVIDENCE_DIR ?? 'artifacts/remote-directory-commune');
await mkdir(evidenceDir,{recursive:true});

async function lan(){
const root=await mkdtemp(join(tmpdir(),'ridge-mobile-cwd-'));
const detachedProbe=true;
const assets=process.env.RIDGE_CWD_PROBE_ASSETS??join(root,'assets'), shellA=join(root,'cwd-one'), shellB=join(root,'cwd-two'), moved=join(shellA,'cwd-live');
const evidence={scope:'Production mobile bundle, real cmd.exe PTYs, localhost LAN HTTPS/TOTP/WSS with pinned TLS certificate',checks:[]};
let kernel,browser,endpoint,page,phase='build';
const creationReplies=[];
async function api(method,route,body){
  const response=await fetch('http://127.0.0.1:'+endpoint.port+route,{method,
    headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},
    body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(8000)});
  assert.equal(response.status,200,route);return response.json();
}
async function command(method,args={}){
  const reply=await api('POST','/v1/domain/commands',{method,args,native:true,roots:[],connectionId:randomUUID()});
  assert.equal(reply.ok,true,method);return reply.value;
}
async function until(check,timeout=14000){
  const end=Date.now()+timeout;
  do{const result=await check();if(result)return result;await delay(80);}while(Date.now()<end);
  throw Error('bounded observation elapsed');
}
const equalPath=(a,b)=>a?.replaceAll('\\','/').replace(/\/+$/,'').toLowerCase()===b.replaceAll('\\','/').replace(/\/+$/,'').toLowerCase();
try{
  if(!process.env.RIDGE_CWD_PROBE_ASSETS){
  const log=await open(join(root,'build.log'),'w');
  const build=spawn(process.execPath,['node_modules/vite/bin/vite.js','build','--config','vite.remote.config.js','--outDir',join(assets,'mobile')],
    {windowsHide:true,stdio:['ignore',log.fd,log.fd]});
  const [code]=await once(build,'exit');await log.close();assert.equal(code,0,'mobile production build');
  }
  phase='runtime setup';
  const executable=join(root,'ridge-kernel.exe');await copyFile(resolve('target/debug/ridge-kernel.exe'),executable);
  kernel=spawn(executable,[],{cwd:root,windowsHide:true,stdio:'ignore',env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,
    RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_TEST_TLS_DIR:join(root,'tls'),
    RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_REMOTE_UI_ROOT:assets,RIDGE_REMOTE_MDNS_WINDOW_SECS:'0'}});
  endpoint=await until(async()=>{try{const value=JSON.parse(await readFile(join(root,'kernel.json'),'utf8'));return value.pid===kernel.pid?value:null;}catch{return null;}});
  await mkdir(moved,{recursive:true});await mkdir(shellB);
  await writeFile(join(moved,'ridge-previous-pane-only.txt'),'isolated CWD marker');
  const workspace=(await api('POST','/v1/domain/workspaces',{name:'CWD proof'})).workspace_id;
  const paneA=(await api('GET','/v1/domain/workspaces/'+workspace)).panes[0],paneB=randomUUID(),detached=randomUUID();
  await api('PUT','/v1/domain/workspaces/'+workspace+'/topology',{pane_tree:{
    root:{Split:{direction:'Horizontal',ratios:detachedProbe?[34,33,33]:[50,50],children:[{Leaf:paneA},{Leaf:paneB},...(detachedProbe?[{Leaf:detached}]:[])]}},
    panes:{[paneA]:{id:paneA,mode:'Terminal',cwd:shellA},[paneB]:{id:paneB,mode:'Terminal',cwd:shellB},...(detachedProbe?{[detached]:{id:detached,mode:'Terminal'}}:{})},
  }});
  for(const [pane,cwd] of [[paneA,shellA],[paneB,shellB]]){
    const result=await api('POST','/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:'cmd.exe',args:['/d','/q','/k'],cwd,env:{},role:'shell',cols:80,rows:24});
    assert.equal(result.ok,true,'plain terminal start');
  }
  const otherWorkspace=(await api('POST','/v1/domain/workspaces',{name:'Late metadata scope'})).workspace_id;
  await command('set_remote_enabled',{enabled:true,port:0});
  const info=await command('get_remote_info');
  assert.equal(info.tls,true);
  const cert=new X509Certificate(await readFile(join(root,'tls','cert.pem')));
  const pin=createHash('sha256').update(cert.publicKey.export({type:'spki',format:'der'})).digest('base64');
  browser=await chromium.launch({headless:true,args:['--ignore-certificate-errors-spki-list='+pin,'--enable-unsafe-swiftshader','--use-angle=swiftshader']});
  page=await browser.newPage({viewport:{width:390,height:844},isMobile:true,hasTouch:true,locale:'en-US'});

  let replayMetadata;
  const outgoing=[];
  await page.routeWebSocket(url => url.pathname === '/ws', clientWire => {
    const runtimeWire=clientWire.connectToServer();
    clientWire.onMessage(message => {
      if(typeof message==='string')try{outgoing.push(JSON.parse(message));}catch{}
      runtimeWire.send(message);
    });
    runtimeWire.onMessage(message => {
      if(typeof message==='string') try {
        const frame=JSON.parse(message);
        if(frame.type==='create-pane-result')creationReplies.push(frame);
        const paneMeta=frame.type==='pty-meta'?frame:frame.type==='event'&&frame.name==='pane-meta-changed'?frame.payload:null;
        if(paneMeta?.cwd && equalPath(paneMeta.cwd,moved)) replayMetadata=()=>clientWire.send(message);
      } catch {}
      clientWire.send(message);
    });
  });

  const metadata=[];
  page.on('websocket',socket=>socket.on('framereceived',({payload})=>{
    if(typeof payload!=='string')return;
    try{const frame=JSON.parse(payload);if(frame.type==='create-pane-result')creationReplies.push(frame);if(frame.type==='pty-meta'&&frame.cwd)metadata.push(frame);else if(frame.type==='event'&&frame.name==='pane-meta-changed'&&frame.payload?.cwd)metadata.push(frame.payload);}catch{}
  }));
  phase='LAN login';
  const navigation=await page.goto('https://127.0.0.1:'+info.port+'/?ui=mobile');
  const html=await readFile(join(assets,'mobile','index.html'),'utf8');
  const fingerprint=html.match(/assets\/index-[A-Za-z0-9_-]+\.js/)?.[0];
  assert.ok(fingerprint && (await navigation.text()).includes(fingerprint),'browser loaded the exact production bundle');
  evidence.productionBundleVerified=true;
  const input=page.locator('input[inputmode="numeric"][maxlength="6"]');
  await expect(input).toBeVisible({timeout:15000});
  await input.fill(String((await command('get_remote_info')).totpCode));await input.press('Enter');
  const header=page.locator('.breadcrumb-cwd');
  const tree=page.locator('button.tree-trigger');
  await expect(tree).toBeVisible({timeout:18000});
  async function select(cwd){
    if(equalPath(await header.getAttribute('title',{timeout:5000}).catch(()=>null),cwd))return;
    await tree.tap();
    const row=page.locator('.pane-row').filter({hasText:basename(cwd)});
    if(await row.count()===0)await page.locator('.ws-row').filter({hasText:'CWD proof'}).tap();
    await expect(row).toBeVisible({timeout:10000});
    await row.tap();
    await expect.poll(async()=>equalPath(await header.getAttribute('title').catch(()=>null),cwd),{timeout:12000}).toBe(true);
  }
  phase='select initial pane';
  await select(shellA);
  phase='host cd and live pane header';
  const text='cd /d "'+moved+'"\r';
  const written=await api('POST','/v1/domain/ptys/'+paneA+'/write',{workspace_id:workspace,data_b64:Buffer.from(text).toString('base64')});
  assert.equal(written.ok,true);
  await expect.poll(async()=>equalPath(await header.getAttribute('title').catch(()=>null),moved),{timeout:12000}).toBe(true);
  await expect(header).toContainText('cwd-one/cwd-live');
  evidence.checks.push('plain cmd.exe cd changes active mobile header without browser refresh or a cwd RPC');
  evidence.liveTransportMetadata=metadata.some(row=>equalPath(row.cwd,moved));
  phase='switch panes';
  await select(shellB);
  evidence.capturedLiveMetadata=Boolean(replayMetadata);
  assert.equal(evidence.capturedLiveMetadata,true);
  replayMetadata();
  await delay(900);
  evidence.delayedOtherPaneMetadataKeptCurrentCwd=equalPath(await header.getAttribute('title'),shellB);
  assert.equal(evidence.delayedOtherPaneMetadataKeptCurrentCwd,true);
  await select(moved);
  evidence.checks.push('switching between two real panes restores each pane’s own cwd');

  phase='inspect detached pane and sidebar scope';
  await tree.tap();
  const empty=page.locator('.pane-row').filter({hasNot:page.locator('.pane-cwd')});
  await expect(empty).toHaveCount(1,{timeout:10000});
  await empty.tap();
  await expect(page.locator('.breadcrumb-text')).toHaveText('shell',{timeout:8000});
  evidence.detachedSelected=true;
  await expect(page.getByTestId('terminal-detached-status')).toBeVisible();
  await expect(page.getByText(/REMOTE_RESIZE_FAILED/)).toHaveCount(0);
  await expect(page.getByTestId('terminal-stream-retry')).toHaveCount(0);
  await delay(800);
  await expect(header).toHaveCount(0);
  evidence.cwdFromPreviousPane=equalPath(await header.getAttribute('title',{timeout:500}).catch(()=>null),moved);
  const unknownStart=outgoing.length;
  const files=page.locator('button.hdr-btn[title="Files"]');
  await files.tap();
  const sidebar=page.getByRole('dialog',{name:'Sidebar'});
  await expect(sidebar).toBeVisible({timeout:10000});
  const sidebarCwd=sidebar.locator('.sb-header .cwd');
  evidence.detachedSidebar={};
  evidence.detachedSidebar.filesShowsPreviousCwd=equalPath(await sidebarCwd.getAttribute('title'),moved);
  evidence.detachedSidebar.filesShowPreviousDirectoryEntry=await sidebar.getByText('ridge-previous-pane-only.txt',{exact:true}).waitFor({state:'visible',timeout:5500}).then(()=>true,()=>false);
  await sidebar.locator('button.tab[title="Git"]').tap();
  evidence.detachedSidebar.gitShowsPreviousCwd=equalPath(await sidebarCwd.getAttribute('title'),moved);
  await sidebar.locator('button.tab[title="Search"]').tap();
  evidence.detachedSidebar.searchShowsPreviousCwd=equalPath(await sidebarCwd.getAttribute('title'),moved);
  await expect(sidebar.locator('.unknown-directory')).toBeVisible();
  const refersToOldDirectory=value=>typeof value==='string' ? equalPath(value,moved) : value && typeof value==='object' ? Object.values(value).some(refersToOldDirectory) : false;
  assert.equal(outgoing.slice(unknownStart).some(refersToOldDirectory),false,'unknown directory panels must not request the previous cwd');
  for (const button of await sidebar.locator('.sb-header button').all()) {
    const box=await button.boundingBox();assert.ok(box.width>=44 && box.height>=44,'sidebar controls require 44px touch targets');
    await button.focus();await expect(button).toBeFocused();
  }
  await sidebar.locator('button.close').tap();
  await select(moved);
  evidence.returnToOriginalPaneShowsLatestCwd=equalPath(await header.getAttribute('title'),moved);
  await tree.tap();
  const otherRow=page.locator('.ws-row').filter({hasText:'Late metadata scope'});
  await otherRow.tap();
  await expect(otherRow).toHaveClass(/\bactive\b/);
  await page.locator('.tree-backdrop').tap({position:{x:10,y:20}});
  await expect(header).toHaveCount(0);
  replayMetadata();
  await files.tap();
  await expect(sidebar.locator('.unknown-directory')).toBeVisible();
  await expect(header).toHaveCount(0);
  await sidebar.locator('button.close').tap();
  await select(moved);
  evidence.checks.push('Late metadata from a previous workspace does not restore its cwd or sidebar results');

  assert.equal(evidence.returnToOriginalPaneShowsLatestCwd,true);
  const staleScope=evidence.cwdFromPreviousPane||Object.values(evidence.detachedSidebar).some(Boolean);
  assert.equal(staleScope,false,'unknown-directory pane must not retain previous scope');
  phase='new workspace reuses its default terminal';
  await tree.tap();
  const originalWorkspaceIds=new Set((await api('GET','/v1/domain/workspaces')).workspaces);
  const previousPtys=(await api('GET','/v1/domain/ptys')).ptys.map(p=>({id:p.id,pid:p.child_pid}));
  await page.locator('.tree-add:not([data-testid="tree-open-saved"])').tap();
  const createdWorkspace=await until(async()=>
    (await api('GET','/v1/domain/workspaces')).workspaces.find(id=>!originalWorkspaceIds.has(id)));
  const activeRows=page.locator('.ws-row.active + .pane-group .pane-row');
  await expect(activeRows).toHaveCount(1);
  await delay(800);
  await expect(activeRows).toHaveCount(1);
  const workspacePtys=async()=> (await api('GET','/v1/domain/ptys')).ptys.filter(p=>p.workspace_id===createdWorkspace);
  assert.equal((await workspacePtys()).length,1,'new workspace must own exactly one default PTY');
  assert.ok(equalPath((await workspacePtys())[0].cwd,root),'default PTY cwd must stay inside the configured Runtime root');
  await page.locator('.pane-new').last().tap();
  const creationReply=await until(()=>creationReplies.at(-1));
  assert.equal(creationReply.success,true,JSON.stringify(creationReply));
  await expect(activeRows).toHaveCount(2);
  assert.equal((await workspacePtys()).length,2,'explicit new terminal must add exactly one PTY');
  assert.ok((await workspacePtys()).every(p=>equalPath(p.cwd,root)),'new PTY must inherit the default pane cwd');
  const currentPtys=(await api('GET','/v1/domain/ptys')).ptys;
  for(const previous of previousPtys)assert.equal(currentPtys.find(p=>p.id===previous.id)?.child_pid,previous.pid,'workspace creation must retain previous PTYs');
  evidence.checks.push('new workspace has one default terminal; explicit addition has two; previous PTYs stay alive');
  phase='explicit shell choice starts the detached pane';
  await page.locator('.ws-row').filter({hasText:'CWD proof'}).tap();
  await page.locator('.ws-row.active + .pane-group .pane-row').filter({hasNot:page.locator('.pane-cwd')}).tap();
  await expect(page.getByTestId('terminal-detached-status')).toBeVisible();
  assert.equal((await api('GET','/v1/domain/ptys')).ptys.some(p=>p.id===detached),false,'viewing detached state must not auto-start it');
  await tree.tap();
  await page.locator('.pane-row.active .row-shell').tap();
  await page.locator('.shell-item').filter({hasText:/powershell/i}).first().tap({timeout:20000});
  const started=await until(async()=> (await api('GET','/v1/domain/ptys')).ptys.find(p=>p.id===detached));
  assert.ok(started.child_pid>0,'explicit shell choice must create a real process for the same pane');
  assert.ok(equalPath(started.cwd,root),'explicit shell start must use the configured Runtime root when no cwd was saved');
  await page.locator('.tree-backdrop').tap({position:{x:10,y:20}});
  await expect(page.getByTestId('terminal-detached-status')).toHaveCount(0);
  await expect(page.getByText(/REMOTE_RESIZE_FAILED/)).toHaveCount(0);
  evidence.checks.push('detached view does not auto-start; explicit shell choice starts the same pane with a real PTY');
  evidence.status='PASS';
  if(staleScope) evidence.phase=phase;

}catch(error){
  evidence.status='FAIL';evidence.phase=phase;evidence.error=String(error.message).slice(0,1800);process.exitCode=1;
  evidence.creationReplies=creationReplies;
  if(endpoint)evidence.runtimePtys=(await api('GET','/v1/domain/ptys').catch(()=>({ptys:[]}))).ptys.map(p=>({id:p.id,pid:p.child_pid,cwd:p.cwd,workspace:p.workspace_id}));
  if(page&&!page.isClosed()) evidence.header={title:await page.locator('.breadcrumb-cwd').getAttribute('title',{timeout:1000}).catch(()=>null),paneRows:await page.locator('.pane-row .pane-cwd').allTextContents()};
  if(page&&!page.isClosed()) await page.screenshot({path:join(evidenceDir,'lan-error.png')}).catch(()=>{});
}finally{
  await browser?.close();
  if(endpoint)await api('POST','/v1/shutdown',{}).catch(()=>{});
  if(kernel?.exitCode===null){
    await Promise.race([once(kernel,'exit'),delay(4500)]);
    if(kernel.exitCode===null)await new Promise(done=>spawn('taskkill',['/F','/T','/PID',String(kernel.pid)],{windowsHide:true,stdio:'ignore'}).once('exit',done));
  }
  await writeFile(join(evidenceDir,'lan-cwd.json'),JSON.stringify(evidence,null,2)+'\n');
  console.log(JSON.stringify(evidence,null,2));
}



}
async function cloud(){
const root=await mkdtemp(join(tmpdir(),'ridge-mobile-cloud-'));
const mobile=join(root,'mobile');
const evidence={scope:'Built mobile SPA, real browser WebRTC and signed E2EE with Kernel, localhost account API and signaling fixtures',checks:[]};
const scope={grantId:'fixture-grant',granteeUserId:'guest',ownerUserId:'owner',deviceName:'desktop',role:'operator',delegable:false};
const types={'.html':'text/html','.js':'application/javascript','.css':'text/css','.wasm':'application/wasm','.json':'application/json','.webmanifest':'application/manifest+json','.png':'image/png'};
const diagnostics={};
let sharedWorkspace,host,sharedPeer,browser,kernel,endpoint,step='build',page;
const peers=new Map();let nextId=0;
const delay=ms=>new Promise(done=>setTimeout(done,ms));
const pass=(response,data)=>{response.setHeader('content-type','application/json');response.end(JSON.stringify({ok:true,data}));};
const reject=response=>{response.writeHead(403,{'content-type':'application/json'});response.end(JSON.stringify({ok:false,error:{code:'FORBIDDEN',message:'fixture grant denied'}}));};
const server=http.createServer(async(request,response)=>{
  try {
    const route=new URL(request.url,'http://localhost').pathname;
    if(route==='/api/v1/auth/session'){
      const guest=request.headers.cookie?.includes('ridge_sso=fixture-guest');
      if(!guest&&!request.headers.cookie?.includes('ridge_sso=fixture-owner'))return reject(response);
      const username=guest?'guest':'owner';
      return pass(response,{token:guest?'owned-guest':'owned-controller',user:{id:username,username,premium:true}});
    }
    if(route==='/api/v1/ice-servers')return pass(response,{iceServers:[]});
    if(route==='/api/v1/workspace-shares/shared-with-me'){
      if(request.headers.authorization!=='Bearer owned-guest')return reject(response);
      return pass(response,{shares:[{id:scope.grantId,ownerUsername:'owner',status:'active'}]});
    }
    if(route==='/api/v1/workspace-shares/fixture-grant/access-token'){
      if(request.headers.authorization!=='Bearer owned-guest'||request.method!=='POST')return reject(response);
      return pass(response,{...scope,token:'owned-share',workspaceId:sharedWorkspace});
    }
    if(route==='/api/v1/workspace-shares/verify-access'){
      let body='';for await (const bytes of request)body+=bytes;
      if(request.headers.authorization!=='Bearer owned-device'||JSON.parse(body).token!=='owned-share')return reject(response);
      return pass(response,{...scope,workspaceId:sharedWorkspace});
    }
    if(route.startsWith('/api/')){response.writeHead(404);response.end();return;}
    const file=resolve(mobile,'.'+(route==='/'?'/index.html':route));
    if(!file.startsWith(mobile+sep)){response.writeHead(404);response.end();return;}
    const info=await stat(file);
    response.writeHead(200,{'content-type':types[extname(file)]??'application/octet-stream','content-length':info.size});
    createReadStream(file).pipe(response);
  }catch{if(!response.headersSent)response.writeHead(404);response.end();}
});
await new Promise(done=>server.listen(0,'127.0.0.1',done));
const port=server.address().port, origin='http://localhost:'+port;
const relay=new WebSocketServer({server});
const send=(socket,message)=>{if(socket?.readyState===1)socket.send(JSON.stringify(message));};
relay.on('connection',(socket,request)=>{
  const url=new URL(request.url,origin),role=url.searchParams.get('role'),token=url.searchParams.get('token');
  if(role==='host'?token!=='owned-device':!['owned-controller','owned-share'].includes(token)){socket.close();return;}
  let id;
  if(role==='host'){
    host=socket;send(socket,{t:'welcome',room:'owned',role:'host',peerPresent:peers.size>0});
  }else{
    id='owned-'+(++nextId);peers.set(id,socket);
    if(token==='owned-share'){
      sharedPeer=socket;
      send(host,{t:'workspace-scope',cid:id,token:'owned-share'});
    }
    send(socket,{t:'welcome',room:'owned',role:'controller',cid:id,peerPresent:!!host});
    send(host,{t:'peer-join',role:'controller',cid:id});
  }
  socket.on('message',bytes=>{
    let message;try{message=JSON.parse(bytes);}catch{return;}
    if(role==='controller'){if(['offer','ice','e2ee-pubkey'].includes(message.t))send(host,{...message,cid:id});}
    else if(message.t==='kick')peers.get(message.cid)?.close();
    else if(['answer','ice','e2ee-pubkey'].includes(message.t))send(peers.get(message.cid),message);
  });
  socket.on('close',()=>{
    if(role==='host'){if(host===socket)host=undefined;for(const peer of peers.values())send(peer,{t:'peer-leave',role:'host'});}
    else{peers.delete(id);send(host,{t:'peer-leave',role:'controller',cid:id});}
  });
});
async function until(check,label,timeout=15000){
  const end=Date.now()+timeout;
  do {const value=await check();if(value)return value;await delay(90);}while(Date.now()<end);
  throw Error(label);
}
async function request(route,body,method='POST',timeout=8000){
  const result=await fetch('http://127.0.0.1:'+endpoint.port+route,{method,
    headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},
    body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(timeout)});
  assert.equal(result.status,200,route);return result.json();
}
async function command(method,args={},timeout=8000){
  const reply=await request('/v1/domain/commands',{method,args,native:true,roots:[],connectionId:randomUUID()},'POST',timeout);
  assert.equal(reply.ok,true,method+' rejected');return reply.value;
}
try{
  const buildLog=join(root,'build.log');
  const {open}=await import('node:fs/promises');
  const log=await open(buildLog,'w');
  const build=spawn(process.execPath,['node_modules/vite/bin/vite.js','build','--config','vite.remote.config.js','--outDir',mobile],
    {windowsHide:true,stdio:['ignore',log.fd,log.fd],env:{...process.env,RIDGE_CLOUD_BASE_DOMAIN:'localhost:'+port,RIDGE_CLOUD_DEV_PLAINTEXT:'1'}});
  const [code]=await once(build,'exit');await log.close();
  assert.equal(code,0,'mobile production build failed');
  evidence.checks.push('mobile production bundle built against the isolated loopback Cloud service');
  step='runtime';
  const target=resolve(process.env.CARGO_TARGET_DIR ?? 'target','debug');
  const executable=join(target,'ridge-kernel.exe');
  kernel=spawn(executable,[],{cwd:root,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,RUST_LOG:'info',RIDGE_KERNEL_DATA_DIR:root,
    RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:root,RIDGE_AUTH_FILE:join(root,'auth.json'),
    RIDGE_CDP_SHELL_PATH:process.env.PATH,
    RIDGE_TEST_TLS_DIR:join(root,'tls'),RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'}});
  const { createInterface } = await import('node:readline');
  const { stripVTControlCharacters } = await import('node:util');
  evidence.hostDisconnects=[];
  for (const stream of [kernel.stdout, kernel.stderr]) {
    createInterface({input:stream}).on('line',line=>{
      const row=stripVTControlCharacters(line);
      if(!row.includes('Cloud controller disconnected'))return;
      const error=row.match(/\berror=(.+)$/)?.[1];
      evidence.hostDisconnects.push(error?.slice(0,220)??'Host reported controller disconnect');
    });
  }
  endpoint=await until(async()=>{try{const e=JSON.parse(await readFile(join(root,'kernel.json'),'utf8'));return e.pid===kernel.pid?e:null;}catch{return null;}},'Runtime startup deadline');

  const workspaceId=(await request('/v1/domain/workspaces',{name:'Cloud CWD proof'})).workspace_id;
  const {panes:[paneA]}=await request('/v1/domain/workspaces/'+workspaceId,undefined,'GET');
  const paneB=randomUUID(), detached=randomUUID();
  const shellA=join(root,'cloud-cwd-one'), shellB=join(root,'cloud-cwd-two'), moved=join(shellA,'cloud-cwd-live');
  await mkdir(moved,{recursive:true}); await mkdir(shellB);
  await writeFile(join(moved,'ridge-previous-pane-only.txt'),'isolated CWD marker');
  await request('/v1/domain/workspaces/'+workspaceId+'/topology',{pane_tree:{
    root:{Split:{direction:'Horizontal',ratios:[34,33,33],children:[{Leaf:paneA},{Leaf:paneB},{Leaf:detached}]}},
    panes:{[paneA]:{id:paneA,mode:'Terminal',cwd:shellA},[paneB]:{id:paneB,mode:'Terminal',cwd:shellB},[detached]:{id:detached,mode:'Terminal'}}
  }},'PUT');
  for(const [paneId,cwd] of [[paneA,shellA],[paneB,shellB]]) {
    const started=await request('/v1/domain/ptys',{workspace_id:workspaceId,pty_id:paneId,program:'cmd.exe',
      args:['/d','/q','/k'],cwd,env:{},role:'shell',cols:80,rows:24});
    assert.equal(started.ok,true);
  }
  sharedWorkspace=workspaceId;

  await command('cloud_host_online',{deviceToken:'owned-device',deviceName:'desktop',username:'owner',baseDomain:'localhost:'+port,plaintext:true});
  await until(async()=>(await command('get_cloud_host_state')).state==='online','Cloud host did not reach fixture relay');
  browser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
  async function mobilePage(role){
    const context=await browser.newContext({viewport:{width:390,height:844},isMobile:true,hasTouch:true,locale:'en-US'});
    await context.addCookies([{name:'ridge_sso',value:'fixture-'+role,url:origin,httpOnly:true,sameSite:'Lax'}]);
    await context.addInitScript(() => {
      const Native = window.RTCPeerConnection;
      if (!Native) return;
      const transitions = window.__ridgeProbeRtc = [];
      window.RTCPeerConnection = class extends Native {
        constructor(...args) {
          super(...args);
          const observe = event => transitions.push({
            event, at: Math.round(performance.now()),
            connection: this.connectionState, ice: this.iceConnectionState,
            signaling: this.signalingState,
          });
          observe('create');
          for (const name of ['connectionstatechange', 'iceconnectionstatechange', 'signalingstatechange']) {
            this.addEventListener(name, () => observe(name));
          }
        }
      };
    });
    const result=await context.newPage();
    diagnostics[role]={api:[],socket:0,jsErrors:[],sidebarAssets:[],reconnects:[]};
    result.on('pageerror',error=>diagnostics[role].jsErrors.push(String(error.message).slice(0,180)));
    result.on('console', async event => {
      if (!event.text().startsWith('[cloud-controller] scheduleReconnect')) return;
      const detail = await event.args()[1]?.jsonValue().catch(() => null);
      if (detail) diagnostics[role].reconnects.push({ reason: detail.reason, attempt: detail.attempt });
    });
    result.on('response',response=>{const route=new URL(response.url()).pathname;if(route.startsWith('/api/'))diagnostics[role].api.push([route,response.status()]);if(route.includes('RemoteSidebar'))diagnostics[role].sidebarAssets.push([route,response.status()]);});
    result.on('websocket',()=>diagnostics[role].socket++);
    await result.goto(origin+'/?cloudHost=desktop&u=owner'+(role==='guest'?'&share=fixture-grant':''));
    return result;
  }
  step='owner cloud login and TOTP';
  const ownerPage=await mobilePage('owner');
  page=ownerPage;
  const input=page.locator('input[inputmode="numeric"][maxlength="6"]');
  await expect(input).toBeVisible({timeout:20000});
  const realCode=String((await command('get_remote_info')).totpCode);
  await input.fill(realCode==='000000'?'111111':'000000');await input.press('Enter');
  await expect(page.locator('.error-msg')).toBeVisible({timeout:12000});
  await input.fill(String((await command('get_remote_info')).totpCode));await input.press('Enter');

  step='Cloud live cwd and pane switching';
  const header=page.locator('.breadcrumb-cwd');
  const tree=page.locator('button.tree-trigger');
  const pathEquals=(a,b)=>a?.replaceAll('\\','/').replace(/\/+$/,'').toLowerCase()===b.replaceAll('\\','/').replace(/\/+$/,'').toLowerCase();
  await expect(tree).toBeVisible({timeout:20000});
  async function select(cwd) {
    if(pathEquals(await header.getAttribute('title',{timeout:800}).catch(()=>null),cwd))return;
    await tree.tap();
    const pane=page.locator('.pane-row').filter({hasText:cwd.split(/[\\/]/).at(-1)});
    if(await pane.count()===0){
      await page.locator('.ws-row').filter({hasText:'Cloud CWD proof'}).tap();
      if(pathEquals(await header.getAttribute('title',{timeout:800}).catch(()=>null),cwd))return;
      await tree.tap();
    }
    await expect(pane).toBeVisible({timeout:12000});
    await pane.tap();
    await expect.poll(async()=>pathEquals(await header.getAttribute('title',{timeout:800}).catch(()=>null),cwd)).toBe(true);
  }
  await select(shellA);
  const written=await request('/v1/domain/ptys/'+paneA+'/write',{workspace_id:workspaceId,
    data_b64:Buffer.from('cd /d "'+moved+'"\r').toString('base64')});
  assert.equal(written.ok,true);
  await expect.poll(async()=>pathEquals(await header.getAttribute('title'),moved),{timeout:12000}).toBe(true);
  evidence.checks.push('real cmd.exe cd updates the Cloud mobile header without reload');
  const status=page.locator('.status-dot').first();
  evidence.firstPaneBeforeSwitch={
    connectionDot:await status.getAttribute('class'),
    rtcSampleTime:await page.evaluate(()=>Math.round(performance.now())),
  };
  step='Cloud Files before pane switch';
  const files=page.locator('button.hdr-btn[title="Files"]');
  const sidebar=page.getByRole('dialog',{name:'Sidebar'});
  await expect(status).toHaveClass(/\bconnected\b/);
  await files.tap();
  await expect(sidebar).toBeVisible({timeout:8000});
  evidence.firstPaneBeforeSwitch.sidebarShowsCurrentCwd=pathEquals(
    await sidebar.locator('.sb-header .cwd').getAttribute('title'),moved,
  );
  await expect(sidebar.getByText('ridge-previous-pane-only.txt',{exact:true})).toBeVisible({timeout:8000});
  evidence.firstPaneBeforeSwitch.filesReadCurrentDirectory=true;
  await sidebar.locator('button.close').tap();
  await expect(status).toHaveClass(/\bconnected\b/);
  evidence.checks.push('Files opens on a healthy Cloud connection and reads the active shell directory');
  step='Cloud live pane switches after positive Files control';
  await select(shellB);
  await select(moved);
  evidence.checks.push('two live panes show their own cwd after switching');
  evidence.livePane={connectionDot:await status.getAttribute('class')};
  await files.tap();
  evidence.livePane.sidebarOpened=await sidebar.waitFor({state:'visible',timeout:5000}).then(()=>true,()=>false);
  if(evidence.livePane.sidebarOpened){
    evidence.livePane.sidebarShowsCurrentCwd=pathEquals(await sidebar.locator('.sb-header .cwd').getAttribute('title'),moved);
    await sidebar.locator('button.close').tap();
  }else{
    const overlay=page.locator('.sidebar-overlay');
    if(await overlay.isVisible())await overlay.click({position:{x:10,y:80}});
  }
  evidence.livePane.connectionDotAfterSidebar=await status.getAttribute('class');
  evidence.livePane.rtcSampleTime=await page.evaluate(()=>Math.round(performance.now()));
  step='Cloud detached pane sidebar';
  await tree.tap();
  const detachedRow=page.locator('.pane-row').filter({hasNot:page.locator('.pane-cwd')});
  await expect(detachedRow).toHaveCount(1);
  const detachedTitle=await detachedRow.locator('.pane-name').innerText();
  await detachedRow.tap();
  await expect(page.locator('.breadcrumb-text')).toHaveText(detachedTitle,{timeout:8000});
  evidence.detachedPaneTitle=detachedTitle;
  await expect(page.getByTestId('terminal-detached-status')).toBeVisible();
  await expect(page.getByText(/REMOTE_RESIZE_FAILED/)).toHaveCount(0);
  await expect(page.getByTestId('terminal-stream-retry')).toHaveCount(0);
  await delay(800);
  evidence.detachedSelected=true;
  evidence.cwdFromPreviousPane=pathEquals(await header.getAttribute('title',{timeout:800}).catch(()=>null),moved);
  await files.tap();
  const opened=await sidebar.waitFor({state:'visible',timeout:8000}).then(()=>true,()=>false);
  evidence.detachedSidebarOpened=opened;
  assert.equal(opened,true,'unknown directory drawer must open');
  await expect(header).toHaveCount(0);
  await expect(status).toHaveClass(/\bconnected\b/);
  if(opened){
    const cwd=sidebar.locator('.sb-header .cwd');
    evidence.detachedSidebar={filesShowsPreviousCwd:pathEquals(await cwd.getAttribute('title'),moved)};
    evidence.detachedSidebar.filesShowPreviousDirectoryEntry=await sidebar.getByText('ridge-previous-pane-only.txt',{exact:true}).waitFor({state:'visible',timeout:5500}).then(()=>true,()=>false);
    await sidebar.locator('button.tab[title="Git"]').tap();
    evidence.detachedSidebar.gitShowsPreviousCwd=pathEquals(await cwd.getAttribute('title'),moved);
    await sidebar.locator('button.tab[title="Search"]').tap();
    evidence.detachedSidebar.searchShowsPreviousCwd=pathEquals(await cwd.getAttribute('title'),moved);
    await expect(sidebar.locator('.unknown-directory')).toBeVisible();
    await sidebar.locator('button.close').tap();
  }else{
    evidence.sidebarDiagnostics={
      filesButtonActive:await files.evaluate(button=>button.classList.contains('active')),
      sidebarLoading:(await page.locator('.sidebar-loading').allTextContents()).map(text=>text.trim()),
      overlayVisible:await page.locator('.sidebar-overlay').isVisible(),
      connectionDot:await page.locator('.status-dot').first().getAttribute('class'),
      sidebarAssets:diagnostics.owner.sidebarAssets,
      pageErrors:diagnostics.owner.jsErrors,
    };
    const overlay=page.locator('.sidebar-overlay');
    if(await overlay.isVisible())await overlay.click({position:{x:10,y:80}});
  }

  await select(moved);
  evidence.returnToOriginalPaneShowsLatestCwd=pathEquals(await header.getAttribute('title'),moved);
  evidence.rtcTransitions=await page.evaluate(()=>window.__ridgeProbeRtc);
  evidence.reconnects=diagnostics.owner.reconnects;
  assert.deepEqual(evidence.reconnects,[],'pane switching must not reconnect');
  assert.deepEqual(evidence.hostDisconnects,[],'per-pane errors must not close Cloud control');
  assert.equal(evidence.returnToOriginalPaneShowsLatestCwd,true);
  const staleScope=evidence.cwdFromPreviousPane||Object.values(evidence.detachedSidebar??{}).some(Boolean);
  assert.equal(staleScope,false,'unknown-directory pane must not retain previous scope');
  evidence.status='PASS';
  if(staleScope)evidence.phase=step;
  step='shared workspace Commune navigation';
  const guest=await mobilePage('guest');page=guest;
  await expect(guest.locator('button.tree-trigger')).toBeVisible({timeout:20000});
  await guest.locator('button.hdr-btn[title="Team"]').tap();
  const guestRoster=guest.getByTestId('global-commune-roster');
  await expect(guestRoster).toBeVisible({timeout:10000});
  await expect(guestRoster.getByRole('button',{name:/^(Groups|编组)$/})).toHaveCount(0);
  await expect(guestRoster.getByTestId('host-commune-groups')).toHaveCount(0);
  evidence.checks.push('A real workspace-scoped Cloud grant does not expose host group navigation or management');

}catch(error){
  evidence.status='FAIL';evidence.phase=step;evidence.error=String(error.message).slice(0,950);
  process.exitCode=1;
}finally{
  await browser?.close();
  if(endpoint)try{await request('/v1/shutdown',{});}catch{}
  if(kernel?.exitCode===null){
    await Promise.race([once(kernel,'exit'),delay(5000)]);
    if(kernel.exitCode===null)await new Promise(done=>spawn('taskkill',['/F','/T','/PID',String(kernel.pid)],{windowsHide:true,stdio:'ignore'}).once('exit',done));
  }
  for(const socket of relay.clients)socket.terminate();
  await new Promise(done=>relay.close(done));await new Promise(done=>server.close(done));
  await writeFile(join(evidenceDir,'cloud-cwd.json'),JSON.stringify(evidence,null,2)+'\n');
  console.log(JSON.stringify(evidence,null,2));
}




}
await lan();
await cloud();
