// Actual Remote app -> development proxy -> owned TLS/WSS Kernel/files/Git.
// Delayed/failing observers are explicit fault injection; successful IO stays real.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { spawn, execFileSync } from 'node:child_process';
import { chromium, expect } from '@playwright/test';

const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-remote-tabs-'));
const evidenceDir=path.resolve('artifacts/remote086');fs.mkdirSync(evidenceDir,{recursive:true});
const evidence={scope:'Actual Remote dev app and owned current Kernel via real TLS/WSS proxy; Chromium software WebGL, no handset/WAN claim',checks:[],result:'RUNNING'};
const repo=path.join(root,'repo'),cwd=path.join(repo,'src');fs.mkdirSync(path.join(cwd,'nested'),{recursive:true});
const origin=path.join(root,'owned-origin.git');execFileSync('git',['init','--bare',origin],{windowsHide:true,stdio:'pipe'});
const git=(...args)=>execFileSync('git',args,{cwd:repo,windowsHide:true,stdio:'pipe'}).toString().trim();
git('init');git('config','user.name','Owned acceptance');git('config','user.email','owned@example.invalid');
git('remote','add','origin',origin);
const tracked=path.join(cwd,'tracked.txt');fs.writeFileSync(tracked,'BASE\n');git('add','.');git('commit','-m','owned base');
fs.writeFileSync(tracked,'INDEX_ONLY\n');git('add','.');fs.writeFileSync(tracked,'WORKTREE_ONLY\n');
fs.writeFileSync(path.join(cwd,'untracked.txt'),'UNTRACKED_CONTENT\n');
fs.writeFileSync(path.join(cwd,'nested','inner.txt'),'ORIGINAL_INNER\n');
fs.writeFileSync(path.join(cwd,'old.txt'),'OLD_NEEDLE\n');fs.writeFileSync(path.join(cwd,'new.txt'),'NEW_NEEDLE\n');
fs.writeFileSync(path.join(cwd,'large.txt'),Array.from({length:4800},(_,i)=>'LINE_'+(i+1)).join('\n'));
fs.writeFileSync(path.join(cwd,'image.svg'),'<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48"><rect width="48" height="48" fill="green"/></svg>');
const beforeHead=git('rev-parse','HEAD'),beforeIndex=git('diff','--cached');
// Historical component is an ignored benchmark fixture, not a production fallback.
const baseline=execFileSync('git',['show','780779a0:src/remote/lib/FileViewer.svelte'],{encoding:'utf8'}).replace("from './clipboard'",`from '${path.resolve('src/remote/lib/clipboard.ts').replaceAll('\\','/')}'`);
fs.writeFileSync(path.join(evidenceDir,'FileViewerBaseline.svelte'),baseline);
let kernel,vite,browser,endpoint,page;const pageErrors=[];
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const until=async(check,label='readiness')=>{const end=Date.now()+30000;while(Date.now()<end){if(await check())return;await delay(40);}throw Error(label+' deadline');};
const call=async(method,url,body)=>{const response=await fetch(`http://127.0.0.1:${endpoint.port}${url}`,{method,headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(10000)});assert.ok(response.ok);return response.json();};
const command=async(method,args={})=>{const response=await call('POST','/v1/domain/commands',{method,args,native:true,roots:[],connectionId:crypto.randomUUID()});assert.equal(response.ok,true,method+': '+response.error?.message);return response.value;};
const sourceUrl=relative=>'/@fs/'+path.resolve(relative).replaceAll('\\','/');
try {
  kernel=spawn(path.resolve('target/debug/ridge-kernel'+(process.platform==='win32'?'.exe':'')),[],{windowsHide:true,stdio:'ignore',env:{...process.env,RIDGE_KERNEL_DATA_DIR:root,RIDGE_AUTH_FILE:path.join(root,'auth.json'),RIDGE_KERNEL_FS_ROOT:root,RIDGE_TEST_ALLOW_NON_BREAKAWAY:'1'}});
  await until(()=>{try{endpoint=JSON.parse(fs.readFileSync(path.join(root,'kernel.json')));return endpoint.pid===kernel.pid;}catch{return false;}},'owned Kernel');
  const workspace=(await call('POST','/v1/domain/workspaces',{name:'Remote tabs'})).workspace_id;
  const pane=(await call('GET',`/v1/domain/workspaces/${workspace}`)).panes[0];
  await call('PUT',`/v1/domain/workspaces/${workspace}/topology`,{pane_tree:{root:{Leaf:pane},panes:{[pane]:{id:pane,mode:'Terminal',cwd}}}});
  const launched=await call('POST','/v1/domain/ptys',{pty_id:pane,workspace_id:workspace,program:process.platform==='win32'?'cmd.exe':'/bin/sh',args:process.platform==='win32'?['/d','/q','/k']:[],cwd,env:{},role:'shell',cols:100,rows:30});assert.equal(launched.ok,true);
  await command('set_remote_enabled',{enabled:true,port:0});const info=await command('get_remote_info');
  const port=await new Promise(resolve=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port));});});
  const log=fs.openSync(path.join(root,'vite.log'),'a');
  vite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--config','vite.remote.config.js'],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,RIDGE_REMOTE_DEV_PORT:String(port),RIDGE_REMOTE_DEV_HOST_URL:`https://127.0.0.1:${info.port}`}});fs.closeSync(log);
  const url=`http://127.0.0.1:${port}`;await until(()=>fetch(url,{signal:AbortSignal.timeout(1000)}).then(response=>response.ok,()=>false),'Remote dev');
  browser=await chromium.launch({headless:true,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
  page=await browser.newPage({viewport:{width:412,height:915}});page.on('pageerror',error=>pageErrors.push(error.message));
  await page.addInitScript(()=>{window.__RIDGE_E2E__=true;window.probeSockets=[];const Original=WebSocket;window.WebSocket=class extends Original{constructor(...args){super(...args);if(new URL(args[0],location.href).pathname==='/ws')window.probeSockets.push(this);}};});
  await page.goto(url);await page.locator('input[inputmode="numeric"]').waitFor();
  await page.evaluate(async moduleUrl=>{
    const {WsDataProvider}=await import(moduleUrl);window.probe={counts:{},gates:{},fail:{},pending:{},completed:{},calls:{}};
    for(const method of ['getFileTree','readFile','writeFile','gitStatus','gitGraph','gitDiffFile','searchFiles','gitStage','gitUnstage','gitCommit']){
      const original=WsDataProvider.prototype[method];WsDataProvider.prototype[method]=async function(...args){
        const key=method+':'+args[0];window.probe.counts[key]=(window.probe.counts[key]??0)+1;
        window.probe.dp=this;
        window.probe.calls[method]=args.slice(0,3);
        if(window.probe.fail[key]){delete window.probe.fail[key];throw Error('owned injected '+method+' failure');}
        if(window.probe.hold===key){window.probe.pending[key]=true;await new Promise(resolve=>window.probe.gates[key]=resolve);}
        const response=await original.apply(this,args);window.probe.completed[key]=true;return response;
      };
    }
  },sourceUrl('src/lib/transport/ws.ts'));
  await page.locator('input[inputmode="numeric"]').fill(info.totpCode);await page.locator('.card button').click();
  await page.locator('.hidden-input:not([disabled])').waitFor({timeout:40000});
  const rpcHits=await page.evaluate(async({moduleUrl,cwd})=>{const {TauriDataProvider}=await import(moduleUrl);return new TauriDataProvider().searchFiles('NEW_NEEDLE',cwd);},{moduleUrl:sourceUrl('src/lib/transport/tauri.ts'),cwd});
  assert.ok(rpcHits.some(hit=>hit.path.endsWith('new.txt')&&hit.snippet==='NEW_NEEDLE'));
  evidence.checks.push('The shared RPC DataProvider used by desktop/Cloud also searches the real current Kernel; no nested legacy request envelope');
  const files=()=>page.locator('.hdr-btn').nth(0).click();
  const closeViewer=()=>page.locator('.v-header button[aria-label]').click();
  const tab=async index=>page.locator('.sidebar .tab').nth(index).click();
  await files();await expect(page.locator('.ft-name').filter({hasText:/^tracked.txt$/})).toBeVisible();
  await page.evaluate(target=>window.probe.fail['getFileTree:'+target]=true,path.join(cwd,'nested'));
  await page.getByRole('button',{name:'nested',exact:true}).click();await expect(page.locator('.ft [role="alert"]')).toContainText('owned injected');
  await page.locator('.ft [role="alert"] button').click();await expect(page.locator('.ft-path')).toHaveText(path.join(cwd,'nested'));
  await page.getByRole('button',{name:'inner.txt',exact:true}).click();await expect(page.locator('.code')).toContainText('ORIGINAL_INNER');await closeViewer();
  await expect(page.locator('.ft-path')).toHaveText(path.join(cwd,'nested'));evidence.checks.push('Failed directory retries its attempted target; viewer return retains the successful nested directory');
  await tab(2);await page.locator('.search-input').fill('NEW_NEEDLE');await expect(page.locator('.hit-name')).toHaveText('new.txt');
  await page.locator('.hit').click();await expect(page.locator('.code-row.hit')).toContainText('NEW_NEEDLE');await closeViewer();
  await expect(page.locator('.search-input')).toHaveValue('NEW_NEEDLE');await expect(page.locator('.hit-name')).toHaveText('new.txt');
  evidence.checks.push('Search query/result and exact hit line survive viewer return without retyping');
  await page.evaluate(()=>window.probe.hold='searchFiles:OLD_NEEDLE');await page.locator('.search-input').fill('OLD_NEEDLE');
  await page.waitForFunction(()=>window.probe.pending['searchFiles:OLD_NEEDLE']);await page.locator('.search-input').fill('NEW_NEEDLE');
  await page.evaluate(()=>{window.probe.hold=undefined;window.probe.gates['searchFiles:OLD_NEEDLE']();});
  await page.waitForFunction(()=>window.probe.completed['searchFiles:OLD_NEEDLE']);
  await expect(page.locator('.hit-name')).toHaveText('new.txt');await expect(page.locator('.hit-name').filter({hasText:'old.txt'})).toHaveCount(0);
  await page.evaluate(()=>window.probe.fail['searchFiles:RETRY_NEEDLE']=true);await page.locator('.search-input').fill('RETRY_NEEDLE');
  await expect(page.locator('.search [role="alert"]')).toBeVisible();fs.writeFileSync(path.join(cwd,'retry.txt'),'RETRY_NEEDLE\n');
  await page.locator('.search [role="alert"] button').click();await expect(page.locator('.hit-name')).toHaveText('retry.txt');
  evidence.checks.push('Delayed obsolete search cannot replace the current query; failed search retries without retyping');
  await tab(1);await expect(page.locator('.file-row').filter({hasText:'src/tracked.txt'})).toHaveCount(2);
  await page.locator('.file-row').filter({hasText:'src/tracked.txt'}).first().click();await expect(page.locator('.diff-pre')).toContainText('INDEX_ONLY');await expect(page.locator('.diff-pre')).not.toContainText('WORKTREE_ONLY');await closeViewer();
  await page.locator('.file-row').filter({hasText:'src/tracked.txt'}).last().click();await expect(page.locator('.diff-pre')).toContainText('WORKTREE_ONLY');await closeViewer();
  await page.locator('.file-row').filter({hasText:'src/untracked.txt'}).click();await expect(page.locator('.code')).toContainText('UNTRACKED_CONTENT');await closeViewer();
  await tab(0);await page.locator('.ft-bar button').first().click();await page.getByRole('button',{name:'tracked.txt',exact:true}).click();
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-pencil')}).click();await page.locator('.editor').fill('EDITED_WORKING\n');
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.dirty-dot')).toHaveCount(0);await closeViewer();
  await page.getByRole('button',{name:'nested',exact:true}).click();await tab(1);await page.locator('.file-row').filter({hasText:'src/tracked.txt'}).last().click();await expect(page.locator('.diff-pre')).toContainText('EDITED_WORKING');await closeViewer();
  evidence.checks.push('Editing a file invalidates its cached working diff even though Git rows use relative paths; the index remains unchanged');
  evidence.checks.push('Nested-CWD Git uses actual repository root; staged/index and working-tree previews differ; untracked opens its file');
  await tab(2);await expect(page.locator('.hit-name')).toHaveText('retry.txt');await tab(1);await expect(page.locator('.file-row').filter({hasText:'src/tracked.txt'})).toHaveCount(2);
  const warm=await page.evaluate(()=>({...window.probe.counts}));for(let cycle=0;cycle<4;cycle++){await tab(0);await expect(page.locator('.ft-path')).toHaveText(path.join(cwd,'nested'));await tab(2);await expect(page.locator('.hit-name')).toHaveText('retry.txt');await tab(1);await expect(page.locator('.file-row').filter({hasText:'src/tracked.txt'})).toHaveCount(2);}
  await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
  assert.deepEqual(await page.evaluate(()=>window.probe.counts),warm);evidence.warmTabExtraRequests=0;
  evidence.checks.push('Four warm files/search/Git cycles add no file-tree, search, Git status or graph RPC');
  await tab(0);await page.getByRole('button',{name:'inner.txt',exact:true}).click();
  await page.locator('.v-header button[title]').filter({has:page.locator('svg.lucide-pencil')}).click();
  await page.locator('.editor').fill('SUBMITTED\n');const saveKey='writeFile:'+path.join(cwd,'nested','inner.txt');
  await page.evaluate(key=>window.probe.hold=key,saveKey);await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();
  await page.waitForFunction(key=>window.probe.pending[key],saveKey);await page.locator('.editor').fill('NEWER_UNSAVED\n');
  await expect(page.locator('.v-header button[aria-label]')).toBeDisabled();await page.evaluate(key=>{window.probe.hold=undefined;window.probe.gates[key]();},saveKey);
  await until(()=>fs.readFileSync(path.join(cwd,'nested','inner.txt'),'utf8')==='SUBMITTED\n','submitted file write');
  await expect(page.locator('.dirty-dot')).toBeVisible();await expect(page.locator('.editor')).toHaveValue('NEWER_UNSAVED\n');
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.dirty-dot')).toHaveCount(0);
  assert.equal(fs.readFileSync(path.join(cwd,'nested','inner.txt'),'utf8'),'NEWER_UNSAVED\n');
  await page.locator('.editor').fill('RETRY_BUFFER\n');await page.evaluate(key=>window.probe.fail[key]=true,saveKey);
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.v-saveerr')).toBeVisible();
  await expect(page.locator('.editor')).toHaveValue('RETRY_BUFFER\n');page.once('dialog',dialog=>dialog.dismiss());await closeViewer();await expect(page.locator('.editor')).toBeVisible();
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.dirty-dot')).toHaveCount(0);
  await page.locator('.editor').fill('OFFLINE_BUFFER\n');await page.context().setOffline(true);await page.evaluate(()=>window.probeSockets.at(-1).close());
  await page.locator('.conn-banner').waitFor();await expect(page.locator('.editor')).toHaveValue('OFFLINE_BUFFER\n');
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.v-saveerr')).toContainText('not connected');
  await page.context().setOffline(false);await page.evaluate(()=>window.dispatchEvent(new Event('online')));await page.locator('.conn-banner').waitFor({state:'detached',timeout:30000});
  await expect(page.locator('.editor')).toHaveValue('OFFLINE_BUFFER\n');await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();await expect(page.locator('.dirty-dot')).toHaveCount(0);await closeViewer();
  evidence.checks.push('Offline save fails immediately without transmission; editing survives reconnect and only a later explicit save writes the buffer');
  evidence.checks.push('Late save ACK confirms the submitted snapshot only; subsequent typing remains dirty and requires a second explicit save');
  await page.locator('.ft-bar button').first().click();await page.route('**/file?**',route=>route.abort());await page.getByRole('button',{name:'image.svg',exact:true}).click();
  await expect(page.locator('.v-msg[role="alert"]')).toBeVisible();await page.unroute('**/file?**');await page.locator('.v-msg[role="alert"] button').click();await expect(page.locator('.image-preview')).toBeVisible();
  await page.waitForFunction(()=>document.querySelector('.image-preview')?.naturalWidth===48);await closeViewer();
  const token=await page.evaluate(()=>localStorage.getItem('ridge_remote_token'));const validity=await page.request.get(url+'/session?token='+encodeURIComponent(token));assert.equal((await validity.json()).valid,true);
  evidence.checks.push('Development file/image and session proxy endpoints reach the authenticated real host');
  await page.evaluate(target=>window.probe.fail['readFile:'+target]=true,path.join(cwd,'large.txt'));await page.getByRole('button',{name:'large.txt',exact:true}).click();
  await expect(page.locator('.v-msg[role="alert"]')).toBeVisible();await page.locator('.v-msg[role="alert"] button').click();
  await expect(page.locator('.code-row')).toHaveCount(200);await page.locator('.v-more').click();await expect(page.locator('.code-row')).toHaveCount(400);
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-pencil')}).click();assert.ok((await page.locator('.editor').inputValue()).endsWith('LINE_4800'));await closeViewer();
  evidence.checks.push('Large-file read starts at 200 rows, extends on demand, retains complete content');
  assert.equal(git('rev-parse','HEAD'),beforeHead);assert.equal(git('diff','--cached'),beforeIndex,'preview must not mutate the index');
  const outside=path.join(root,'outside.txt');fs.writeFileSync(outside,'NOT_AUTHORIZED');
  const denied=await page.evaluate(async target=>{try{await window.probe.dp.readFile(target);return '';}catch(error){return String(error);}},outside);assert.match(denied,/outside|permitted|denied/i);
  evidence.checks.push('Real Runtime still rejects a file outside the permitted roots; all preceding previews leave HEAD and index unchanged');
  await tab(1);await page.getByRole('button',{name:/^(Stage all|暂存全部)$/}).click();
  await expect(page.locator('.git-panel [role="alert"],.git [role="alert"]')).toContainText('outside permitted');
  assert.equal(git('diff','--cached'),beforeIndex,'a subdirectory read grant must not authorize its parent Git index');
  // An explicit second owned source grants the repository, as normal Host scopes do.
  const grantWorkspace=(await call('POST','/v1/domain/workspaces',{name:'Owned repository grant'})).workspace_id;
  const grantPane=(await call('GET',`/v1/domain/workspaces/${grantWorkspace}`)).panes[0];
  const granted=await call('POST','/v1/domain/ptys',{pty_id:grantPane,workspace_id:grantWorkspace,program:process.platform==='win32'?'cmd.exe':'/bin/sh',args:process.platform==='win32'?['/d','/q','/k']:[],cwd:repo,env:{},role:'shell',cols:80,rows:24});assert.equal(granted.ok,true);
  await page.waitForFunction(async repo=>{try{return await window.probe.dp.pathExists(repo);}catch{return false;}},repo);
  await page.getByRole('button',{name:/^(Stage all|暂存全部)$/}).click();
  assert.equal(await page.evaluate(()=>window.probe.calls.gitStage[0]),cwd);
  await expect(page.locator('.file-row').filter({hasText:'src/tracked.txt'})).toHaveCount(1);await page.locator('.file-row').filter({hasText:'src/tracked.txt'}).click();
  await expect(page.locator('.diff-pre')).toContainText('EDITED_WORKING');await closeViewer();
  await page.getByRole('button',{name:/^(Unstage all|撤销暂存全部)$/}).click();await until(()=>git('diff','--cached')==='','real unstage');
  await page.getByRole('button',{name:/^(Stage all|暂存全部)$/}).click();await page.locator('.write-box textarea').fill('Owned Remote acceptance');
  page.once('dialog',dialog=>dialog.dismiss());await page.locator('.commit-btn').click();assert.equal(git('rev-parse','HEAD'),beforeHead);
  page.once('dialog',dialog=>dialog.accept());await page.locator('.commit-btn').click();await until(()=>git('rev-parse','HEAD')!==beforeHead,'real explicit commit');
  page.once('dialog',dialog=>dialog.accept());await page.getByRole('button',{name:'Push',exact:true}).click();
  const pushed=git('rev-parse','HEAD'),branch=git('branch','--show-current');
  await until(()=>{try{return execFileSync('git',['--git-dir',origin,'rev-parse','refs/heads/'+branch],{stdio:'pipe'}).toString().trim()===pushed;}catch{return false;}},'owned local-origin push');
  await page.locator('.view-btn').last().click();await expect(page.locator('.graph-row').first()).toContainText('Owned Remote acceptance');
  evidence.checks.push('Explicit stage/unstage refresh both diff comparisons; declined commit writes nothing; accepted commit, local-origin push and lazy graph work from the authorized nested context');
  await page.screenshot({path:path.join(evidenceDir,'remote-files.png')});
  await page.evaluate(async({clientUrl,viewerUrl,providerUrl,cwd,target})=>{
    const [client,{default:Viewer},{createWsSidebarProvider}]=await Promise.all([import(clientUrl),import(viewerUrl),import(providerUrl)]);
    const container=document.createElement('main');document.body.append(container);const provider=createWsSidebarProvider(cwd,window.probe.dp);
    window.scopeProbe={container,provider,app:client.createClassComponent({component:Viewer,target:container,props:{provider,kind:'file',path:target,onClose(){}}})};
  },{clientUrl:sourceUrl('scripts/commune-permission-ui-client.mjs'),viewerUrl:sourceUrl('src/remote/lib/FileViewer.svelte'),providerUrl:sourceUrl('src/remote/lib/sidebarProvider.ts'),cwd,target:path.join(cwd,'nested','inner.txt')});
  await page.locator('.v-header button').filter({has:page.locator('svg.lucide-pencil')}).click();await page.locator('.editor').fill('CAPTURED_TARGET_A\n');
  await page.evaluate(key=>{window.probe.hold=key;delete window.probe.pending[key];},saveKey);await page.locator('.v-header button').filter({has:page.locator('svg.lucide-save')}).click();
  await page.waitForFunction(key=>window.probe.pending[key],saveKey);await page.evaluate(target=>window.scopeProbe.app.$set({path:target}),path.join(cwd,'new.txt'));
  await expect(page.locator('.code')).toContainText('NEW_NEEDLE');await page.locator('.v-header button').filter({has:page.locator('svg.lucide-pencil')}).click();await page.locator('.editor').fill('UNSAVED_TARGET_B\n');
  await page.evaluate(key=>{window.probe.hold=undefined;window.probe.gates[key]();},saveKey);await until(()=>fs.readFileSync(path.join(cwd,'nested','inner.txt'),'utf8')==='CAPTURED_TARGET_A\n','captured target save');
  await expect(page.locator('.dirty-dot')).toBeVisible();await expect(page.locator('.editor')).toHaveValue('UNSAVED_TARGET_B\n');assert.equal(fs.readFileSync(path.join(cwd,'new.txt'),'utf8'),'NEW_NEEDLE\n');
  await page.evaluate(()=>{window.scopeProbe.app.$destroy();window.scopeProbe.container.remove();});evidence.checks.push('Late successful save to target A cannot clear or write the unsaved buffer of newly opened target B');
  const plain=path.join(root,'plain');fs.mkdirSync(plain);
  const plainWorkspace=(await call('POST','/v1/domain/workspaces',{name:'Owned non-Git source'})).workspace_id;
  const plainPane=(await call('GET',`/v1/domain/workspaces/${plainWorkspace}`)).panes[0];
  assert.equal((await call('POST','/v1/domain/ptys',{pty_id:plainPane,workspace_id:plainWorkspace,program:process.platform==='win32'?'cmd.exe':'/bin/sh',args:process.platform==='win32'?['/d','/q','/k']:[],cwd:plain,env:{},role:'shell',cols:80,rows:24})).ok,true);
  await page.locator('.sidebar .close').click();await page.locator('.tree-trigger').click();await page.locator('.ws-row').filter({hasText:'Owned non-Git source'}).click();await page.locator('.tree-backdrop').click({position:{x:5,y:5}});
  await files();await expect(page.locator('.ft-path')).toHaveText(plain);await tab(2);await expect(page.locator('.search-input')).toHaveValue('');await tab(1);
  await expect(page.locator('.git .msg')).toContainText(/不是.*Git|不在.*Git|not a git/i);
  execFileSync('git',['init',plain],{windowsHide:true,stdio:'pipe'});
  await page.locator('.git button').filter({has:page.locator('svg.lucide-refresh-cw')}).click();await expect(page.locator('.write-box')).toBeVisible();
  evidence.checks.push('Changing workspace resets directory/search scope; explicit Git refresh detects a repository initialized after a cached non-Git result');
  await tab(3);await expect(page.getByTestId('global-commune-roster')).toBeVisible();await expect(page.getByTestId('host-commune-groups')).toHaveCount(1);await expect(page.locator('.sidebar [role="alert"]')).toHaveCount(0);
  evidence.checks.push('Commune still renders its single Host-global group authority after a source switch');
  // Fixed in-memory text isolates rendering cost from network/cache timing.
  const benchmark=await page.evaluate(async({baselineUrl,viewerUrl,clientUrl})=>{
    const client=await import(clientUrl),[{default:Before},{default:After}]=await Promise.all([import(baselineUrl),import(viewerUrl)]);
    const target=document.createElement('main');document.body.append(target);const text=Array.from({length:4800},(_,i)=>'LINE_'+(i+1)).join('\n');const result={before:[],after:[]};
    for(let run=0;run<6;run++)for(const [name,component]of[['before',Before],['after',After]]){
      const start=performance.now();const app=client.createClassComponent({component,target,props:{provider:{readFile:async()=>text},kind:'file',path:'/owned/large.txt',onClose(){}}});
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));result[name].push({ms:performance.now()-start,rows:target.querySelectorAll('.code-row').length});app.$destroy();
    }target.remove();return result;
  },{baselineUrl:sourceUrl('artifacts/remote086/FileViewerBaseline.svelte'),viewerUrl:sourceUrl('src/remote/lib/FileViewer.svelte'),clientUrl:sourceUrl('scripts/commune-permission-ui-client.mjs')});
  assert.ok(benchmark.before.every(sample=>sample.rows===4800));assert.ok(benchmark.after.every(sample=>sample.rows===200));evidence.renderBenchmark=benchmark;
  assert.deepEqual(pageErrors,[]);evidence.result='PASS';
}catch(error){evidence.result='FAIL';evidence.error=String(error);evidence.pageErrors=pageErrors;if(page){evidence.calls=await page.evaluate(()=>window.probe?.calls).catch(()=>undefined);evidence.pathProof=await page.evaluate(async()=>{const call=window.probe?.calls.gitStage;if(!call)return;return Promise.all([call[0],...call[1]].map(async path=>{try{return {path,exists:await window.probe.dp.pathExists(path)};}catch(error){return {path,error:String(error)};}}));}).catch(()=>undefined);evidence.visibleError=await page.locator('[role="alert"],.v-msg').allTextContents().catch(()=>[]);await page.screenshot({path:path.join(evidenceDir,'failure.png')}).catch(()=>{});}process.exitCode=1;}
finally{
  await browser?.close();if(endpoint?.pid===kernel?.pid)await call('POST','/v1/shutdown').catch(()=>{});
  if(vite?.pid)await new Promise(resolve=>{const child=process.platform==='win32'?spawn('taskkill',['/PID',String(vite.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'}):spawn('kill',[String(vite.pid)],{stdio:'ignore'});child.once('exit',resolve);});
  evidence.fixtureDirectory=root;fs.writeFileSync(path.join(evidenceDir,'acceptance.json'),JSON.stringify(evidence,null,2));console.log(JSON.stringify({result:evidence.result,evidence:path.join(evidenceDir,'acceptance.json'),error:evidence.error}));
}
