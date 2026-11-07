// Two owned existing protocol processes + real Kernel/native facade + production Commune components.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { chromium, expect } from '@playwright/test';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-global-commune-'));
const evidenceDir=process.env.RIDGE_COMMUNE_EVIDENCE_DIR ?? 'artifacts/commune084';
const output = path.resolve(evidenceDir,'global-ui.json');
fs.mkdirSync(path.dirname(output), { recursive: true });
const fixture = fs.readdirSync('target/debug/deps').filter(name => /^commune_registration-[a-f0-9]+\.exe$/.test(name))
  .map(name => path.resolve('target/debug/deps', name)).sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];
assert.ok(fixture, 'build commune_registration first');
const evidence = { scope: 'Owned ACP process fixtures + real Kernel native RPC + production desktop/mobile Commune components; no external Agent/WAN claim', checks: [], result: 'RUNNING' };
let kernel, vite, browser, page, endpoint;
const until = async check => { const deadline = Date.now() + 30000; while (Date.now() < deadline) { if (await check()) return; await new Promise(resolve => setTimeout(resolve, 30)); } throw Error('owned global Commune readiness timeout'); };
const call = async (method, url, body) => {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}${url}`, { method,
    headers: { 'x-ridge-kernel-token': endpoint.token, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(6000) });
  assert.ok(response.ok); return response.json();
};
try {
  const port = await new Promise(resolve => { const server = net.createServer(); server.listen(0, '127.0.0.1', () => { const port = server.address().port; server.close(() => resolve(port)); }); });
  const devUrl = `http://127.0.0.1:${port}`;
  const log = fs.openSync(path.join(root, 'vite.log'), 'a');
  vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js'], { windowsHide: true, stdio: ['ignore', log, log],
    env: { ...process.env, RIDGE_WEB_REMOTE: '', RIDGE_NATIVE_RUNTIME: '1', RIDGE_DEV_SERVER_PORT: String(port) } }); fs.closeSync(log);
  await until(async () => fetch(devUrl + '/@vite/client', { signal: AbortSignal.timeout(1000) }).then(response => response.ok, () => false));
  kernel = spawn(path.resolve('target/debug/ridge-kernel.exe'), [], { windowsHide: true, stdio: 'ignore',
    env: { ...process.env, RIDGE_KERNEL_DATA_DIR: root, RIDGE_AUTH_FILE: path.join(root, 'auth.json') } });
  await until(() => { if (!fs.existsSync(path.join(root, 'kernel.json'))) return false; endpoint = JSON.parse(fs.readFileSync(path.join(root, 'kernel.json'))); return endpoint.pid === kernel.pid; });
  const workspaces = [];
  for (const name of ['Global one', 'Global two']) {
    const workspaceId = (await call('POST', '/v1/domain/workspaces')).workspace_id;
    const cwd = path.join(root, workspaceId); fs.mkdirSync(cwd);
    const descriptorPath = path.join(cwd, 'endpoint.json'); const paneId = crypto.randomUUID();
    await call('POST', '/v1/domain/ptys', { workspace_id: workspaceId, pty_id: paneId, program: fixture,
      args: ['--exact', 'existing_protocol_endpoint_fixture', '--ignored'], cwd,
      env: { RIDGE_COMMUNE_FIXTURE: descriptorPath, RIDGE_COMMUNE_LABEL: name }, role: 'shell', cols: 80, rows: 24 });
    await until(() => fs.existsSync(descriptorPath));
    const descriptor = JSON.parse(fs.readFileSync(descriptorPath));
    const registered = await call('POST', '/v1/domain/commune/register', { workspaceId, paneId, process: descriptor.process,
      endpointOwner: descriptor.process, endpoint: descriptor.endpoint, sessionId: descriptor.sessionId, cwd });
    assert.equal(registered.ok, true, registered.error); workspaces.push({ id: workspaceId, name });
  }
  const terminalWorkspace=(await call('POST','/v1/domain/workspaces')).workspace_id;
  const terminalDescriptorPath=path.join(root,'terminal-endpoint.json');
  const terminalPane=(await call('GET',`/v1/domain/workspaces/${terminalWorkspace}`)).panes[0];
  await call('POST','/v1/domain/ptys',{workspace_id:terminalWorkspace,pty_id:terminalPane,program:fixture,args:['--exact','existing_protocol_endpoint_fixture','--ignored'],cwd:root,env:{RIDGE_COMMUNE_FIXTURE:terminalDescriptorPath,RIDGE_COMMUNE_LABEL:'Terminal candidate'},role:'shell',cols:80,rows:24});
  await until(()=>fs.existsSync(terminalDescriptorPath));
  const terminalDescriptor=JSON.parse(fs.readFileSync(terminalDescriptorPath));
  const terminalConnectionPath=path.join(root,'terminal-connection.json');
  fs.writeFileSync(terminalConnectionPath,JSON.stringify({workspaceId:terminalWorkspace,paneId:terminalPane,process:terminalDescriptor.process,endpointOwner:terminalDescriptor.process,endpoint:terminalDescriptor.endpoint,sessionId:terminalDescriptor.sessionId,cwd:root}));
  const command=async(method,args,native=true)=>{const reply=await call('POST','/v1/domain/commands',{method,args,native,roots:[],connectionId:crypto.randomUUID()});assert.equal(reply.ok,true,JSON.stringify(reply));return reply.value;};
  await command('save_agent_profile_overrides',{overrides:[{id:'owned-unregistered',processNames:[path.basename(fixture)],executable:'node',resumeArgv:[],yoloArgs:[]}]});
  workspaces.push({id:terminalWorkspace,name:'Global terminal'});
  await until(async()=>{const topology=await command('get_teammate_topology',{workspaceId:terminalWorkspace},false);return topology.discovered?.length===1;});
  const observation=(await command('get_teammate_topology',{workspaceId:terminalWorkspace},false)).discovered[0];
  assert.deepEqual(Object.keys(observation).sort(),['agentId','candidateRef','connectionState','name','paneId','protocolConnected','recentOutput','workspaceId']);
  assert.equal(observation.paneId,terminalPane);assert.equal(observation.protocolConnected,false);
  const denied=await call('POST','/v1/domain/commands',{method:'get_agent_terminal_view',args:{workspaceId:terminalWorkspace,paneId:terminalPane},native:false,roots:[],connectionId:crypto.randomUUID()});
  assert.equal(denied.ok,false);evidence.checks.push('Non-native real process discovery is read-only and redacted; native-only terminal interaction still denied');
  const grant = await call('POST', '/v1/local/clients', { deviceId: 'owned-global-commune', origin: devUrl });
  browser = await chromium.launch({ headless: true }); page = await browser.newPage({ viewport: { width: 1100, height: 950 } });
  const pageErrors = []; page.on('pageerror', error => pageErrors.push(error.message));
  await page.route('**/__global_commune', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><body style="height:100vh"><main id="component" style="height:100%"></main></body>' }));
  await page.exposeFunction('fixtureGrant', () => grant); await page.goto(devUrl + '/__global_commune');
  for (let boot = 0; boot < 3; boot++) { try {
  await page.evaluate(async workspaces => {
    window.isTauri = true; window.__TAURI_INTERNALS__ = { invoke: command => {
      if (command === 'bootstrap_runtime_client') return window.fixtureGrant(); throw Error('Unexpected OS business call: ' + command);
    } };
    const [{ connectPresentationRuntime }, { bridge }, { invoke }, { listen }, client, { default: Panel }, paneTree, highlight] = await Promise.all([
      import('/src/lib/transport/runtimeClient.ts'), import('/src/lib/transport/tauriShim/bridge.ts'),
      import('/src/lib/transport/nativeRuntimeCore.ts'), import('/src/lib/transport/nativeRuntimeEvent.ts'),
      import('/scripts/commune-permission-ui-client.mjs'), import('/src/lib/teammate/AgentCenterPanel.svelte'),
      import('/src/lib/stores/paneTree.ts'), import('/src/lib/teammate/agentPaneHighlightSync.ts'),
    ]);
    const connection = await connectPresentationRuntime(); bridge.attach(connection.transport, { useGlobalWorkspace: false });
    const [{ initThemeSystem, getThemeIds }, { applyTheme }] = await Promise.all([
      import('/src/lib/stores/themes.ts'), import('/src/lib/stores/settings.ts'),
    ]);
    await initThemeSystem(); applyTheme(getThemeIds()[0]);
    paneTree.workspacesList.set(workspaces);
    await highlight.refreshAgentPaneHighlight({ workspaceIds: workspaces.map(workspace => workspace.id), invoke });
    const groupReadFault={active:true};
    const nativeInvoke=bridge.invoke.bind(bridge);
    bridge.invoke=(method,args,...rest)=>{
      if(groupReadFault.active&&method==='get_teammate_topology'&&args?.groupScope==='host')return Promise.reject(Error('owned initial group read unavailable'));
      return nativeInvoke(method,args,...rest);
    };
    const app = client.createClassComponent({ component: Panel, target: document.getElementById('component'), props: { workspaceId: workspaces[0].id, visible: true } });
    window.globalCommune = { app, client, invoke, listen, workspaces, connection, bridge, groupReadFault };
  }, workspaces);
  break;
  } catch (error) {
    if (boot === 2 || !/Execution context was destroyed|net::ERR_ABORTED/.test(String(error))) throw error;
    await page.goto(devUrl + '/__global_commune', { waitUntil: 'domcontentloaded' });
  } }
  await expect(page.locator('#commune-panel-members > ul').getByText('existing history', { exact: true })).toHaveCount(2, { timeout: 20000 });
  await page.evaluate(() => window.globalCommune.app.$set({ workspaceId: window.globalCommune.workspaces[1].id }));
  await expect(page.locator('#commune-panel-members > ul').getByText('existing history', { exact: true })).toHaveCount(2);
  await expect(page.getByTestId('commune-terminal-agent')).toHaveCount(1);
  const globalGroups=page.getByTestId('host-commune-groups');await expect(globalGroups).toHaveCount(1);
  await expect(globalGroups).toHaveAttribute('open', '');
  await expect(globalGroups.getByRole('alert')).toContainText('owned initial group read unavailable');
  await globalGroups.getByTestId('commune-create-group').click();
  await globalGroups.getByRole('textbox',{name:/组名|Group name/i}).fill('Draft survives initial retry');
  await expect(globalGroups.locator('.builder').getByRole('button',{name:/^(Create group|创建编组)$/})).toBeDisabled();
  await expect(globalGroups.getByRole('button',{name:/^(Refresh|刷新)$/})).toBeEnabled();
  await page.evaluate(()=>window.globalCommune.groupReadFault.active=false);
  await globalGroups.getByRole('button',{name:/^(Refresh|刷新)$/}).click();
  await expect(globalGroups.locator('.builder').getByRole('button',{name:/^(Create group|创建编组)$/})).toBeEnabled();
  await expect(globalGroups.getByRole('textbox',{name:/组名|Group name/i})).toHaveValue('Draft survives initial retry');
  await globalGroups.locator('.builder').getByRole('button',{name:/^(Back|返回)$/}).click();
  evidence.checks.push('Initial Host group read failure disables writes, retains the local form and exposes an enabled explicit refresh that recovers the authoritative revision');
  await expect(page.getByTestId('commune-terminal-agent').getByRole('button',{name:/连接会话|Connect session/i})).toBeVisible();
  await globalGroups.getByTestId('commune-create-group').click();
  await globalGroups.getByRole('textbox',{name:/组名|Group name/i}).fill('Cross-source pair');
  await expect(globalGroups.locator('[data-group-member]').getByRole('checkbox')).toHaveCount(2);
  for(const checkbox of await globalGroups.locator('[data-group-member]').getByRole('checkbox').all())await checkbox.check();
  await expect(globalGroups.getByTestId('commune-group-candidate')).toHaveCount(1);
  await expect(globalGroups.getByTestId('commune-group-candidate').getByRole('button')).toBeVisible();
  await globalGroups.locator('.builder').getByRole('button',{name:/^(Create group|创建编组)$/}).click();
  await expect(globalGroups.getByTestId('host-commune-group')).toHaveCount(1);
  const originalGroup=globalGroups.getByTestId('host-commune-group').first();
  const dispatch=originalGroup.getByTestId('commune-group-tasks');await dispatch.locator('summary').click();
  for(const checkbox of await dispatch.getByRole('checkbox').all())await checkbox.check();
  await dispatch.locator('textarea').fill('ui-cross-source-once');
  await dispatch.getByRole('button').filter({hasText:/2/}).click();
  await expect(dispatch.locator('[data-group-task-id]')).toHaveCount(1);
  await expect(dispatch.locator('[data-child-state="reply_ended"]')).toHaveCount(2);
  evidence.checks.push('One desktop builder selects members from both sources and submits a real frozen cross-source protocol task');
  await globalGroups.getByTestId('commune-create-group').click();
  await expect(globalGroups.locator('.builder [data-group-member]')).toHaveCount(2);
  for (const checkbox of await globalGroups.locator('.builder [data-group-member]').getByRole('checkbox').all()) await expect(checkbox).toBeDisabled();
  await globalGroups.getByRole('textbox',{name:/组名|Group name/i}).fill('Move destination');
  await globalGroups.locator('.builder').getByRole('button',{name:/^(Create group|创建编组)$/}).click();
  await expect(globalGroups.getByTestId('host-commune-group')).toHaveCount(2);
  const groupBefore = await page.evaluate(() => window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
  const sourceId=groupBefore.groups.find(group=>group.name==='Cross-source pair').id;
  const destinationId=groupBefore.groups.find(group=>group.name==='Move destination').id;
  const movingMember=originalGroup.locator('.member').first();
  await movingMember.getByRole('button',{name:/^(Set leader|设为组长)$/}).click();
  await expect(movingMember.getByRole('button',{name:/^(Unset leader|取消组长)$/})).toBeVisible();
  await movingMember.getByRole('combobox').selectOption(destinationId);
  const destinationGroup=globalGroups.getByTestId('host-commune-group').last();
  await expect(destinationGroup.locator('.member')).toHaveCount(1);
  await expect(originalGroup.locator('.member')).toHaveCount(1);
  const movedProjection=await page.evaluate(()=>window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
  assert.equal(movedProjection.groups.find(group=>group.id===sourceId).leaderAgentId ?? null,null);
  await expect(dispatch.locator('[data-group-task-id]')).toHaveCount(1);
  await destinationGroup.locator('.member').getByRole('combobox').selectOption(sourceId);
  await expect(originalGroup.locator('.member')).toHaveCount(2);
  await page.evaluate(async () => {
    const current=window.globalCommune;
    const {teammateGroupStore}=await import('/src/lib/teammate/teammateGroups.svelte.ts');
    const invoke=current.bridge.invoke.bind(current.bridge);
    let release;
    const held=new Promise(resolve=>release=resolve);
    let releaseWrite;
    const heldWrite=new Promise(resolve=>releaseWrite=resolve);
    current.groupReadHeld=false;
    current.groupReadCaptured=false;
    current.groupWriteHeld=false;
    current.bridge.invoke=async (method,args,...rest)=>{
      if(method==='get_teammate_topology'&&args.groupScope==='host'&&!current.groupReadHeld){
        current.groupReadHeld=true;
        const snapshot=await invoke(method,args,...rest);
        current.groupReadCaptured=true;
        await held;
        return snapshot;
      }
      if(method==='set_teammate_groups'&&args.groupScope==='host'&&!current.groupWriteHeld){
        const snapshot=await invoke(method,args,...rest);
        current.groupWriteHeld=true;
        await heldWrite;
        return snapshot;
      }
      return invoke(method,args,...rest);
    };
    current.releaseGroupWrite=()=>releaseWrite();
    current.releaseGroupRead=async()=>{release();await current.groupReadPromise;current.bridge.invoke=invoke;};
    current.groupReadPromise=teammateGroupStore().refresh();
  });
  await expect.poll(()=>page.evaluate(()=>window.globalCommune.groupReadCaptured)).toBe(true);
  await expect(globalGroups.getByTestId('commune-group-sync-state')).toContainText(/正在刷新|Refreshing/);
  const groupName=destinationGroup.getByRole('textbox',{name:/Rename group|重命名编组/});
  await expect(groupName).toBeEnabled();
  await groupName.fill('Editable during refresh');
  await groupName.press('Tab');
  await expect.poll(async () => {
    const projection=await page.evaluate(()=>window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
    return projection.groups.find(group=>group.id===destinationId)?.name;
  }).toBe('Editable during refresh');
  await expect.poll(()=>page.evaluate(()=>window.globalCommune.groupWriteHeld)).toBe(true);
  await expect(globalGroups.getByTestId('commune-group-sync-state')).toContainText(/正在保存|Saving/);
  await destinationGroup.getByRole('button',{name:/^(Dissolve|解散)$/}).click();
  await expect(destinationGroup.locator('.confirmation').getByRole('button',{name:/^(Confirm|确认)$/})).toBeDisabled();
  await destinationGroup.locator('.confirmation').getByRole('button',{name:/^(Back|返回)$/}).click();
  await expect(destinationGroup.locator('.confirmation')).toHaveCount(0);
  await page.evaluate(()=>window.globalCommune.releaseGroupWrite());
  await expect(groupName).toBeEnabled();
  await page.evaluate(()=>window.globalCommune.releaseGroupRead());
  await expect(groupName).toHaveValue('Editable during refresh');
  const confirmedAfterRead=await page.evaluate(async () => {
    const {teammateGroupStore}=await import('/src/lib/teammate/teammateGroups.svelte.ts');
    const store=teammateGroupStore();
    const authoritative=await window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'});
    return {revision:store.revision,authoritativeRevision:authoritative.groupsRevision,names:store.groups.map(group=>group.name)};
  });
  assert.equal(confirmedAfterRead.revision,confirmedAfterRead.authoritativeRevision);
  assert.ok(confirmedAfterRead.names.includes('Editable during refresh'));
  await expect(destinationGroup.getByRole('button',{name:/^(Dissolve|解散)$/})).toBeEnabled();
  await destinationGroup.getByRole('button',{name:/^(Dissolve|解散)$/}).click();
  await expect(destinationGroup.locator('.confirmation')).toBeVisible();
  await destinationGroup.locator('.confirmation').getByRole('button',{name:/^(Confirm|确认)$/}).click();
  await expect(globalGroups.getByTestId('host-commune-group')).toHaveCount(1);
  evidence.checks.push('A held old Kernel snapshot leaves rename editable and allows a real CAS write; releasing it preserves the new name and revision. All assigned members remain visible; explicit move clears previous leadership and keeps the frozen dispatch; dissolve uses inline confirmation');
  await page.screenshot({path:path.resolve(evidenceDir,'groups-desktop.png')});
  await page.getByTestId('commune-tab-history').click();
  await expect(page.locator('[data-history-kind="agent_message_chunk"]')).toHaveCount(2);
  evidence.checks.push('Desktop members/replies/history aggregate two existing sessions and survive active-workspace prop switching');
  await page.screenshot({ path: path.resolve(evidenceDir,'global-desktop.png') });
  await page.evaluate(async () => {
    const current = window.globalCommune; current.app.$destroy();
    const [{ CloudRemoteConnection }, { default: Panel }] = await Promise.all([import('/src/remote/lib/cloudRemote.ts'), import('/src/remote/lib/SidebarTeamRoster.svelte')]);
    const link = new CloudRemoteConnection({ adapter: { onTerminalFrame: () => () => {}, sendControl() {} },
      tryTrustGrant: async () => true, verifyTotp: async () => true, disconnect() {} },
      { invoke: (command, args) => command === 'read_agent_recent_replies' && args.workspaceId === current.failHistorySource
          ? Promise.reject(Error('owned source temporarily unavailable')) : current.invoke(command, args),
        listen: current.listen, hasCapability: () => true, onCapabilitiesChanged: () => () => {} }, { fixedAuthorized: true });
    await link.init(); current.link = link;
    current.app = current.client.createClassComponent({ component: Panel, target: document.getElementById('component'), props: { ws: link, workspaces: current.workspaces, workspaceId: current.workspaces[0].id } });
  });
  await page.setViewportSize({ width: 412, height: 915 });
  await expect(page.getByText('existing history', { exact: true })).toHaveCount(2, { timeout: 20000 });
  await page.evaluate(() => window.globalCommune.app.$set({ workspaceId: window.globalCommune.workspaces[1].id }));
  await expect(page.getByText('existing history', { exact: true })).toHaveCount(2);
  await expect(page.getByTestId('remote-terminal-agent')).toHaveCount(1);
  await expect(page.getByTestId('host-commune-groups')).toHaveCount(1);
  await expect(page.getByTestId('host-commune-group')).toHaveCount(1);
  await page.getByTestId('host-commune-groups').getByTestId('commune-create-group').click();
  await expect(page.getByTestId('commune-group-candidate')).toHaveCount(1);
  await expect(page.getByTestId('commune-group-candidate').getByRole('button',{name:/定位终端|Locate terminal/})).toBeVisible();
  await page.screenshot({path:path.resolve(evidenceDir,'groups-mobile.png')});
  await page.evaluate(async()=>{const {setLocale}=await import('/src/lib/i18n/locale.ts');setLocale('en');});
  await page.setViewportSize({width:390,height:844});
  const mobileGroups=page.getByTestId('host-commune-groups');
  await expect(mobileGroups.locator('.builder').getByRole('button',{name:'Create group',exact:true})).toBeVisible();
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth > innerWidth),false,'English groups must fit a 390px viewport');
  for(const button of await mobileGroups.locator('button:visible').all()){
    assert.ok((await button.boundingBox()).height>=44,'mobile group buttons must retain 44px touch height');
  }
  await page.screenshot({path:path.resolve(evidenceDir,'groups-mobile-en.png')});
  await page.evaluate(async()=>{const {setLocale}=await import('/src/lib/i18n/locale.ts');setLocale('zh');});
  await page.setViewportSize({width:412,height:915});
  evidence.checks.push('Chinese and English group controls fit narrow screens, retain focus and expose touch targets at least 44px high');
  await page.getByTestId('host-commune-groups').getByRole('textbox',{name:/组名|Group name/i}).fill('Terminal before connection');
  await page.getByTestId('commune-group-candidate').getByRole('checkbox').check();
  await page.getByTestId('host-commune-groups').locator('.builder').getByRole('button',{name:/^(Create group|创建编组)$/}).click();
  await expect(page.getByTestId('host-commune-group')).toHaveCount(2);
  const terminalOrganization=await page.evaluate(()=>window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
  const terminalGroup=terminalOrganization.groups.find(group=>group.name==='Terminal before connection');
  const terminalAgentId=terminalGroup.memberAgentIds[0];
  assert.ok(terminalAgentId);
  const terminalMember=terminalOrganization.members.find(member=>member.agentId===terminalAgentId);
  assert.equal(terminalMember.protocolConnected,false);assert.equal(terminalMember.canSend,false);assert.equal(terminalMember.terminalAvailable,true);
  const organizationTasks=page.getByTestId('host-commune-group').last().getByTestId('commune-group-tasks');
  await organizationTasks.locator('summary').click();
  await expect(organizationTasks.getByRole('checkbox')).toBeDisabled();
  assert.equal(fs.existsSync(path.join(root,'prompt-log')),false,'group enrollment must not send protocol or terminal input');
  evidence.checks.push('Narrow Remote groups an unconnected terminal through its opaque reference, receives one stable member and disables structured dispatch');
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth > innerWidth),false,'narrow Commune must fit its viewport');
  evidence.checks.push('Narrow Remote includes ordinary terminal candidate and the same one global group authority');
  await page.locator('.member-card .msg-input').first().fill('mobile-individual-once');
  await page.locator('.member-card .msg-row .send').first().click();
  const individualLog=path.join(root,workspaces[0].id,'prompt-log');
  await until(()=>fs.existsSync(individualLog)&&fs.readFileSync(individualLog,'utf8').includes('mobile-individual-once'));
  assert.equal(fs.readFileSync(individualLog,'utf8').split('mobile-individual-once').length-1,1);
  evidence.checks.push('Remote individual message reaches the original ACP process once through the production connection and Kernel command parser');
  await page.evaluate(() => {
    const current = window.globalCommune;
    const original = current.invoke;
    current.deliveryKeys = []; current.droppedDeliveryAck = false;
    current.invoke = async (command, args) => {
      const result = await original(command, args);
      if (command === 'send_agent_message' && args.message === 'owned-lost-ack') {
        current.deliveryKeys.push(args.idempotency_key);
        if (!current.droppedDeliveryAck) { current.droppedDeliveryAck = true; throw Error('Owned ACK lost after commit'); }
      }
      if (command === 'send_agent_message' && args.message === 'owned-delayed-ack') await new Promise(resolve => setTimeout(resolve, 650));
      return result;
    };
  });
  const memberInput = page.locator('.member-card .msg-input').first();
  const memberSend = page.locator('.member-card .send').first();
  await expect(memberInput).toHaveValue('');
  await memberInput.fill('owned-lost-ack'); await memberSend.click();
  await expect(page.getByText('Error: Owned ACK lost after commit', { exact: true })).toBeVisible();
  await until(() => fs.readFileSync(individualLog, 'utf8').includes('owned-lost-ack'));
  await memberInput.fill('new draft after lost ACK');
  await memberSend.click();
  await expect.poll(() => page.evaluate(() => window.globalCommune.deliveryKeys.length)).toBe(2);
  await expect(memberSend).toBeEnabled();
  await expect(page.getByText('Error: Owned ACK lost after commit', { exact: true })).toHaveCount(0);
  await expect(memberInput).toHaveValue('new draft after lost ACK');
  const keys = await page.evaluate(() => window.globalCommune.deliveryKeys);
  assert.equal(keys.length, 2); assert.equal(new Set(keys).size, 1);
  assert.equal(fs.readFileSync(individualLog, 'utf8').split('owned-lost-ack').length - 1, 1);
  evidence.checks.push('ACK lost after real Kernel commit retries the frozen original key/text once and preserves the new draft');
  await memberInput.fill('owned-delayed-ack'); await memberSend.click();
  await expect(memberSend).toBeDisabled();
  await memberInput.press('Enter');
  await memberSend.evaluate(button => button.click());
  await memberInput.fill('typing while sending');
  await expect(memberSend).toBeEnabled();
  await expect(memberInput).toHaveValue('typing while sending');
  await until(() => fs.readFileSync(individualLog, 'utf8').includes('owned-delayed-ack'));
  assert.equal(fs.readFileSync(individualLog, 'utf8').split('owned-delayed-ack').length - 1, 1);
  await expect(page.locator('.member-card').first().locator('[data-delivery-state="turn_ended"]')).toBeVisible({ timeout: 10000 });
  evidence.checks.push('A delayed real delivery ACK blocks duplicate click/Enter and retains subsequent typing');
  await page.getByRole('button', { name: /^(History|历史)$/ }).click();
  await expect(page.locator('[data-history-kind="agent_message_chunk"]')).toHaveCount(2);
  await page.evaluate(async () => {
    const current = window.globalCommune;
    current.failHistorySource = current.workspaces[0].id;
    const workspaceId = current.workspaces[1].id;
    const topology = await current.invoke('get_teammate_topology', { workspaceId });
    const member = topology.roster[0];
    await current.invoke('send_agent_message', { workspaceId, paneId: member.paneId, agentId: member.agentId,
      generation: member.generation, lease: member.lease, message: 'produce-outputs', idempotency_key: 'owned-history-source-isolation' });
  });
  await page.getByTestId('commune-history').getByRole('button', { name: /refresh|刷新/i }).click();
  await expect(page.getByRole('alert')).toContainText('owned source temporarily unavailable');
  await expect(page.getByText('done is not a verified deliverable', { exact: true })).toBeVisible({ timeout: 15000 });
  await expect(page.locator('[data-history-kind="agent_message_chunk"] pre').filter({ hasText: /^existing history$/ })).toHaveCount(2);
  await page.evaluate(() => { window.globalCommune.failHistorySource = undefined; });
  await page.getByTestId('commune-history').getByRole('button', { name: /refresh|刷新/i }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  evidence.checks.push('Injected one-source read failure retains its confirmed history while a real healthy ACP prompt adds replies; retry recovers');
  await page.getByRole('button', { name: /^(Members|成员)/ }).first().click();
  const quietMember = await page.evaluate(async () => {
    const current=window.globalCommune;const workspaceId=current.workspaces[1].id;
    const member=(await current.invoke('get_teammate_topology',{workspaceId})).roster[0];
    return {workspaceId,paneId:member.paneId,agentId:member.agentId,id:member.id,generation:member.generation,lease:member.lease};
  });
  await expect(page.locator('.member-card').getByTitle(quietMember.id,{exact:true})).toHaveCount(1);
  await new Promise(resolve=>setTimeout(resolve,600));
  await page.evaluate(async target => {
    const {id,...request}=target;const result=await window.globalCommune.invoke('release_teammate_agent',request);
    if(result.removed!==true)throw Error('Owned quiet member removal failed');
  },quietMember);
  await expect(page.locator('.member-card').getByTitle(quietMember.id,{exact:true})).toHaveCount(0,{timeout:10000});
  evidence.checks.push('A real quiet Kernel member retirement reaches the narrow Cloud facade and removes its card without terminal frames or manual refresh');
  assert.deepEqual(pageErrors, []); evidence.checks.push('Narrow mobile production Commune aggregates the same two source sessions through the real Runtime facade; not a Cloud-network test');
  await page.screenshot({ path: path.resolve(evidenceDir,'global-mobile.png') });
  await page.evaluate(async () => {
    const current=window.globalCommune;current.app.$destroy();
    const {default:Panel}=await import('/src/lib/teammate/AgentCenterPanel.svelte');
    current.app=current.client.createClassComponent({component:Panel,target:document.getElementById('component'),props:{workspaceId:current.workspaces[2].id,visible:true}});
  });
  await page.setViewportSize({width:1100,height:950});
  const authorityBeforeConnect=fs.readFileSync(path.join(root,'commune-groups.json'),'utf8');
  await expect(page.getByTestId('commune-terminal-agent')).toHaveCount(1);
  await page.getByTestId('commune-terminal-agent').getByRole('button',{name:/连接会话|Connect session/i}).click();
  const wizard=page.getByRole('dialog');
  await wizard.getByLabel('选择连接文件').setInputFiles(terminalConnectionPath);
  await expect(wizard.getByLabel('会话预览')).toContainText(terminalDescriptor.sessionId);
  await wizard.getByRole('button',{name:'连接',exact:true}).click();
  await expect(wizard).toHaveCount(0);
  await expect(page.getByTestId('commune-terminal-agent')).toHaveCount(1);
  await expect(page.getByTestId('commune-terminal-agent').getByRole('button',{name:/连接会话|Connect session/i})).toHaveCount(0);
  await expect(page.getByTestId('commune-terminal-agent').getByText('existing history',{exact:true})).toBeVisible();
  const afterConnect=await page.evaluate(()=>window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
  assert.equal(afterConnect.groups.find(group=>group.id===terminalGroup.id).memberAgentIds[0],terminalAgentId);
  assert.equal(afterConnect.members.filter(member=>member.agentId===terminalAgentId).length,1);
  assert.equal(afterConnect.members.find(member=>member.agentId===terminalAgentId).protocolConnected,true);
  assert.equal(afterConnect.members.find(member=>member.agentId===terminalAgentId).canSend,true);
  assert.equal(fs.readFileSync(path.join(root,'commune-groups.json'),'utf8'),authorityBeforeConnect);
  const connectedTasks=page.getByTestId('host-commune-group').last().getByTestId('commune-group-tasks');
  await connectedTasks.locator('summary').click();
  await expect(connectedTasks.getByRole('checkbox')).toBeEnabled();
  await connectedTasks.getByRole('checkbox').check();
  await connectedTasks.locator('textarea').fill('terminal-to-protocol-once');
  await connectedTasks.getByRole('button').filter({hasText:/1/}).click();
  await until(()=>fs.existsSync(path.join(root,'prompt-log'))&&fs.readFileSync(path.join(root,'prompt-log'),'utf8').includes('terminal-to-protocol-once'));
  assert.equal(fs.readFileSync(path.join(root,'prompt-log'),'utf8').split('terminal-to-protocol-once').length-1,1);
  await expect(connectedTasks.locator('[data-group-task-id]').first()).toContainText(/回复结束|Reply ended/);
  await page.evaluate(groupId => {
    const current=window.globalCommune;
    const invoke=current.bridge.invoke.bind(current.bridge);
    current.organizationRequests=[];
    current.bridge.invoke=async (method,args,...rest)=>{
      if(method!=='send_agent_message'||args.groupId!==groupId||args.message!=='disconnect-during-prompt')return invoke(method,args,...rest);
      current.organizationRequests.push(JSON.parse(JSON.stringify(args)));
      const result=await invoke(method,args,...rest);
      if(current.organizationRequests.length===1)throw Error('Owned organization ACK lost after commit');
      return result;
    };
  },terminalGroup.id);
  await connectedTasks.getByRole('checkbox').check();
  await connectedTasks.locator('textarea').fill('disconnect-during-prompt');
  await connectedTasks.getByRole('button').filter({hasText:/1/}).click();
  await expect(connectedTasks.getByRole('alert')).toContainText('Owned organization ACK lost after commit');
  await expect.poll(async()=>{
    const projection=await page.evaluate(()=>window.globalCommune.invoke('get_teammate_topology',{groupScope:'host'}));
    return projection.members.find(member=>member.agentId===terminalAgentId).canSend;
  }).toBe(false);
  await page.evaluate(async () => {
    const current=window.globalCommune;current.app.$destroy();
    const {default:Panel}=await import('/src/lib/teammate/AgentCenterPanel.svelte');
    current.app=current.client.createClassComponent({component:Panel,target:document.getElementById('component'),props:{workspaceId:current.workspaces[2].id,visible:true}});
  });
  const offlineTasks=page.getByTestId('host-commune-group').last().getByTestId('commune-group-tasks');
  await offlineTasks.locator('summary').click();
  await expect(offlineTasks.getByRole('checkbox')).toBeDisabled();
  await offlineTasks.getByRole('button',{name:/重试原派发确认|Retry confirmation for the original dispatch/}).click();
  await expect(offlineTasks.getByRole('button',{name:/重试原派发确认|Retry confirmation for the original dispatch/})).toHaveCount(0);
  const originalRequests=await page.evaluate(()=>window.globalCommune.organizationRequests);
  assert.equal(originalRequests.length,2);assert.equal(originalRequests[0].idempotency_key,originalRequests[1].idempotency_key);
  assert.equal(fs.readFileSync(path.join(root,'prompt-log'),'utf8').split('disconnect-during-prompt').length-1,1);
  evidence.checks.push('Global v3 group retains an unconfirmed dispatch through actual disconnect and component remount; offline effective permissions disable new work while explicit original-key confirmation avoids replay');
  assert.deepEqual(pageErrors,[]);
  await page.screenshot({path:path.resolve(evidenceDir,'organization-connected-desktop.png')});
  evidence.checks.push('Native file wizard attaches the exact existing terminal session to its Remote-enrolled agentId and original group without rewriting organization data; one real group prompt reaches that process');
  evidence.result = 'PASS';
} catch (error) {
  evidence.result = 'FAIL'; evidence.error = String(error); process.exitCode = 1;
  if (page && !page.isClosed()) {
    evidence.uiFailure = await page.getByTestId('host-commune-groups').innerText().catch(() => 'Host group surface unavailable');
    await page.screenshot({path:path.resolve(evidenceDir,'global-ui-failure.png')}).catch(() => {});
  }
}
finally {
  await browser?.close();
  if (endpoint?.pid === kernel?.pid) await call('POST', '/v1/shutdown').catch(() => {});
  if (vite?.pid) await new Promise(resolve => spawn('taskkill', ['/PID', String(vite.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' }).once('exit', resolve));
  evidence.fixtureDirectory = root; fs.writeFileSync(output, JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ result: evidence.result, evidence: output }));
}
