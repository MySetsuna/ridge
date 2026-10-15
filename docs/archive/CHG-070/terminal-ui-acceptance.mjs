// Targeted real UI receipt. Every process/profile/data directory belongs to this fixture.
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,copyFile,open,readFile,writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {randomUUID,createHash} from 'node:crypto';
import {createServer} from 'node:net';
import {chromium} from '@playwright/test';
const execute=promisify(execFile),repo=process.cwd(),root=await mkdtemp(join(tmpdir(),'ridge-terminal-ui-'));
const allowed=join(root,'allowed'),outside=join(root,'outside'),data=join(root,'runtime'),profile=join(root,'webview');
for(const path of [allowed,outside,data,profile])await mkdir(path);
const kernelBinary=join(root,'fixture-kernel.exe'),desktopBinary=join(root,'fixture-desktop.exe');
await copyFile(resolve('target/debug/ridge-kernel.exe'),kernelBinary);await copyFile(resolve('target/debug/ridge.exe'),desktopBinary);
const compiledOrigins=[...new Set((await readFile(desktopBinary)).toString('latin1').match(/http:\/\/127\.0\.0\.1:\d{2,5}/g))];
assert.equal(compiledOrigins.length,1,'the copied debug desktop needs one unambiguous compiled dev origin');
const devUrl=new URL(compiledOrigins[0]);
assert.equal(devUrl.hostname,'127.0.0.1');assert.equal(devUrl.protocol,'http:');
const env={...process.env,RIDGE_KERNEL_DATA_DIR:data,RIDGE_KERNEL_FS_ROOT:allowed,RIDGE_AUTH_FILE:join(root,'auth.json'),RIDGE_WORKSPACE_SAVE_DIR:allowed,
 RIDGE_CDP_USER_DATA_DIR:profile,RIDGE_CDP_STARTUP_CWD:allowed,RIDGE_CDP_NATIVE_DATA_DIR:join(root,'native'),RIDGE_KERNEL_HOST_BINARY:kernelBinary,
 RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1',RIDGE_CDP_ALLOW_NON_BREAKAWAY:'1',RIDGE_TEST_TLS_DIR:join(root,'tls'),RIDGE_DISABLE_SINGLE_INSTANCE:'1',RIDGE_REMOTE_UI_ROOT:resolve('remote-dist'),
 WEBVIEW2_USER_DATA_FOLDER:profile,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*'};
delete env.RIDGE_NATIVE_RUNTIME;
const delay=ms=>new Promise(done=>setTimeout(done,ms));
async function until(check,message,ms=30000){const end=Date.now()+ms;while(Date.now()<end){if(await check())return;await delay(100);}throw Error(message);}
async function launch(binary,args,environment,name){const log=await open(join(root,name+'.log'),'a');const process=spawn(binary,args,{cwd:repo,env:environment,stdio:['ignore',log.fd,log.fd],windowsHide:true});await log.close();return process;}
const kernel=await launch(kernelBinary,[],env,'kernel');let endpoint,desktop,vite,remoteVite,browser,remoteBrowser,page,remote;
const report={completeChange:false,checks:[],recoveries:[],root};
async function request(path,body,method='POST'){
 const response=await fetch(`http://127.0.0.1:${endpoint.port}${path}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});
 assert.equal(response.status,200,`${method} ${path}`);return response.json();
}
const connection=randomUUID();
async function command(method,args={}){const result=await request('/v1/domain/commands',{method,args,native:true,roots:[],connectionId:connection});assert.equal(result.ok,true,`${method}: ${JSON.stringify(result.error)}`);return result.value;}
async function inventory(){const {stdout}=await execute('powershell',['-NoProfile','-Command','Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CreationDate | ConvertTo-Json -Compress'],{windowsHide:true,maxBuffer:4*1024*1024});return JSON.parse(stdout);}
async function stopOwned(child,expected){
 if(!child||child.exitCode!==null||child.signalCode!==null)return;
 const rows=await inventory(),owner=rows.find(row=>row.ProcessId===child.pid);
 assert.equal(owner?.ExecutablePath?.toLowerCase(),expected.toLowerCase(),'fixture process path changed');
 const ids=new Set([child.pid]);let changed=true;while(changed){changed=false;for(const row of rows)if(ids.has(row.ParentProcessId)&&!ids.has(row.ProcessId)){ids.add(row.ProcessId);changed=true;}}
 const owned=rows.filter(row=>ids.has(row.ProcessId));assert.ok(owned.every(row=>row.ProcessId!==endpoint?.pid));
 const current=await inventory();const exact=owned.filter(row=>current.some(now=>now.ProcessId===row.ProcessId&&now.CreationDate===row.CreationDate&&now.ExecutablePath===row.ExecutablePath));
 if(exact.length)await execute('powershell',['-NoProfile','-Command',`Stop-Process -Id ${exact.map(row=>row.ProcessId).join(',')} -Force -ErrorAction SilentlyContinue`],{windowsHide:true});
 await Promise.race([once(child,'exit').catch(()=>{}),delay(3000)]);
}
async function openDesktop(){
 desktop=await launch(desktopBinary,[],env,'desktop');
 let port;
 await until(async()=>{assert.equal(desktop.exitCode,null);try{port=Number((await readFile(join(profile,'EBWebView','DevToolsActivePort'),'utf8')).split('\n')[0]);return (await fetch(`http://127.0.0.1:${port}/json/version`,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}},'owned WebView CDP startup failed',60000);
 const version=await(await fetch(`http://127.0.0.1:${port}/json/version`)).json();assert.ok(version.webSocketDebuggerUrl.startsWith(`ws://127.0.0.1:${port}/`));
 browser=await chromium.connectOverCDP(version.webSocketDebuggerUrl);await until(async()=>{page=browser.contexts().flatMap(context=>context.pages())[0];return Boolean(page);},'owned WebView missing');
 page.on('console',async message=>{if(message.text().startsWith('[ridge-term] terminal stream requires snapshot'))report.recoveries.push(await Promise.all(message.args().map(arg=>arg.jsonValue().catch(()=>null))));});
 await until(async()=>page.locator('[data-rg-pane-id]').count().catch(()=>0),'desktop pane did not mount',60000).catch(async error=>{
  await writeFile(join(root,'startup.json'),JSON.stringify(await page.evaluate(()=>({url:location.href,title:document.title,text:document.body?.innerText?.slice(0,4000),osBridge:Boolean(window.__TAURI_INTERNALS__)})),null,2));
  await page.screenshot({path:join(root,'startup.png')}).catch(()=>{});throw error;
 });
 const owner=(await inventory()).find(row=>row.ProcessId===desktop.pid);assert.equal(owner?.ExecutablePath?.toLowerCase(),desktopBinary.toLowerCase());
}
async function rendered(){return page.evaluate(async pane=>{
 const {TerminalManager}=await import('/packages/remote/src/shared/terminal/manager.ts');const entry=TerminalManager.instance().panes.get(pane);
 const input=entry?.kernel.terminalInputContext();
 return entry?{context:input?{pane:input[0],terminalEpoch:input[1],activationId:input[2]}:null,text:entry.kernel.dumpVisibleText()}:null;
},report.pane);}
async function typeCommand(text){const pane=page.locator(`[data-rg-pane-id="${report.pane}"]`);await pane.click({position:{x:30,y:30}});await page.keyboard.type(text);await page.keyboard.press('Enter');}
async function renderedRemote(){return remote.evaluate(async pane=>{
 const resource=performance.getEntriesByType('resource').find(entry=>entry.name.includes('/packages/remote/src/shared/terminal/manager.ts'));
 if(!resource)return null;
 const {TerminalManager}=await import(resource.name);
 for(const entry of TerminalManager.instance().panes.values()){
  const input=entry.kernel.terminalInputContext();
  if(input?.[0]?.paneId===pane)return {epoch:input[1],text:entry.kernel.dumpVisibleText()};
 }
 return null;
},report.pane);}
async function freePort(){const server=createServer();await new Promise(done=>server.listen(0,'127.0.0.1',done));const port=server.address().port;await new Promise(done=>server.close(done));return port;}
try{
 await until(async()=>{assert.equal(kernel.exitCode,null);try{endpoint=JSON.parse(await readFile(join(data,'kernel.json'),'utf8'));return endpoint.pid===kernel.pid&&(await request('/v1/health',undefined,'GET')).ok;}catch{return false;}},'owned Runtime startup failed');
 await command('set_user_default_cwd',{path:allowed});
 const created=await request('/v1/domain/workspaces',{});report.workspace=created.workspace_id;
 const graph=await request(`/v1/domain/workspaces/${report.workspace}`,undefined,'GET');report.pane=graph.panes[0];
 assert.equal((await request('/v1/domain/ptys',{pty_id:report.pane,workspace_id:report.workspace,program:'cmd.exe',args:[],cwd:allowed,launch_profile:'ridge-interactive',cols:80,rows:24})).ok,true);
 const secondWorkspace=(await request('/v1/domain/workspaces',{name:'Second workspace'})).workspace_id;
 const secondPane=(await request(`/v1/domain/workspaces/${secondWorkspace}`,undefined,'GET')).panes[0];
 assert.equal((await request('/v1/domain/ptys',{pty_id:secondPane,workspace_id:secondWorkspace,program:'cmd.exe',args:[],cwd:allowed,launch_profile:'ridge-interactive',cols:80,rows:24})).ok,true);
 vite=await launch(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',devUrl.port,'--strictPort'],env,'vite');
 await until(async()=>{assert.equal(vite.exitCode,null);try{return(await fetch(new URL('/@vite/client',devUrl),{signal:AbortSignal.timeout(3000)})).ok;}catch{return false;}},'owned Vite failed',60000);
 await openDesktop();await until(async()=>Boolean((await rendered())?.context),'desktop canonical snapshot missing');
 const initial=await rendered();report.epochs=[initial.context.terminalEpoch];
 async function fitted(){
  const geometry=await page.evaluate(async pane=>{
   const {TerminalManager}=await import('/packages/remote/src/shared/terminal/manager.ts');const entry=TerminalManager.instance().panes.get(pane);
   const rect=entry.container.getBoundingClientRect(),style=getComputedStyle(entry.container);
   return {width:rect.width,height:rect.height,rows:entry.kernel.rows(),cols:entry.kernel.cols(),
    expectedRows:Math.max(1,Math.floor((rect.height-(parseFloat(style.paddingTop)||0)-(parseFloat(style.paddingBottom)||0))/entry.cellH)),
    expectedCols:Math.max(1,Math.floor((rect.width-(parseFloat(style.paddingLeft)||0)-(parseFloat(style.paddingRight)||0))/entry.cellW))};
  },report.pane);
  const pty=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===report.pane);
  return geometry.rows===geometry.expectedRows&&geometry.cols===geometry.expectedCols&&pty.rows===geometry.rows&&pty.cols===geometry.cols?geometry:null;
 }
 await until(async()=>Boolean(await fitted()),'initial desktop pane never fitted its actual Runtime grid');
 const beforeSize=await fitted();
 const surface=page.locator(`[data-rg-pane-id="${report.pane}"]`);
 const originalSize=await surface.evaluate(element=>({width:element.style.width,height:element.style.height}));
 await surface.evaluate((element,size)=>{element.style.width=`${Math.floor(size.width*.65)}px`;element.style.height=`${Math.floor(size.height*.65)}px`;},beforeSize);
 await until(async()=>{const grid=await fitted();return grid&&grid.rows<beforeSize.rows&&grid.cols<beforeSize.cols;},'shrinking pane did not resize actual Runtime grid');
 await surface.evaluate((element,size)=>{element.style.width=size.width;element.style.height=size.height;},originalSize);
 await until(async()=>{const grid=await fitted();return grid&&grid.rows===beforeSize.rows&&grid.cols===beforeSize.cols;},'expanding pane did not restore actual Runtime grid');
 report.checks.push('real desktop pane initially fills available grid and Runtime follows shrink and expansion');
 // Lazy API dependency discovery can reload a dev WebView; warm it before mounting the fixture panel.
 await page.evaluate(()=>import('/src/lib/teammate/AgentCenterPanel.svelte')).catch(()=>{});
 await delay(1500);await page.reload();
 await until(async()=>Boolean((await rendered())?.context),'canonical pane missing after lazy module warmup');
 await command('save_agent_profile_overrides',{overrides:[{id:'owned-protocol-candidate',processNames:['cmd'],executable:'cmd',resumeArgv:[],yoloArgs:[]}]});
 await page.evaluate(async workspaceId=>{
  const [{mount,unmount},{default:Panel}]=await Promise.all([import('/node_modules/svelte/src/index-client.js'),import('/src/lib/teammate/AgentCenterPanel.svelte')]);
  const target=document.createElement('div');target.dataset.ownedCommune='true';
  Object.assign(target.style,{position:'fixed',inset:'0 auto 0 0',width:'340px',zIndex:'100',background:'#171717'});document.body.appendChild(target);
  const panel=mount(Panel,{target,props:{workspaceId}});window.closeOwnedCommune=async()=>{await unmount(panel);target.remove();};
 },report.workspace);
 const commune=page.locator('[data-owned-commune="true"]');
 const firstRow=commune.locator(`[data-testid="commune-terminal-agent"][data-pane-id="${report.pane}"]`);
 const secondRow=commune.locator(`[data-testid="commune-terminal-agent"][data-pane-id="${secondPane}"]`);
 await firstRow.waitFor({timeout:30000});await secondRow.waitFor({timeout:30000});
 assert.equal(await commune.getByTestId('commune-terminal-agent').count(),2);
 await firstRow.getByRole('button',{name:'展开交互',exact:true}).click();
 const send=firstRow.getByRole('button',{name:'发送并回车',exact:true});
 await firstRow.getByRole('textbox').fill('echo SIDEBAR_FIRST');
 await until(()=>send.isEnabled(),'sidebar view did not become writable');await send.click();
 await until(async()=>(await rendered())?.text.some(row=>row.trim()==='SIDEBAR_FIRST'),'sidebar text did not reach original terminal');
 await firstRow.getByRole('button',{name:'向原终端发送 ArrowUp',exact:true}).click();
 await firstRow.getByRole('button',{name:'向原终端发送 Enter',exact:true}).click();
 await until(async()=>(await rendered())?.text.filter(row=>row.trim()==='SIDEBAR_FIRST').length===2,'sidebar history selection keys did not reach original terminal');
 await secondRow.getByRole('button',{name:'展开交互',exact:true}).click();
 await secondRow.getByRole('textbox').fill('echo SIDEBAR_SECOND');
 const secondSend=secondRow.getByRole('button',{name:'发送并回车',exact:true});await until(()=>secondSend.isEnabled(),'second workspace view unavailable');await secondSend.click();
 const secondCandidate=(await command('get_teammate_topology',{workspaceId:secondWorkspace})).discovered[0];
 const secondView=()=>command('get_agent_terminal_view',{workspaceId:secondWorkspace,paneId:secondPane,process:secondCandidate.process,terminalEpoch:secondCandidate.terminalEpoch});
 await until(async()=>(await secondView()).text.split('\n').some(row=>row.trim()==='SIDEBAR_SECOND'),'cross-workspace sidebar message did not reach its own terminal');
 assert.ok(!(await rendered()).text.some(row=>row.trim()==='SIDEBAR_SECOND'),'cross-workspace input leaked to active pane');
 report.checks.push('actual sidebar aggregates two workspaces, sends text and selection keys into original PTYs without switching or mixing scope');
 const duplicate={process:secondCandidate.process,input:{pane:{workspaceId:secondWorkspace,paneId:secondPane},terminalEpoch:secondCandidate.terminalEpoch,activationId:1,sourceId:randomUUID(),sequence:1,bytes:[...Buffer.from('echo EXACTLY_ONCE\r')]}};
 assert.equal((await command('write_agent_terminal_input',duplicate)).alreadyApplied,false);
 assert.equal((await command('write_agent_terminal_input',duplicate)).alreadyApplied,true);
 await until(async()=>(await secondView()).text.split('\n').filter(row=>row.trim()==='EXACTLY_ONCE').length===1,'input retry did not retain exactly one terminal write');
 report.checks.push('repeated exact input returns duplicate ACK and executes only once');
 await commune.getByTestId('commune-tab-history').click();
 await commune.getByTestId('commune-history').getByText('此工作区尚未取得已登记会话的协议记录。',{exact:false}).waitFor();
 await page.screenshot({path:join(root,'desktop-fit-and-discovery.png')});
 await page.evaluate(()=>window.closeOwnedCommune());
 await command('save_agent_profile_overrides',{overrides:[]});
 report.checks.push('actual sidebar observes the owned CLI automatically; protocol history remains separate');
 if(!process.argv.includes('--resize-only')){
 await typeCommand('echo GUI_BEFORE_REBUILD');await until(async()=>(await rendered())?.text.some(row=>row.trim()==='GUI_BEFORE_REBUILD'),'real GUI input did not reach the child');
 await typeCommand('exit');await until(async()=>{const state=await rendered();return state?.context&&state.context.terminalEpoch!==report.epochs[0];},'desktop did not display the replacement lifetime');
 report.epochs.push((await rendered()).context.terminalEpoch);
 await until(async()=>(await rendered())?.text.join('').toLowerCase().includes(`${allowed}>`.toLowerCase()),'replacement shell prompt missing');
 await typeCommand('echo GUI_AFTER_REBUILD');await until(async()=>(await rendered())?.text.some(row=>row.trim()==='GUI_AFTER_REBUILD'),'replacement GUI input failed');
 assert.equal(await page.getByTestId('terminal-exit-status').count(),0);await page.screenshot({path:join(root,'desktop-rebuilt.png')});report.checks.push('real desktop keyboard input and new-lifetime render after ordinary shell exit');
 await command('set_remote_enabled',{enabled:true,port:0});const info=await command('get_remote_info');const remotePort=await freePort();
 remoteVite=await launch(process.execPath,['node_modules/vite/bin/vite.js','--config','vite.remote.config.js','--host','127.0.0.1','--port',String(remotePort),'--strictPort'],{...env,RIDGE_REMOTE_DEV_HOST_URL:`https://127.0.0.1:${info.port}`},'remote-vite');
 const remoteUrl=`http://127.0.0.1:${remotePort}`;await until(async()=>{assert.equal(remoteVite.exitCode,null);try{return(await fetch(`${remoteUrl}/@vite/client`,{signal:AbortSignal.timeout(3000)})).ok;}catch{return false;}},'owned Remote Vite failed',60000);
 remoteBrowser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});remote=await remoteBrowser.newPage({viewport:{width:1100,height:800}});await remote.addInitScript(()=>performance.setResourceTimingBufferSize(2000));await remote.goto(remoteUrl);
 const auth=remote.locator('input[maxlength="6"]');await auth.waitFor();const fresh=await command('get_remote_info');await auth.fill(fresh.totpCode);await auth.press('Enter');
 await remote.locator('.term-stage').waitFor({timeout:30000});
 await until(async()=>(await renderedRemote())?.text.some(row=>row.trim()==='GUI_AFTER_REBUILD'),'Remote canonical output missing',60000);
 report.checks.push('real Remote auth and canonical terminal output through owned TLS Host');
 await typeCommand(`cd /d "${outside}"`);
 await until(async()=>(await rendered())?.text.join('').toLowerCase().includes(`${outside}>`.toLowerCase()),'owned child did not show the outside cwd prompt');
 assert.equal((await request(`/v1/domain/workspaces/${report.workspace}/topology`,{pane_tree:{root:{Leaf:report.pane},panes:{[report.pane]:{id:report.pane,mode:'Terminal',cwd:outside}}}},'PUT')).ok,true);
 await typeCommand('exit');
 await page.getByTestId('terminal-exit-status').filter({hasText:'Shell could not restart'}).waitFor();assert.equal(await page.getByTestId('terminal-exit-status').getAttribute('role'),'alert');
 await remote.getByTestId('terminal-exit-status').filter({hasText:'Shell could not restart'}).waitFor();
 const notice=await page.getByTestId('terminal-exit-status').evaluate(element=>{const css=getComputedStyle(element);return {color:css.color,background:css.backgroundColor};});
 assert.notEqual(notice.background,'rgba(0, 0, 0, 0)');assert.notEqual(notice.color,notice.background);
 await remote.reload();await remote.getByTestId('terminal-exit-status').filter({hasText:'Shell could not restart'}).waitFor({timeout:30000});
 await until(async()=>(await renderedRemote())?.text.some(row=>row.trim()==='GUI_AFTER_REBUILD'),'late Remote view lost retained output',60000);
 report.checks.push('late Remote attach retains canonical output after the child exited');
 await page.screenshot({path:join(root,'desktop-recovery-failed.png')});await remote.screenshot({path:join(root,'remote-recovery-failed.png')});report.checks.push('desktop and actual Remote render the Runtime recovery failure');
 const stopped=(await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===report.pane);assert.equal(stopped.status,'Exited');report.stoppedPid=stopped.child_pid;
 await browser.close();browser=null;await stopOwned(desktop,desktopBinary);desktop=null;
 assert.equal((await request('/v1/health',undefined,'GET')).pid,kernel.pid);await openDesktop();
 await page.getByTestId('terminal-exit-status').filter({hasText:'Shell could not restart'}).waitFor({timeout:30000});assert.equal((await request('/v1/domain/ptys',undefined,'GET')).ptys.find(pty=>pty.pty_id===report.pane).child_pid,report.stoppedPid);
 await page.screenshot({path:join(root,'desktop-reopened-stopped.png')});report.checks.push('reopened actual desktop renders retained stopped state without another child');
 }
 report.kernelPid=kernel.pid;report.binaryHashes={kernel:createHash('sha256').update(await readFile(kernelBinary)).digest('hex'),desktop:createHash('sha256').update(await readFile(desktopBinary)).digest('hex')};
 await writeFile(join(root,'result.json'),JSON.stringify(report,null,2));console.log(`PASS terminal UI: ${join(root,'result.json')}`);
}catch(error){
 try{await writeFile(join(root,'failure-terminal.json'),JSON.stringify({body:await page.locator('body').innerText(),rendered:await rendered(),recoveries:report.recoveries,ptys:await request('/v1/domain/ptys',undefined,'GET')},null,2));await page.screenshot({path:join(root,'failure.png')});}catch{}
 await writeFile(join(root,'failure.json'),JSON.stringify({message:String(error).replace(/([?&](?:token|code|auth)=)[^&\s]+/g,'$1<redacted>'),checks:report.checks},null,2));console.error(`FAIL terminal UI; evidence: ${root}`);process.exitCode=1;
}finally{
 try{await remoteBrowser?.close();}catch{}try{await browser?.close();}catch{}
 for(const [child,path]of[[desktop,desktopBinary],[remoteVite,process.execPath],[vite,process.execPath]])try{await stopOwned(child,path);}catch(error){console.error(`Owned cleanup failed: ${error.message}`);process.exitCode=1;}
 if(endpoint)try{await request('/v1/shutdown',{});}catch{}
 if(kernel.exitCode===null&&kernel.signalCode===null){await Promise.race([once(kernel,'exit'),delay(6000)]);if(kernel.exitCode===null&&kernel.signalCode===null){kernel.kill();}}
}
