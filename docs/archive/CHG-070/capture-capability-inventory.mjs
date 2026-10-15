// Read-only source census; writes migration evidence beside this script, never specifications.
import {readFile,readdir,writeFile} from 'node:fs/promises';
import {dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import ts from 'typescript';
const read=path=>readFile(path,'utf8');
async function files(root){const result=[];for(const entry of await readdir(root,{withFileTypes:true})){const path=`${root}/${entry.name}`;if(entry.isDirectory())result.push(...await files(path));else result.push(path);}return result;}
const methods=new Map(),events=new Map(),dynamic=[];
function add(map,name,path,kind){if(!map.has(name))map.set(name,[]);map.get(name).push({path,kind});}
for(const path of [...await files('src'),...await files('packages/remote/src')].filter(path=>/\.(ts|svelte)$/.test(path)&&!/(?:test|spec|Harness|generated)/.test(path))){
 let source=await read(path);if(path.endsWith('.svelte'))source=[...source.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)].map(match=>match[1]).join('\n');
 const tree=ts.createSourceFile(path,source,ts.ScriptTarget.Latest,true);
 const visit=node=>{if(ts.isCallExpression(node)&&/(?:^|\.)(invoke|listen|request|call|notify)$/.test(node.expression.getText(tree))&&node.arguments[0]){
  const kind=node.expression.getText(tree).split('.').at(-1),arg=node.arguments[0];
  if(ts.isStringLiteral(arg))add(kind==='listen'?events:methods,arg.text,path,kind);
  else dynamic.push({path,call:node.getText(tree).replace(/\s+/g,' ').slice(0,220)});
 }ts.forEachChild(node,visit);};visit(tree);
}
const native=await read('src/lib/transport/nativeRuntimeCore.ts');
const os=new Set([...native.match(/OS_COMMANDS = new Set\(\[([\s\S]*?)\]/)[1].matchAll(/'([^']+)'/g)].map(match=>match[1]));
for(const method of os)add(methods,method,'src/lib/transport/nativeRuntimeCore.ts','OS whitelist');
const caps=await read('packages/ridge-core/src/capability.rs');
const array=name=>[...caps.match(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`))[1].matchAll(/^\s*"([^"]+)",/gm)].map(match=>match[1]);
const remote=new Set(array('REMOTE_ALLOWLIST')),mutating=new Set(array('MUTATING_METHODS'));
for(const method of remote)add(methods,method,'packages/ridge-core/src/capability.rs','Remote declaration');
for(const method of ['git_cherry_pick_abort','git_revert_abort','dismiss_execution_rejection'])add(methods,method,'src/lib/components/SourceControl.svelte / src/lib/teammate/HitlApprovalModal.svelte','resolved dynamic call');
for(const path of (await files('packages/ridge-cli/src')).filter(path=>path.endsWith('.rs'))){
 const source=(await read(path)).split(/#\[cfg\(test\)\]\s*mod\s+tests\b/)[0];
 for(const match of source.matchAll(/(?:\.(?:request|invoke)|\b(?:command|rpc))\s*(?:::\s*<[^>]+>)?\(\s*"([a-z][a-z0-9_-]+)"|"method"\s*:\s*"([a-z][a-z0-9_-]+)"/g))add(methods,match[1]??match[2],path,'CLI literal control');
}
for(const [event,path]of [['editor-window-open-file','src/lib/stores/editorWindow.ts'],['editor-window-closed','src/lib/stores/editorWindow.ts'],['ridge://memory-reclaim','src/lib/transport/nativePresentationLifecycle.ts'],['ridge://memory-restore','src/lib/transport/nativePresentationLifecycle.ts'],['teammate://hitl-approval-required','src/lib/teammate/HitlApprovalModal.svelte'],['pane-prompt-${workspaceId}-${paneId}','src/lib/components/RidgePane.svelte'],['pane-cwd-changed-${workspaceId}-${paneId}','src/lib/stores/paneTree.ts']])add(events,event,path,'resolved dynamic event');
const paths=[...await files('packages/ridge-kernel/src'),...await files('packages/ridge-core/src'),...await files('src-tauri/src')].filter(path=>path.endsWith('.rs'));
// Some files place a test module before later production definitions. Preserve the
// whole source for discovery; references below still require an execution-path audit.
const rust=new Map(await Promise.all(paths.map(async path=>[path,await read(path)])));
const host=rust.get('packages/ridge-kernel/src/host.rs');
const hostMethods=new Set([...host.match(/fn is_kernel_host_method[\s\S]*?(?=\nfn panes_frame)/)[0].matchAll(/"([^"]+)"/g)].map(match=>match[1]));
const modules=['git_commands','clipboard_assets','terminal_ops','foreign_panes','outbound_hosts','cloud/manager','remote_control','lsp','commune','headless','remote_auth','identity','remote_hosts','projects','workspace_history','workspace_commands'];
const workspaceFiles=new Set(['save_workspace_to_file','open_workspace_from_file','get_workspace_save_info','list_workspace_save_info','get_default_workspace_save_dir','list_saved_workspace_files']);
const adapters=new Set(['register_pane_delta_channel','set_pane_delta_mode','take_terminal_frames']);
const rawLegacy=new Set(['get_pane_scrollback_tail','get_pane_scrollback_before','get_pane_resync_frame']);
const ref=path=>`[${path}](../../../${path})`;
function owner(method){
 if(method.startsWith('/'))return ['Cloud HTTP', 'packages/ridge-kernel/src/cloud/http.rs'];
 if(os.has(method))return ['OS presentation','src/lib/transport/nativeRuntimeCore.ts'];
 if(adapters.has(method))return ['Channel adapter','src/lib/transport/tauriShim/core.ts'];
 if(['subscribe-pane','unsubscribe-pane','use-global-workspace'].includes(method))return ['Transport control','packages/ridge-kernel/src/host.rs'];
 if(rawLegacy.has(method))return ['Legacy reserved','packages/ridge-core/src/dispatch.rs'];
 if(method==='resume_agent_session')return ['Retired automatic Agent resume','packages/remote/src/shared/transport/wsRemote.ts'];
 if(workspaceFiles.has(method))return ['Runtime files','packages/ridge-kernel/src/workspace_files.rs'];
 if(hostMethods.has(method))return ['Runtime transport','packages/ridge-kernel/src/host.rs'];
 if(method==='cloud_http')return ['Runtime Cloud HTTP','packages/ridge-kernel/src/cloud/http.rs'];
 if(['get_user_default_cwd','set_user_default_cwd'].includes(method))return ['Runtime settings','packages/ridge-kernel/src/settings.rs'];
 for(const module of modules){const path=`packages/ridge-kernel/src/${module}.rs`,source=rust.get(path);if(source?.includes(`"${method}"`))return ['Runtime '+module,path];}
 const core=rust.get('packages/ridge-core/src/dispatch.rs');if(core.includes(`"${method}"`))return ['Shared core','packages/ridge-core/src/dispatch.rs'];
 for(const [path,source]of rust)if(path.startsWith('packages/ridge-kernel/')&&source.includes(`"${method}"`))return ['Runtime source reference',path];
 return ['UNRESOLVED',''];
}
const rows=[...methods].sort().map(([method,refs])=>({method,refs,...Object.fromEntries(['category','owner'].map((key,index)=>[key,owner(method)[index]]))}));
const osEvents=new Set(['editor-window-open-file','editor-window-closed','ridge://memory-reclaim','ridge://memory-restore','ridge://auth-focus']);
const eventRows=[...events].sort().map(([name,refs])=>{
 const producers=[...rust].filter(([,source])=>source.includes(`"${name}"`)).map(([path])=>path);
 if(name.startsWith('pane-cwd-changed-'))producers.push('packages/ridge-kernel/src/workspace_commands.rs');
 if(name.startsWith('pane-prompt-')&&rust.get('packages/ridge-kernel/src/pty.rs')?.includes('"pane-prompt-{'))producers.push('packages/ridge-kernel/src/pty.rs');
 return {name,refs,producers,status:osEvents.has(name)?'OS presentation':producers.length===0?'GAP: emitter not established':'Runtime source reference'};
});
const unique=refs=>[...new Set(refs.map(item=>item.path))].map(ref).join('<br>');
let md=`# CHG-070 capability and event inventory\n\nMigration evidence; not an authoritative specification or a completion certificate. Regenerate from the repository root with \`node docs/archive/CHG-070/capture-capability-inventory.mjs\`.\n\nScope: live frontend literal invoke/listen/request/call/notify sites, the Native OS whitelist, Remote declarations, resolved dynamic Git/HITL/editor events, and CLI literal control requests. Generic dispatch forwarding, interpolated Cloud HTTP paths and other computed calls remain visible in the final table; they are not silently counted as covered. Source references identify routing candidates; their presence alone does not prove execution, authorization, event delivery or persistence.\n\n## Common boundaries and effects\n\n| Class | Authority / persistence | Permission / event boundary | Acceptance location |\n| --- | --- | --- | --- |\n| OS presentation | Native windows, clipboard access and unsaved editor window handoff; saved file I/O stays in Runtime | Exact OS command/event whitelist and Tauri plugin capabilities | nativeRuntimeCore.test.ts; nativeRuntimeEvent.test.ts |\n| Runtime transport | Kernel graph and PTY/terminal producer; connection-local selection/activation; canonical bounded history | Local grants or authenticated LAN/Cloud; workspace/pane/epoch fences; BusinessEvents | local_client.rs; conformance_runtime.rs; desktop-runtime-lifecycle-acceptance.mjs |\n| Git | Existing typed domain executors and ridge-core process guard; repository worktree and .git | Common capability/root admission; enclosing repository check; original Git result shapes; repository watch notifications | git_commands_runtime.rs; tauri.test.ts; ws.test.ts; remoteAllowlist.test.ts |\n| Files / projects / settings / themes | Existing Runtime stores and authorized filesystem paths; no WebView writer fallback | Common admission plus owner-specific root checks; fs/scm/default-cwd/workspace notifications | fs_watch_runtime.rs; projects_runtime.rs; settings_runtime.rs; theme_runtime.rs; workspace_file_owner.rs; migration_runtime.rs; [migration-runtime-acceptance.md](migration-runtime-acceptance.md) |\n| Agents / approvals / groups / history | Runtime Commune roster, existing conversation and durable task/history/permission stores | Existing protocol identity/generation/lease; native-only registration/removal; redacted Remote events | commune_registration.rs; gateway_permissions_runtime.rs; owned membership UI receipt; external user Agent still outstanding |\n| Hosts / Cloud / identity | Runtime connection managers, credentials, trust pins and persisted topology | Native-only Host management; Browser Remote keeps headless capability without second-hop Host controls; Cloud HTTP origin/redirect bounds | outbound_hosts_runtime.rs; host_topology_runtime.rs; cloud_http_runtime.rs; Host reconnect UI receipt; [controller identity receipt](controller-identity-acceptance.md); [NSIS exclusion receipt](installer-runtime-acceptance.md) |\n| Legacy reserved | Old raw-history command identifiers remain in policy data; current clients use canonical snapshot/history | Not treated as a working migration fallback or as acceptance evidence | Reconcile reserved declarations before final closure |\n\n## Commands and HTTP APIs (${rows.length})\n\nPermission column reports Remote list membership, not unrestricted execution. Additional role, root, approval and workspace checks still apply. A write flag reports the shared risk predicate; it is not an authorization grant.\n\n| Command / API | Class | Owner / routing reference | Admission | Call / declaration sources |\n| --- | --- | --- | --- | --- |\n`;
for(const row of rows){const permission=row.method.startsWith('/')?'Cloud authentication':os.has(row.method)?'Native OS':adapters.has(row.method)?'Local adapter':row.category==='Transport control'?'Authenticated transport':remote.has(row.method)?'Native + Remote-listed':'Native';md+=`| \`${row.method}\` | ${row.category}${mutating.has(row.method)?'; write':''} | ${row.owner?ref(row.owner):'UNRESOLVED'} | ${permission} | ${unique(row.refs)} |\n`;}
md+='\n## Dock execution receipt\n\n`dock_pane` now executes through Runtime workspace_commands, including cross-workspace PTY/foreign-view binding fences, empty-source archive and associated-file autosave. See [dock-runtime-acceptance.md](dock-runtime-acceptance.md) for the actual shell, registered protocol-process and authenticated TLS checks and their limits.\n';
md+=`\n## Events (${eventRows.length})\n\nRuntime source references must be followed to actual emission and subscription. Prompt and terminal-error delivery have an actual PTY/Native-grant execution receipt in [terminal-events-acceptance.md](terminal-events-acceptance.md). Natural exit and ordinary-shell rebuild now originate in Runtime; see [terminal-lifecycle-acceptance.md](terminal-lifecycle-acceptance.md) for actual child, PowerShell prompt, protocol-member and process-reclamation checks. Rendered Native/Remote lifecycle acceptance is recorded separately. The five obsolete global-focus/layout/activity/circuit consumers are archived; current tree changes and Commune session updates supply layout, roster and history refreshes. A zero source-emitter gap count does not certify all event behavior or complete CHG-070.\n\n| Event | Classification | Source reference | Consumers |\n| --- | --- | --- | --- |\n`;
for(const row of eventRows)md+=`| \`${row.name}\` | ${row.status} | ${row.producers.map(ref).join('<br>')||'No Runtime emitter established'} | ${unique(row.refs)} |\n`;
md+='\n## Computed calls and forwarding\n\nGit abort resolves to git_cherry_pick_abort/git_revert_abort; git_${op} resolves to fetch/pull/push/sync. File creation and transfer conditionals resolve to create_file/create_directory and copy_path/move_path. HITL resolution and rejection dismissal resolve to the named Runtime commands. Generic transport forwarding is governed by the common admission boundary; it does not establish a new business owner. The remaining source sites below require individual follow-through before the full inventory can be closed.\n\n| Source | Computed call / forwarding |\n| --- | --- |\n';
for(const item of dynamic)md+=`| ${ref(item.path)} | \`${item.call.replaceAll('`',"'").replaceAll('|','&#124;')}\` |\n`;
await writeFile(join(dirname(fileURLToPath(import.meta.url)),'capability-inventory.md'),md);
console.log(JSON.stringify({commands:rows.length,events:eventRows.length,computedCalls:dynamic.length,unresolvedCommands:rows.filter(row=>row.category==='UNRESOLVED').map(row=>row.method),eventGaps:eventRows.filter(row=>row.status.startsWith('GAP')).map(row=>row.name)}));
