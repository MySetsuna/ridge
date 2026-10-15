import assert from 'node:assert/strict';
import {readFile,readdir} from 'node:fs/promises';
import {test} from 'vitest';

test('native command registration matches the presentation client OS whitelist',async()=>{
 const rust=await readFile('src-tauri/src/lib.rs','utf8');
 const frontend=await readFile('src/lib/transport/nativeRuntimeCore.ts','utf8');
 const registered=[...rust.match(/generate_handler!\[([\s\S]*?)\]/)[1].matchAll(/::(\w+)/g)].map(match=>match[1]).sort();
 const routed=[...frontend.match(/OS_COMMANDS = new Set\(\[([\s\S]*?)\]/)[1].matchAll(/'([^']+)'/g)].map(match=>match[1]).sort();
 assert.deepEqual(registered,routed);
 assert.equal(registered.length,9);
});

test('native build cannot restore retired business modules or domain state',async()=>{
 const shell=await readFile('src-tauri/src/lib.rs','utf8');
 assert.doesNotMatch(shell,/\bmod (?:engine|fs|hosts|lsp|remote_host_impl|teammate|types)\s*;/);
 const state=await readFile('src-tauri/src/state.rs','utf8');
 const fields=[...state.matchAll(/pub (\w+)\s*:/g)].map(match=>match[1]).sort();
 assert.deepEqual(fields,['quitting','startup_cli_cwd','startup_cwd_kind']);
 assert.doesNotMatch(shell,/GlobalEvent|spawn_event_forwarder|mpsc::channel|AppState::new\([^)]*event/);
 for(const directory of ['src-tauri/src','packages/ridge-cli/src','packages/ridge-kernel/src']){
 for(const file of await readdir(directory,{recursive:true})){
  if(!file.endsWith('.rs'))continue;
  const source=await readFile(`${directory}/${file}`,'utf8');
  assert.doesNotMatch(source,/docs[\\/]archive|desktop-retired-tree/,`${file} references archived implementation`);
 }
 }
});

test('frontend topology reports cannot restore a native Host business owner',async()=>{
 assert.doesNotMatch(await readFile('src/lib/stores/hosts.ts','utf8'),/register_frontend_host/);
 assert.doesNotMatch(await readFile('packages/ridge-kernel/src/remote_hosts.rs','utf8'),/register_frontend_host/);
 assert.doesNotMatch(await readFile('packages/ridge-kernel/src/foreign_panes.rs','utf8'),/frontend-remote-link/);
 const hosts=await readFile('src/lib/stores/hosts.ts','utf8');
 assert.doesNotMatch(hosts,/pump_host_output|get_outbound_stats|get_live_backpressure|get_foreign_history_tail|outboundLifecycleByKey|outboundPumpByHost/);
 const panel=await readFile('src/lib/components/hosts/HostsPanel.svelte','utf8');
 assert.doesNotMatch(panel,/hostReconnect|stepHostReconnect|cancelHostReconnect|pumpAllConnectedOutbound|runtimeOwned|hostExtraBadges/);
});

test('CLI presentation cannot restore its retired workspace or raw terminal owners',async()=>{
 const entry=await readFile('packages/ridge-cli/src/tui/mod.rs','utf8');
 assert.doesNotMatch(entry,/mod (?:workspace|scrollback)\s*;|LocalPtySession/);
 assert.doesNotMatch(entry,/mod lan_host\s*;|wait_tcp_ready|lan_host::run/);
 for(const file of ['session','pager','dashboard','canonical_view','lan_session','lan_proto']){
  const source=(await readFile(`packages/ridge-cli/src/tui/${file}.rs`,'utf8')).split('#[cfg(test)]')[0];
  assert.doesNotMatch(source,/TerminalProducer|ModeTracker|ScrollbackRing|TerminalInputLedger|poll_domain_pty_output|parse_binary_frame/,`${file} restored a retired owner`);
 }
});

test('Dashboard remote lifecycle belongs to Runtime and survives presentation detach',async()=>{
 const entry=await readFile('packages/ridge-cli/src/main.rs','utf8');
 const dashboard=await readFile('packages/ridge-cli/src/tui/dashboard.rs','utf8');
 assert.doesNotMatch(entry,/mod daemon_ctl\s*;/);
 assert.doesNotMatch(dashboard,/daemon_ctl|daemon_task|lan_shutdown_tx|lan_host::run/);
 for(const method of ['get_remote_info','set_remote_enabled','get_cloud_host_state','cloud_host_offline'])assert.ok(dashboard.includes(`"${method}"`));
});

test('native Cloud API proxy remains on the shared Runtime with server-side admission',async()=>{
 const shell=await readFile('src-tauri/src/lib.rs','utf8');
 const business=await readFile('packages/ridge-kernel/src/business.rs','utf8');
 const proxy=await readFile('packages/ridge-kernel/src/cloud/http.rs','utf8');
 assert.doesNotMatch(shell,/commands::cloud_http/);
 assert.match(business,/method=="cloud_http"[\s\S]*?crate::cloud::http::execute\(&request.args,request.native\)/);
 assert.match(proxy,/if !native\{return Err\(CoreError::CapabilityDenied/);
 assert.match(proxy,/!allowed_url\(attempt.url\(\)\)/);
});

test('desktop startup releases the mount barrier after selection without retired PTY reattachment',async()=>{
 const page=await readFile('src/routes/+page.svelte','utf8');
 assert.doesNotMatch(page,/invoke(?:<[^>]+>)?\('reattach_kernel_ptys'/);
 assert.match(page,/await ensureActiveWorkspace\(\);[\s\S]*?releaseDesktopKernelReattachGate\(\);/);
 const workspaces=await readFile('src/lib/stores/paneTree.ts','utf8');
 assert.doesNotMatch(workspaces,/claim_workspace_window|acquire_window_workspace|create_workspace_for_window|switch_window_workspace|get_window_active_workspace_id|get_window_pane_layout|set_window_split_ratios|\bserviceClient\b/);
});

test('pane membership uses actual protocol registration and fenced Runtime retirement',async()=>{
 const pane=await readFile('src/lib/components/SplitContainer.svelte','utf8');
 assert.doesNotMatch(pane,/register_teammate_agent|toggleTeammateAgent|node.agent_state/);
 assert.match(pane,/agentTopologyStore\[workspaceId\]/);
 assert.match(pane,/registerCommuneMember\(workspaceId, leafId\)/);
 assert.match(pane,/removeCommuneMember\(workspaceId, member\)/);
});

test('Remote panes do not restore name-based membership or offer native-only registration',async()=>{
 for(const file of ['packages/remote/src/shared/transport/wsRemote.ts','src/remote/lib/cloudRemote.ts','src/remote/lib/WorkspaceTree.svelte','src/lib/hosts/hostForest.ts','src/lib/stores/hosts.ts','src/lib/components/hosts/HostsPanel.svelte']){
  assert.doesNotMatch(await readFile(file,'utf8'),/markPaneAgent|register_teammate_agent|toggleAgentMark/);
 }
 const tree=await readFile('src/remote/lib/WorkspaceTree.svelte','utf8');
 assert.match(tree,/主机登记/);
 assert.doesNotMatch(await readFile('src/remote/lib/cloudRemote.ts','utf8'),/node.agent_state|node.agent_id/);
});

test('WebView never publishes Agent safety proofs or owns a PTY sampler',async()=>{
 const pane=await readFile('src/lib/components/RidgePane.svelte','utf8');
 assert.doesNotMatch(pane,/ptyRuntimeSnapshot|PTY_RUNTIME_|PtyRuntimeIdentity|publish_pty_runtime_snapshot|samplePtyRuntimeState|notePtyRuntimeInput/);
 const gateway=await readFile('packages/ridge-kernel/src/gateway_permissions.rs','utf8');
 const mcp=await readFile('packages/ridge-kernel/src/kernel_mcp.rs','utf8');
 for(const source of [gateway,mcp])assert.match(source,/registered Agent messages require the existing protocol/);
});
