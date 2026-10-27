// Real Chromium + production native RPC/event facades + owned existing ACP fixture.
// Native grant issuance is supplied by the owned fixture; no external Agent claim.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import net from 'node:net';
import { chromium } from '@playwright/test';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-070-permission-ui-'));
const kernelBinary = path.resolve(process.env.RIDGE_TEST_KERNEL_BINARY || 'target/debug/ridge-kernel.exe');
const fixtureBinary = process.env.RIDGE_TEST_COMMUNE_BINARY ? path.resolve(process.env.RIDGE_TEST_COMMUNE_BINARY)
  : fs.readdirSync('target/debug/deps').filter(name => /^commune_registration-[a-f0-9]+(?:\.exe)?$/.test(name))
    .map(name => path.resolve('target/debug/deps', name)).sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];
assert.ok(fixtureBinary, 'build the commune_registration integration fixture first');
let devUrl = process.env.RIDGE_TEST_DEV_URL;
const membershipOnly=process.env.RIDGE_TEST_MEMBERSHIP_ONLY==='1';
const output = path.resolve(membershipOnly?'.webview2-dev-cdp/070-commune-membership-ui.json':'.webview2-dev-cdp/070-commune-permission-ui.json');
const screenshot = path.resolve('.webview2-dev-cdp/070-commune-permission-ui.png');
const evidence = { scope: 'Owned Kernel/PTY ACP protocol fixture + real Chromium production approval component; fixture supplies OS bootstrap grant', completeChange: false, checks: [] };
let endpoint, browser, page, kernel, vite, paneId, identity;
const pageErrors = [];
const until = async condition => {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) { if (await condition()) return; await new Promise(resolve => setTimeout(resolve, 25)); }
  throw Error('owned fixture readiness timeout');
};
const call = async (method, url, body) => {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}${url}`, {
    method, headers: { 'x-ridge-kernel-token': endpoint.token, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(6000),
  });
  assert.ok(response.ok, `Kernel HTTP ${response.status}`);
  return response.json();
};
try {
  if (!devUrl) {
    const port = await new Promise((resolve, reject) => {
      const listener = net.createServer(); listener.on('error', reject);
      listener.listen(0, '127.0.0.1', () => { const port = listener.address().port; listener.close(() => resolve(port)); });
    });
    devUrl = `http://127.0.0.1:${port}`;
    const log = fs.openSync(path.join(root, 'vite.log'), 'a');
    vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js'], { windowsHide: true,
      env: { ...process.env, RIDGE_WEB_REMOTE: '', RIDGE_NATIVE_RUNTIME: '1', RIDGE_DEV_SERVER_PORT: String(port) }, stdio: ['ignore', log, log] });
    fs.closeSync(log);
    await until(async () => { assert.equal(vite.exitCode, null, 'owned Vite exited'); return fetch(devUrl + '/@vite/client', { signal: AbortSignal.timeout(1000) }).then(response => response.ok, () => false); });
  }
  assert.ok(['localhost', '127.0.0.1'].includes(new URL(devUrl).hostname), 'owned test needs a local Vite server');
  assert.ok((await fetch(devUrl + '/@vite/client', { signal: AbortSignal.timeout(5000) })).ok, 'local Vite server must be running');
  kernel = spawn(kernelBinary, [], { windowsHide: true, env: { ...process.env, RIDGE_KERNEL_DATA_DIR: root, RIDGE_AUTH_FILE: path.join(root, 'auth.json') }, stdio: 'ignore' });
  await until(() => {
    assert.equal(kernel.exitCode, null, 'owned Kernel exited');
    if (!fs.existsSync(path.join(root, 'kernel.json'))) return false;
    endpoint = JSON.parse(fs.readFileSync(path.join(root, 'kernel.json'))); return true;
  });
  assert.equal(endpoint.pid, kernel.pid);
  const workspaceId = (await call('POST', '/v1/domain/workspaces')).workspace_id;
  paneId = crypto.randomUUID();
  const descriptorPath = path.join(root, 'existing-endpoint.json');
  await call('POST', '/v1/domain/ptys', { pty_id: paneId, workspace_id: workspaceId, program: fixtureBinary,
    args: ['--exact', 'existing_protocol_endpoint_fixture', '--ignored'], cwd: root,
    env: { RIDGE_COMMUNE_FIXTURE: descriptorPath }, role: 'shell', cols: 80, rows: 24 });
  await until(() => fs.existsSync(descriptorPath));
  const descriptor = JSON.parse(fs.readFileSync(descriptorPath));
  const registered = await call('POST', '/v1/domain/commune/register', { workspaceId, paneId,
    process: descriptor.process, endpointOwner: descriptor.process, endpoint: descriptor.endpoint, sessionId: descriptor.sessionId, cwd: root });
  assert.equal(registered.ok, true, registered.error);
  identity = registered.value;
  const secondRoot = path.join(root,'second'); fs.mkdirSync(secondRoot);
  const secondDescriptorPath = path.join(secondRoot,'endpoint.json');
  const secondPane = crypto.randomUUID();
  await call('POST','/v1/domain/ptys',{pty_id:secondPane,workspace_id:workspaceId,program:fixtureBinary,
    args:['--exact','existing_protocol_endpoint_fixture','--ignored'],cwd:secondRoot,
    env:{RIDGE_COMMUNE_FIXTURE:secondDescriptorPath,RIDGE_COMMUNE_LABEL:'custom-process-two'},role:'shell',cols:80,rows:24});
  await until(() => fs.existsSync(secondDescriptorPath));
  const secondDescriptor = JSON.parse(fs.readFileSync(secondDescriptorPath));
  const secondRegistered = await call('POST','/v1/domain/commune/register',{workspaceId,paneId:secondPane,
    process:secondDescriptor.process,endpointOwner:secondDescriptor.process,endpoint:secondDescriptor.endpoint,sessionId:secondDescriptor.sessionId,cwd:secondRoot});
  assert.equal(secondRegistered.ok,true,secondRegistered.error);
  const grant = await call('POST', '/v1/local/clients', { deviceId: 'owned-approval-window', origin: new URL(devUrl).origin });
  browser = await chromium.launch({ headless: true });
  page = await browser.newPage({ viewport: { width: 900, height: 700 } });
  page.setDefaultTimeout(15000);
  await page.route('**/__commune_permission_ui', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Owned Commune permission UI</title><link rel="stylesheet" href="/src/app.css"><body><main id="component" class="rg-commune"></main></body>' }));
  await page.exposeFunction('fixtureGrant', () => grant);
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.goto(devUrl + '/__commune_permission_ui', { waitUntil: 'domcontentloaded', timeout: 45000 });
  const target = { workspaceId, paneId, agentId: identity.agentId, generation: identity.generation, lease: identity.lease };
  await page.evaluate(async target => {
    const osCalls = [];
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = { invoke: async command => {
      osCalls.push(command); if (command === 'bootstrap_runtime_client') return window.fixtureGrant();
      throw Error('Unexpected OS business call: ' + command);
    }, transformCallback: () => { throw Error('Unexpected native Channel'); } };
    const [{ connectPresentationRuntime }, { bridge }, { invoke }, { mount, unmount }, { default: Modal }, { default: History }, { default: Groups }, { parseTopologySnapshot }, { teammateGroupStore }, { default: WorkspaceTabs }] = await Promise.all([
      import('/src/lib/transport/runtimeClient.ts'), import('/src/lib/transport/tauriShim/bridge.ts'),
      import('/src/lib/transport/nativeRuntimeCore.ts'), import('/scripts/commune-permission-ui-client.mjs'),
      import('/src/lib/teammate/HitlApprovalModal.svelte'),
      import('/src/lib/teammate/CommuneHistory.svelte'),
      import('/src/lib/teammate/TeammateGroupsSection.svelte'), import('/src/lib/teammate/teammateModel.ts'),
      import('/src/lib/teammate/teammateGroups.svelte.ts'),
      import('/src/lib/components/WorkspaceTabs.svelte'),
    ]);
    await import('/src/lib/teammate/AgentCenterPanel.svelte'); // load the production Commune visual tokens
    let connection = await connectPresentationRuntime();
    bridge.attach(connection.transport, { useGlobalWorkspace: false });
    const [{ initThemeSystem, getThemeIds }, { applyTheme }] = await Promise.all([
      import('/src/lib/stores/themes.ts'), import('/src/lib/stores/settings.ts'),
    ]);
    await initThemeSystem(); applyTheme(getThemeIds()[0]);
    const modal = mount(Modal, { target: document.getElementById('component') });
    const historyTarget = document.createElement('section'); historyTarget.className = 'rg-commune'; document.body.append(historyTarget);
    const history = mount(History, { target: historyTarget, props: { workspaceId: target.workspaceId } });
    const groupTarget = document.createElement('section'); groupTarget.className = 'rg-commune'; groupTarget.dataset.testid = 'commune-groups'; document.body.append(groupTarget);
    const topology = parseTopologySnapshot(await invoke('get_teammate_topology', {workspaceId:target.workspaceId}));
    const groups = mount(Groups, {target:groupTarget, props:{workspaceId:target.workspaceId,filePath:null,roster:topology.roster}});
    const workspaceTabTarget=document.createElement('section'); workspaceTabTarget.className='rg-commune'; document.body.append(workspaceTabTarget);
    const workspaceTabs=mount(WorkspaceTabs,{target:workspaceTabTarget,props:{
      workspaces:[{id:'saved-local',index:0,name:'Ridge',displaySeq:1},{id:'temporary',index:1,name:'Scratch',displaySeq:2}],
      activeWorkspaceId:'saved-local',savedWorkspaceIds:['saved-local'],unreadWorkspaceIds:['temporary'],
      onSwitch:()=>{},onClose:()=>{},onReorder:()=>{},onRename:()=>{},
    }});
    await new Promise(resolve=>requestAnimationFrame(()=>resolve()));
    if(workspaceTabTarget.querySelectorAll('[data-testid="workspace-tab-saved"]').length!==1 || workspaceTabTarget.querySelectorAll('[data-testid="workspace-agent-unread"]').length!==1) throw Error('workspace tab does not distinguish the saved workspace from the unread temporary workspace');
    window.ownedGroups = teammateGroupStore(target.workspaceId);
    window.ownedApproval = { invoke, target, osCalls,
      pause: () => { bridge.detach({ preserveListeners: true }); connection.disconnect(); },
      reconnect: async () => { connection = await connectPresentationRuntime(); bridge.attach(connection.transport, { useGlobalWorkspace: false }); },
      dispose: async () => { await unmount(workspaceTabs); await unmount(groups); await unmount(history); await unmount(modal); bridge.detach(); connection.disconnect(); } };
  }, target);
  if(membershipOnly){
    evidence.scope='Owned existing ACP process + production connection/removal dialogs + actual Runtime RPC; no external Agent claim';
    const memberRoot=path.join(root,'membership');fs.mkdirSync(memberRoot);
    const memberPane=crypto.randomUUID(),memberDescriptorPath=path.join(memberRoot,'endpoint.json');
    await call('POST','/v1/domain/ptys',{pty_id:memberPane,workspace_id:workspaceId,program:fixtureBinary,
      args:['--exact','existing_protocol_endpoint_fixture','--ignored'],cwd:memberRoot,
      env:{RIDGE_COMMUNE_FIXTURE:memberDescriptorPath,RIDGE_COMMUNE_LABEL:'unlisted-owned-member'},role:'shell',cols:80,rows:24});
    await until(()=>fs.existsSync(memberDescriptorPath));
    const descriptor=JSON.parse(fs.readFileSync(memberDescriptorPath));
    const description={process:descriptor.process,endpointOwner:descriptor.process,endpoint:descriptor.endpoint,sessionId:descriptor.sessionId,cwd:memberRoot};
    await page.evaluate(async({workspaceId,paneId})=>{
      const [{default:Dialog},{registerCommuneMember,removeCommuneMember},{parseTopologySnapshot},{mount,unmount}]=await Promise.all([
        import('/src/lib/components/RidgeDialog.svelte'),import('/src/lib/teammate/communeMembership.ts'),
        import('/src/lib/teammate/teammateModel.ts'),import('/scripts/commune-permission-ui-client.mjs')]);
      const target=document.createElement('section');document.body.append(target);const dialog=mount(Dialog,{target});
      window.ownedMembership={
        register:()=>registerCommuneMember(workspaceId,paneId),
        remove:async()=>{const member=parseTopologySnapshot(await window.ownedApproval.invoke('get_teammate_topology',{workspaceId})).roster.find(member=>member.paneId===paneId);if(!member)throw Error('registered member missing');await removeCommuneMember(workspaceId,member);},
        dispose:()=>unmount(dialog),
      };
      window.membershipJob=window.ownedMembership.register().then(()=>({ok:true}),error=>({error:String(error)}));
    },{workspaceId,paneId:memberPane});
    const connect=page.getByRole('dialog',{name:'连接既有 Agent 会话'});
    await connect.getByRole('textbox').fill(JSON.stringify(description));
    await connect.getByRole('button',{name:'连接',exact:true}).click();
    assert.deepEqual(await page.evaluate(()=>window.membershipJob),{ok:true});
    const topology=await page.evaluate(async workspaceId=>window.ownedApproval.invoke('get_teammate_topology',{workspaceId}),workspaceId);
    const member=topology.roster.find(member=>member.paneId===memberPane);assert.ok(member?.online);assert.equal(member.sessionId,descriptor.sessionId);
    await page.evaluate(()=>{window.membershipJob=window.ownedMembership.remove().then(()=>({ok:true}),error=>({error:String(error)}));});
    await page.getByRole('dialog',{name:'移出 Commune'}).getByRole('button',{name:'移出',exact:true}).click();
    assert.deepEqual(await page.evaluate(()=>window.membershipJob),{ok:true});
    const after=await page.evaluate(async workspaceId=>window.ownedApproval.invoke('get_teammate_topology',{workspaceId}),workspaceId);
    assert.equal(after.roster.some(member=>member.paneId===memberPane),false);
    const ptys=await call('GET','/v1/domain/ptys');assert.ok(ptys.ptys.some(pty=>(pty.id===memberPane||pty.pty_id===memberPane)&&pty.child_pid===descriptor.process.pid));
    const history=await page.evaluate(async workspaceId=>window.ownedApproval.invoke('read_agent_recent_replies',{workspaceId,limit:100}),workspaceId);
    assert.ok(JSON.stringify(history).includes('existing history'));
    await page.evaluate(()=>window.ownedMembership.dispose());await page.evaluate(()=>window.ownedApproval.dispose());
    assert.deepEqual(pageErrors,[]);
    evidence.checks=['production connection dialog registers the already-running unlisted process and original protocol session','production removal dialog removes membership through Runtime','exact original process survives removal','protocol history remains readable after removal'];
    evidence.result='PASS';
  }else{
  const receipt = await page.evaluate(async () => window.ownedApproval.invoke('send_agent_message', { ...window.ownedApproval.target, message: 'permission-needed', idempotency_key: crypto.randomUUID() }));
  const dialog = page.getByRole('alertdialog');
  await dialog.waitFor();
  await dialog.getByRole('button', { name: 'Allow once', exact: true }).waitFor();
  await dialog.getByRole('button', { name: 'Allow always', exact: true }).waitFor();
  assert.equal(await dialog.getByRole('button', { name: '修改', exact: true }).count(), 0);
  assert.ok((await dialog.textContent()).includes('private-argument'), 'native reviewer must see actual tool input');
  await page.screenshot({ path: screenshot });
  await page.evaluate(async () => {
    const {bridge}=await import('/src/lib/transport/tauriShim/bridge.ts');
    const invoke=bridge.invoke.bind(bridge);
    window.ownedDecisionCalls=0;
    window.restoreDecisionReply=()=>{bridge.invoke=invoke;};
    bridge.invoke=async (...args)=>{
      const result=await invoke(...args);
      if(args[0]==='resolve_hitl_request') {window.ownedDecisionCalls++;throw Error('owned lost reply after actual Runtime commit');}
      return result;
    };
  });
  await dialog.getByRole('button', { name: 'Allow once', exact: true }).click();
  await until(async () => await dialog.count() === 0);
  assert.equal(await page.evaluate(()=>window.ownedDecisionCalls),1,'lost decision reply must not automatically repeat authorization');
  await page.evaluate(()=>window.restoreDecisionReply());

  await until(async () => {
    const value = await page.evaluate(async deliveryId => window.ownedApproval.invoke('get_agent_message_delivery_status', { ...window.ownedApproval.target, deliveryId }), receipt.deliveryId);
    return value.execution?.state === 'turn_ended';
  });
  await page.evaluate(() => window.ownedApproval.pause());
  const external = await call('POST', '/v1/domain/commands', {method:'report_execution_rejection',native:true,roots:[],connectionId:crypto.randomUUID(),args:{
    initiator:'owned-test',action:'offline external command',executor:'external-test-gateway',policySource:'external-test-policy',requestId:'owned-offline-report',
    reason:'reported while presentation disconnected',nextStep:'external authorization only',
  }});
  assert.equal(external.ok,true);
  assert.equal(await dialog.count(),0,'detached component cannot receive the new report');
  await page.evaluate(() => window.ownedApproval.reconnect());
  await dialog.waitFor();
  assert.ok((await dialog.textContent()).includes('reported while presentation disconnected'));
  await page.evaluate(() => window.ownedApproval.pause());
  const dismissed = await call('POST', '/v1/domain/commands', {method:'dismiss_execution_rejection',native:true,roots:[],connectionId:crypto.randomUUID(),args:{id:external.value}});
  assert.equal(dismissed.value,true);
  assert.equal(await dialog.count(),1,'offline presentation retains its last projection');
  await page.evaluate(() => window.ownedApproval.reconnect());
  await until(async () => await dialog.count() === 0);
  const history = page.getByTestId('commune-history');
  await history.getByRole('button', { name: /刷新/ }).click();
  await history.locator('[data-history-kind="permission_decided"]').waitFor();
  assert.ok((await history.textContent()).includes('existing history'));
  const audit = await page.evaluate(async () => window.ownedApproval.invoke('list_hitl_audit_remote', {
    workspaceId: window.ownedApproval.target.workspaceId, limit: 20,
  }));
  assert.equal(audit.cap, 50);
  assert.ok(audit.items.some(item => item.source === 'local' && item.verdict === 'approve' && item.outcome === 'decided'));
  assert.ok(audit.items.every(item => item.riskLevel === 'unknown'));
  assert.ok(!JSON.stringify(audit).includes('private-operation'));
  assert.ok(!JSON.stringify(audit).includes('private-argument'));

  assert.equal(await history.getByRole('button', { name: /恢复|Resume|启动/ }).count(), 0);
  await until(() => page.evaluate(() => !window.ownedGroups.busy));
  const groupUi = page.getByTestId('commune-groups');
  await groupUi.getByRole('button', {name:'新建编组',exact:true}).click();
  await groupUi.getByRole('checkbox',{name:'unknown-protocol-process',exact:true}).check();
  await groupUi.getByPlaceholder('组名（可选）').fill('Browser group');
  await groupUi.getByRole('button', {name:'建组（1）',exact:true}).click();
  await until(() => page.evaluate(() => !window.ownedGroups.busy && window.ownedGroups.groups.length === 1));
  const projection = await page.evaluate(() => ({groups:JSON.parse(JSON.stringify(window.ownedGroups.groups)),groupsRevision:window.ownedGroups.revision}));
  assert.equal(projection.groups[0].memberAgentIds[0],identity.agentId);
  projection.groups[0].name = 'Remote revision';
  const remoteEdit = await call('POST','/v1/domain/commands',{method:'set_teammate_groups',args:{workspaceId,groups:projection.groups,expectedRevision:projection.groupsRevision},native:false,roots:[],connectionId:crypto.randomUUID()});
  assert.equal(remoteEdit.ok,true,JSON.stringify(remoteEdit.error));
  await groupUi.getByText('Remote revision',{exact:true}).waitFor();
  await groupUi.getByRole('button', {name:'设为组长',exact:true}).click();
  await until(() => page.evaluate(() => !window.ownedGroups.busy && !!window.ownedGroups.groups[0]?.leaderAgentId));
  await groupUi.getByPlaceholder('给组长派任务…（Enter 发送 / Shift+Enter 换行）').fill('browser-group-objective');
  await groupUi.getByRole('button', {name:'给组派任务',exact:true}).click();
  let groupReceipt;
  await until(async () => {
    groupReceipt = await page.evaluate(async () => (await window.ownedApproval.invoke('get_teammate_topology',{workspaceId:window.ownedApproval.target.workspaceId})).roster.find(member => member.agentId === window.ownedApproval.target.agentId).currentDelivery);
    return groupReceipt?.kind === 'task' && groupReceipt.execution?.state === 'turn_ended';
  });
  assert.equal(groupReceipt.payload.groupContext.groupId,projection.groups[0].id);
  assert.equal(groupReceipt.payload.groupContext.targets[0].sessionId,descriptor.sessionId);
  assert.ok(groupReceipt.taskId);
  assert.equal(groupReceipt.agentAcknowledged,false);
  assert.equal(fs.readFileSync(path.join(root,'prompt-log'),'utf8'),'browser-group-objective\n');
  await groupUi.getByRole('button',{name:'向该组加成员',exact:true}).click();
  await groupUi.getByRole('button',{name:'custom-process-two',exact:true}).click();
  await until(() => page.evaluate(() => !window.ownedGroups.busy && window.ownedGroups.groups[0].memberAgentIds.length === 2));
  const taskUi = groupUi.getByTestId('commune-group-tasks');
  await taskUi.getByText('审定成员与目标，再派发',{exact:true}).click();
  await taskUi.getByRole('checkbox',{name:'选择 unknown-protocol-process',exact:true}).check();
  await taskUi.getByRole('checkbox',{name:'选择 custom-process-two',exact:true}).check();
  await taskUi.getByRole('textbox',{name:'广播目标',exact:true}).fill('browser-broadcast');
  const selectedDefinition = await page.evaluate(() => ({groups:JSON.parse(JSON.stringify(window.ownedGroups.groups)),groupsRevision:window.ownedGroups.revision}));
  selectedDefinition.groups[0].color = '#123456';
  const changed = await call('POST','/v1/domain/commands',{method:'set_teammate_groups',args:{workspaceId,groups:selectedDefinition.groups,expectedRevision:selectedDefinition.groupsRevision},native:false,roots:[],connectionId:crypto.randomUUID()});
  assert.equal(changed.ok,true);
  await taskUi.getByText('编组或会话绑定已变，须重新审定。',{exact:false}).waitFor();
  assert.equal(await taskUi.getByRole('button',{name:'确认派发（2）',exact:true}).isDisabled(),true);
  await taskUi.getByRole('button',{name:'清除旧选择',exact:true}).click();
  await taskUi.getByRole('checkbox',{name:'选择 unknown-protocol-process',exact:true}).check();
  await taskUi.getByRole('checkbox',{name:'选择 custom-process-two',exact:true}).check();
  await taskUi.getByRole('button',{name:'确认派发（2）',exact:true}).click();
  await until(async () => await taskUi.locator('[data-child-state="reply_ended"]').count() === 2);
  await taskUi.getByRole('combobox',{name:'协作动作',exact:true}).selectOption('assign');
  await taskUi.getByRole('checkbox',{name:'选择 unknown-protocol-process',exact:true}).check();
  await taskUi.getByRole('checkbox',{name:'选择 custom-process-two',exact:true}).check();
  await taskUi.getByRole('textbox',{name:'unknown-protocol-process 的审定任务',exact:true}).fill('cancel-permission');
  await taskUi.getByRole('textbox',{name:'custom-process-two 的审定任务',exact:true}).fill('wait-for-cancel');
  await taskUi.getByRole('button',{name:'确认派发（2）',exact:true}).click();
  await until(async () => await taskUi.locator('[data-child-state="waiting_permission"]').count() === 1 && await taskUi.locator('[data-child-state="running"]').count() === 1);
  const activeParent = taskUi.locator('article').filter({has:page.locator('[data-child-state="waiting_permission"]')});
  const activeParentId = await activeParent.getAttribute('data-group-task-id');
  await dialog.getByRole('button',{name:'取消所属组任务',exact:true}).click();
  await until(async () => await taskUi.locator(`[data-group-task-id="${activeParentId}"] [data-child-state="cancelled"]`).count() === 2);
  await until(async () => await dialog.count() === 0);
  assert.equal(fs.readFileSync(path.join(root,'prompt-log'),'utf8'),'browser-group-objective\nbrowser-broadcast\n');
  assert.equal(fs.readFileSync(path.join(secondRoot,'prompt-log'),'utf8'),'browser-broadcast\nwait-for-cancel\n');
  await taskUi.getByRole('combobox',{name:'协作动作',exact:true}).selectOption('assign');
  await taskUi.getByRole('checkbox',{name:'选择 unknown-protocol-process',exact:true}).check();
  await taskUi.getByRole('textbox',{name:'unknown-protocol-process 的审定任务',exact:true}).fill('produce-outputs');
  await taskUi.getByRole('button',{name:'确认派发（1）',exact:true}).click();
  await taskUi.locator('[data-protocol-output-kind="diff"]').waitFor();
  await until(async () => await taskUi.locator('[data-protocol-output-kind]').count() === 3);
  assert.equal(await taskUi.locator('[data-protocol-output-kind]').count(),3);
  assert.ok(!(await taskUi.textContent()).includes('urn:fixture:obsolete'));
  assert.ok(!(await taskUi.textContent()).includes('urn:fixture:late'));
  await taskUi.locator('[data-protocol-output-kind="diff"]').getByRole('button',{name:'查看原始协议记录',exact:true}).click();
  await until(async () => (await taskUi.locator('[data-protocol-output-kind="diff"]').textContent()).includes('tool_call_update'));
  assert.equal(fs.readFileSync(path.join(root,'prompt-log'),'utf8'),'browser-group-objective\nbrowser-broadcast\nproduce-outputs\n');
  await groupUi.scrollIntoViewIfNeeded();
  await page.screenshot({path:path.resolve('.webview2-dev-cdp/070-commune-groups-ui.png')});
  await page.setViewportSize({width:390,height:844});
  for (const [name, palette] of Object.entries({
    dark: {'--rg-bg':'#101613','--rg-surface':'#18221e','--rg-fg':'#e8eee9','--rg-fg-muted':'#a0ada5','--rg-border':'#34413a','--rg-accent':'#6e8f7a'},
    light: {'--rg-bg':'#f3f5f2','--rg-surface':'#ffffff','--rg-fg':'#17221c','--rg-fg-muted':'#617067','--rg-border':'#d6ded8','--rg-accent':'#2f5d4e'},
  })) {
    await page.evaluate(values => { for (const [key,value] of Object.entries(values)) document.documentElement.style.setProperty(key,value); },palette);
    const dimensions=await groupUi.evaluate(el=>({clientWidth:el.clientWidth,scrollWidth:el.scrollWidth}));
    assert.ok(dimensions.clientWidth>0 && dimensions.scrollWidth<=dimensions.clientWidth+1,`${name} narrow Commune layout overflows: ${JSON.stringify(dimensions)}`);
    await page.screenshot({path:path.resolve(`.webview2-dev-cdp/073-commune-groups-${name}-narrow.png`)});
    const historyUi=page.getByTestId('commune-history');
    const historyDimensions=await historyUi.evaluate(el=>({clientWidth:el.clientWidth,scrollWidth:el.scrollWidth}));
    assert.ok(historyDimensions.clientWidth>0 && historyDimensions.scrollWidth<=historyDimensions.clientWidth+1,`${name} narrow history layout overflows: ${JSON.stringify(historyDimensions)}`);
    await historyUi.screenshot({path:path.resolve(`.webview2-dev-cdp/073-commune-history-${name}-narrow.png`)});
  }
  const calls = await page.evaluate(() => window.ownedApproval.osCalls);
  assert.deepEqual(calls, ['bootstrap_runtime_client', 'bootstrap_runtime_client', 'bootstrap_runtime_client']);
  await page.evaluate(() => window.ownedApproval.dispose());
  assert.deepEqual(pageErrors, [], 'production component reported browser errors');
  const ptys = await call('GET', '/v1/domain/ptys');
  assert.ok(ptys.ptys.some(pty => pty.id === paneId && pty.child_pid === descriptor.process.pid));
  const decisions = fs.readFileSync(path.join(root,'permission-decisions'),'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.deepEqual(decisions,[{optionId:'once',outcome:'selected'},{outcome:'cancelled'}]);
  assert.ok(ptys.ptys.some(pty => pty.id === secondPane && pty.child_pid === secondDescriptor.process.pid));
  evidence.checks = ['Workspace tabs mark only a locally named save and independently surface unread Agent activity', 'After Runtime consumes a real permission, a simulated lost client reply triggers a readonly refresh without another authorization call',
    'Production modal reconnect recovers a rejection created offline and removes a card dismissed offline',
    'Actual peer options appear in the production modal with actual tool input', 'No Modify action for standard ACP permissions',
    'A real click resolves the original permission ID and the original prompt ends', 'Business RPC and permission events use Runtime; OS IPC only issues bootstrap grant',
    'Unmounting the presentation client preserves the exact existing Agent process',
    'Production history component reads replay, Runtime execution and approval records without a launch/resume action',
    'After a real permission click, production RPC reads the durable redacted audit facts without claiming risk or command execution',
    'Production group UI creates exact registered membership through Runtime and converges to a remote revision event',
    'A real group-leader task click freezes group and conversation binding in a durable task receipt; end_turn does not claim objective success',
    'Production multi-member task UI preserves draft across refresh, blocks changed group revision, broadcasts once, shows permission/running per child and cancels both original turns',
    'Production task UI shows protocol-reported outputs and reads their original journal record; ordinary text and after-response output are excluded'];
  evidence.screenshot = screenshot; evidence.result = 'PASS';
  }
} catch (error) {
  evidence.result = 'FAIL'; evidence.error = error.message; evidence.pageErrors = pageErrors;
  if (page) {
    evidence.browser = await page.evaluate(async () => ({ body: document.body.innerText,
      osCalls: window.ownedApproval?.osCalls, pendingCount: window.ownedApproval ? (await window.ownedApproval.invoke('list_hitl_pending_local', {})).length : null })).catch(failure => ({ error: failure.message }));
  }
  process.exitCode = 1;
}
finally {
  await browser?.close();
  if (endpoint && endpoint.pid === kernel?.pid) await call('POST', '/v1/shutdown').catch(() => {});
  if (vite && vite.exitCode === null) {
    if (process.platform === 'win32') await new Promise(resolve => spawn('taskkill', ['/PID', String(vite.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' }).once('exit', resolve));
    else vite.kill('SIGTERM');
  }
  fs.mkdirSync(path.dirname(output), { recursive: true }); fs.writeFileSync(output, JSON.stringify(evidence, null, 2) + '\n');
}
console.log(JSON.stringify(evidence, null, 2));
