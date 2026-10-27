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
let kernel, vite, browser, endpoint;
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
  const candidateScript=path.join(root,'candidate.cjs');fs.writeFileSync(candidateScript,"process.stdout.write('OWNED_TERMINAL_CANDIDATE\\r\\n');setInterval(()=>{},1000)");
  const terminalPane=(await call('GET',`/v1/domain/workspaces/${terminalWorkspace}`)).panes[0];
  await call('POST','/v1/domain/ptys',{workspace_id:terminalWorkspace,pty_id:terminalPane,program:process.execPath,args:[candidateScript],cwd:root,env:{},role:'shell',cols:80,rows:24});
  const command=async(method,args,native=true)=>{const reply=await call('POST','/v1/domain/commands',{method,args,native,roots:[],connectionId:crypto.randomUUID()});assert.equal(reply.ok,true,JSON.stringify(reply));return reply.value;};
  await command('save_agent_profile_overrides',{overrides:[{id:'owned-unregistered',processNames:[path.basename(process.execPath)],executable:'node',resumeArgv:[],yoloArgs:[]}]});
  workspaces.push({id:terminalWorkspace,name:'Global terminal'});
  await until(async()=>{const topology=await command('get_teammate_topology',{workspaceId:terminalWorkspace},false);return topology.discovered?.length===1;});
  const observation=(await command('get_teammate_topology',{workspaceId:terminalWorkspace},false)).discovered[0];
  assert.deepEqual(Object.keys(observation).sort(),['connectionState','name','paneId','protocolConnected','recentOutput','workspaceId']);
  assert.equal(observation.paneId,terminalPane);assert.equal(observation.protocolConnected,false);
  const denied=await call('POST','/v1/domain/commands',{method:'get_agent_terminal_view',args:{workspaceId:terminalWorkspace,paneId:terminalPane},native:false,roots:[],connectionId:crypto.randomUUID()});
  assert.equal(denied.ok,false);evidence.checks.push('Non-native real process discovery is read-only and redacted; native-only terminal interaction still denied');
  const grant = await call('POST', '/v1/local/clients', { deviceId: 'owned-global-commune', origin: devUrl });
  browser = await chromium.launch({ headless: true }); const page = await browser.newPage({ viewport: { width: 1100, height: 950 } });
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
    const app = client.createClassComponent({ component: Panel, target: document.getElementById('component'), props: { workspaceId: workspaces[0].id, visible: true } });
    window.globalCommune = { app, client, invoke, listen, workspaces, connection, bridge };
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
  await globalGroups.locator('summary').first().click();await globalGroups.getByTestId('commune-create-group').click();
  await globalGroups.getByRole('textbox',{name:/组名|Group name/i}).fill('Cross-source pair');
  await expect(globalGroups.getByRole('checkbox')).toHaveCount(2);
  for(const checkbox of await globalGroups.getByRole('checkbox').all())await checkbox.check();
  await globalGroups.getByRole('button',{name:/^(Confirm|确认)$/}).click();
  await expect(globalGroups.getByTestId('host-commune-group')).toHaveCount(1);
  const dispatch=globalGroups.getByTestId('commune-group-tasks');await dispatch.locator('summary').click();
  for(const checkbox of await dispatch.getByRole('checkbox').all())await checkbox.check();
  await dispatch.locator('textarea').fill('ui-cross-source-once');
  await dispatch.getByRole('button').filter({hasText:/2/}).click();
  await expect(dispatch.locator('[data-group-task-id]')).toHaveCount(1);
  evidence.checks.push('One desktop builder selects members from both sources and submits a real frozen cross-source protocol task');
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
  await page.getByTestId('host-commune-groups').locator('summary').first().click();
  await expect(page.getByTestId('host-commune-group')).toHaveCount(1);
  evidence.checks.push('Narrow Remote includes ordinary terminal candidate and the same one global group authority');
  await page.locator('.member-card .msg-input').first().fill('mobile-individual-once');
  await page.locator('.member-card .msg-row .send').first().click();
  const individualLog=path.join(root,workspaces[0].id,'prompt-log');
  await until(()=>fs.existsSync(individualLog)&&fs.readFileSync(individualLog,'utf8').includes('mobile-individual-once'));
  assert.equal(fs.readFileSync(individualLog,'utf8').split('mobile-individual-once').length-1,1);
  evidence.checks.push('Remote individual message reaches the original ACP process once through the production connection and Kernel command parser');
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
  assert.deepEqual(pageErrors, []); evidence.checks.push('Narrow mobile production Commune aggregates the same two source sessions through the real Runtime facade; not a Cloud-network test');
  await page.screenshot({ path: path.resolve(evidenceDir,'global-mobile.png') });
  evidence.result = 'PASS';
} catch (error) { evidence.result = 'FAIL'; evidence.error = String(error); process.exitCode = 1; }
finally {
  await browser?.close();
  if (endpoint?.pid === kernel?.pid) await call('POST', '/v1/shutdown').catch(() => {});
  if (vite?.pid) await new Promise(resolve => spawn('taskkill', ['/PID', String(vite.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' }).once('exit', resolve));
  evidence.fixtureDirectory = root; fs.writeFileSync(output, JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify({ result: evidence.result, evidence: output }));
}
