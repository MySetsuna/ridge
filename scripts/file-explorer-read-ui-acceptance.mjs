// Production Explorer/FileTree and native WebSocket; all filesystem data is owned.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';

const output=path.resolve(process.env.RIDGE_EXPLORER_EVIDENCE_DIR || 'artifacts/file-tree-read-recovery');
fs.mkdirSync(output,{recursive:true});
const root=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-explorer-read-'));
const outside=fs.mkdtempSync(path.join(os.tmpdir(),'ridge-explorer-outside-'));
const allowed=path.join(root,'workspace'),missing=path.join(root,'not-created');
fs.mkdirSync(allowed);fs.mkdirSync(path.join(root,'workspaces'));
fs.writeFileSync(path.join(allowed,'allowed.txt'),'owned read fixture');
fs.writeFileSync(path.join(outside,'outside.txt'),'must remain inaccessible');
const evidence={scope:'Production Explorer/FileTree with private Kernel/native grant and owned directories; no existing app, PTY or Agent used',checks:[],result:'RUNNING'};
let kernel,endpoint,server,browser,page;
const call=async(method,url,body)=>{
  const response=await fetch(`http://127.0.0.1:${endpoint.port}${url}`,{method,
    headers:{'x-ridge-kernel-token':endpoint.token,'content-type':'application/json'},
    body:body ? JSON.stringify(body) : undefined,signal:AbortSignal.timeout(8000)});
  assert.ok(response.ok,`Private Kernel HTTP ${response.status}`);return response.json();
};
try {
  process.env.RIDGE_NATIVE_RUNTIME='1';process.env.RIDGE_WEB_REMOTE='';
  server=await createServer({cacheDir:path.join(output,'vite-cache'),
    optimizeDeps:{entries:['scripts/commune-permission-ui-client.mjs','src/lib/components/Explorer.svelte','src/lib/components/FileTree.svelte']},
    plugins:[{name:'owned-explorer-fixture',enforce:'post',configResolved(config){config.server.watch=null;config.server.hmr=false;}}],
    server:{host:'127.0.0.1',port:0,strictPort:false,watch:null,hmr:false}});
  await server.listen();const origin=`http://127.0.0.1:${server.httpServer.address().port}`;
  const binary=path.resolve(process.env.RIDGE_TEST_KERNEL_BINARY || 'target/debug/ridge-kernel.exe');
  assert.ok(fs.existsSync(binary),'Build the current Kernel first');
  kernel=spawn(binary,[],{cwd:root,windowsHide:true,stdio:'ignore',env:{...process.env,
    RIDGE_KERNEL_DATA_DIR:path.join(root,'kernel'),RIDGE_AUTH_FILE:path.join(root,'auth.json'),
    RIDGE_KERNEL_FS_ROOT:root,RIDGE_WORKSPACE_SAVE_DIR:path.join(root,'workspaces')}});
  const deadline=Date.now()+20000;
  while(Date.now()<deadline){
    assert.equal(kernel.exitCode,null,'Private Kernel exited');
    try{endpoint=JSON.parse(fs.readFileSync(path.join(root,'kernel/kernel.json'),'utf8'));if(endpoint.pid===kernel.pid)break;}catch{}
    await new Promise(resolve=>setTimeout(resolve,50));
  }
  assert.equal(endpoint?.pid,kernel.pid,'Private Kernel readiness');
  const workspaceId=(await call('POST','/v1/domain/workspaces')).workspace_id;
  const paneId=(await call('GET',`/v1/domain/workspaces/${workspaceId}`)).panes[0];
  const grant=await call('POST','/v1/local/clients',{deviceId:'owned-explorer-read',origin});
  browser=await chromium.launch({headless:true});page=await browser.newPage({viewport:{width:400,height:800}});
  const pageErrors=[];page.on('pageerror',error=>pageErrors.push(error.message));
  await page.route('**/__explorer_read',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><body style="margin:0;height:100vh;font-family:system-ui,sans-serif;background:var(--rg-bg);color:var(--rg-fg)"><main id="component" style="height:100%"></main></body>'}));
  await page.exposeFunction('ownedExplorerGrant',()=>grant);
  await page.goto(origin+'/__explorer_read');
  await page.evaluate(async({workspaceId,paneId,outside})=>{
    window.isTauri=true;window.__TAURI_INTERNALS__={invoke:command=>{
      if(command==='bootstrap_runtime_client')return window.ownedExplorerGrant();throw Error('Unexpected OS business call: '+command);
    }};
    const [{connectPresentationRuntime},{bridge},{invoke},client,{default:Explorer},{default:FileTree},paneTree,store,issue,locale,themes,settings]=await Promise.all([
      import('/src/lib/transport/runtimeClient.ts'),import('/src/lib/transport/tauriShim/bridge.ts'),
      import('/src/lib/transport/nativeRuntimeCore.ts'),import('/scripts/commune-permission-ui-client.mjs'),
      import('/src/lib/components/Explorer.svelte'),import('/src/lib/components/FileTree.svelte'),
      import('/src/lib/stores/paneTree.ts'),import('/src/lib/stores/fileExplorer.ts'),import('/src/lib/devIssue.ts'),
      import('/src/lib/i18n/locale.ts'),import('/src/lib/stores/themes.ts'),import('/src/lib/stores/settings.ts')]);
    const connection=await connectPresentationRuntime();bridge.attach(connection.transport,{useGlobalWorkspace:false});
    await themes.initThemeSystem();settings.applyTheme(themes.getThemeIds()[0]);locale.setLocale('zh');
    const control={reads:[],commands:[],holdNext:false,heldReady:false,release:()=>{}};
    const nativeInvoke=bridge.invoke.bind(bridge);
    bridge.invoke=(method,args,...rest)=>{
      control.commands.push(method);
      if(method!=='get_file_tree')return nativeInvoke(method,args,...rest);
      control.reads.push(args.path);
      const pending=nativeInvoke(method,args,...rest);
      if(!control.holdNext)return pending;
      control.holdNext=false;control.heldReady=false;
      const held=new Promise(resolve=>{control.release=resolve;});
      return pending.then(async value=>{control.heldReady=true;await held;return value;},async error=>{control.heldReady=true;await held;throw error;});
    };
    let devIssue=null;issue.devIssue.subscribe(value=>{devIssue=value;});
    paneTree.workspacesList.set([{id:workspaceId,index:0,displaySeq:1,name:'Read recovery'}]);
    paneTree.activeWorkspaceId.set(workspaceId);paneTree.activePaneId.set(paneId);
    paneTree.paneCwdStore.set({[`${workspaceId}:${paneId}`]:outside});
    const app=client.mount(Explorer,{target:document.getElementById('component'),props:{workspaceId}});
    window.explorerProbe={app,FileTree,client,connection,invoke,paneTree,store,control,locale,workspaceId,paneId,
      issue:()=>devIssue,setCwd:cwd=>paneTree.paneCwdStore.set({[`${workspaceId}:${paneId}`]:cwd})};
  },{workspaceId,paneId,outside});
  const initialDocument=await page.evaluate(()=>performance.timeOrigin);
  const error=page.getByTestId('explorer-read-error');
  await expect(error).toHaveAttribute('data-kind','outside_sandbox');
  await expect(error).toContainText(outside);
  assert.equal(await page.getByText('空目录',{exact:true}).count(),0);
  const reads=await page.evaluate(()=>window.explorerProbe.control.reads.length);
  await page.evaluate(async()=>{
    const p=window.explorerProbe;p.paneTree.workspacesList.update(list=>list.map(ws=>({...ws})));
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
  });
  assert.equal(await page.evaluate(()=>window.explorerProbe.control.reads.length),reads,'Failure must not auto-loop');
  evidence.checks.push('Real outside_sandbox stays local, shows the requested directory and never claims an empty directory or loops');

  await page.evaluate(()=>{window.explorerProbe.control.holdNext=true;});
  await error.getByRole('button',{name:'重试读取',exact:true}).dblclick();
  await expect.poll(()=>page.evaluate(()=>window.explorerProbe.control.heldReady)).toBe(true);
  await expect(error.getByRole('button')).toBeDisabled();
  assert.equal(await page.evaluate(()=>window.explorerProbe.control.reads.length),reads+1,'Double click must share the same read');
  await page.evaluate(()=>window.explorerProbe.control.release());
  await expect(error.getByRole('button')).toBeEnabled();
  await expect(error).toHaveAttribute('data-kind','outside_sandbox');
  evidence.checks.push('Explicit duplicate retry issues one real read, retains the error while pending and cannot expand host access');

  await page.evaluate(()=>window.explorerProbe.locale.setLocale('en'));
  await expect(error).toContainText('outside the host');
  await expect(error.getByRole('button',{name:'Retry reading',exact:true})).toBeVisible();
  assert.equal(await error.evaluate(el=>el.scrollWidth>el.clientWidth),false,'English error must fit its narrow column');
  await page.screenshot({path:path.join(output,'denied-en.png')});
  await page.evaluate(()=>window.explorerProbe.locale.setLocale('zh'));
  assert.equal(await error.evaluate(el=>el.scrollWidth>el.clientWidth),false,'Chinese error must fit its narrow column');
  await page.screenshot({path:path.join(output,'denied-zh.png')});
  evidence.checks.push('Chinese and English explanations, full path and keyboard-focusable retry fit a narrow column');

  await page.evaluate(cwd=>window.explorerProbe.setCwd(cwd),missing);
  await expect(error).toHaveAttribute('data-kind','missing');
  assert.equal(fs.existsSync(missing),false,'Directory reads must not create missing directories');
  fs.mkdirSync(missing);fs.writeFileSync(path.join(missing,'recovered.txt'),'explicit owned test setup');
  await error.getByRole('button',{name:'重试读取',exact:true}).click();
  await expect(error).toHaveCount(0);await expect(page.getByText('recovered.txt',{exact:true})).toBeVisible();
  evidence.checks.push('A real missing directory is distinguished from denial; explicit retry displays actual contents after the test owner creates it');

  await page.evaluate(cwd=>window.explorerProbe.setCwd(cwd),allowed);
  await expect(page.getByText('allowed.txt',{exact:true})).toBeVisible();
  await page.evaluate(cwd=>{
    const p=window.explorerProbe;p.control.holdNext=true;
    p.oldRead=p.store.fileExplorerStore.loadTree(`${p.workspaceId}:${cwd}`);
  },allowed);
  await expect.poll(()=>page.evaluate(()=>window.explorerProbe.control.heldReady)).toBe(true);
  fs.writeFileSync(path.join(allowed,'fresh.txt'),'new snapshot after held read');
  await page.evaluate(async cwd=>{
    const p=window.explorerProbe;p.store.fileExplorerStore.reset();
    p.store.fileExplorerStore.syncWithPaneCwds(p.workspaceId,{[p.paneId]:cwd});
    await p.store.fileExplorerStore.loadTree(`${p.workspaceId}:${cwd}`);
  },allowed);
  await expect(page.getByText('fresh.txt',{exact:true})).toBeVisible();
  await page.evaluate(async()=>{const p=window.explorerProbe;p.control.release();await p.oldRead;});
  await expect(page.getByText('fresh.txt',{exact:true})).toBeVisible();
  await expect(error).toHaveCount(0);
  evidence.checks.push('Real stale filesystem snapshot cannot replace a freshly rebuilt same-path column');

  await page.evaluate(outside=>{
    const p=window.explorerProbe;const target=document.createElement('section');target.id='restricted-child';document.body.append(target);
    p.child=p.client.mount(p.FileTree,{target,props:{columnId:'owned-child',node:{name:'Restricted child',path:outside,is_dir:true},depth:0,expandedPaths:new Set([outside]),selectedPath:null,selectedPaths:new Set(),refreshNonce:0}});
  },outside);
  const child=page.locator('#restricted-child');
  await expect(child.getByText('加载失败，请重试',{exact:true})).toBeVisible();
  await child.getByText('加载失败，请重试',{exact:true}).click();
  await expect(child.getByText('加载失败，请重试',{exact:true})).toBeVisible();
  assert.equal(await page.evaluate(()=>window.explorerProbe.issue()),null);
  assert.deepEqual(pageErrors,[]);
  assert.equal(await page.evaluate(()=>performance.timeOrigin),initialDocument,'Recovery must not reload the page');
  const workspaces=await call('GET','/v1/domain/workspaces');
  assert.equal(workspaces.workspaces.length,1);
  const ptys=await call('GET','/v1/domain/ptys');
  assert.equal(ptys.ptys.length,0);
  assert.deepEqual(fs.readdirSync(path.join(root,'workspaces')),[]);
  evidence.checks.push('Child denial uses existing FileTree retry; no global issue, page reload, PTY, workspace or saved-workspace side effect');
  await page.locator('#component').screenshot({path:path.join(output,'recovered.png')});
  evidence.result='PASS';
}catch(error){
  evidence.result='FAIL';evidence.error=error.stack || String(error);process.exitCode=1;
  if(page){evidence.body=await page.locator('body').innerText().catch(()=>'(unavailable)');await page.screenshot({path:path.join(output,'failure.png')}).catch(()=>{});}
}finally{
  await browser?.close();await server?.close();
  if(kernel?.exitCode===null && endpoint?.pid===kernel.pid)kernel.kill();
  fs.writeFileSync(path.join(output,'ui.json'),JSON.stringify(evidence,null,2));
}
console.log(JSON.stringify(evidence,null,2));
