//! Process observations are candidates, never protocol membership or conversation identity.
use crate::{agent_profiles::AgentProfile, server::AppState};
use ridge_core::process_identity::{self, ProcessInstance};
use ridge_core::EventSink;
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, sync::Mutex, time::{Duration, Instant}};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Clone)]
pub(crate) struct Candidate {
    pub source: crate::commune_groups::MemberSource,
    pub name: String,
    declared: Option<Value>,
}

pub(crate) fn resolve(state: &AppState, reference: &str) -> Result<Candidate, String> {
    let candidate = state.commune_candidates.lock().map_err(|_| "candidate authority unavailable")?
        .get(reference).cloned().ok_or("candidate expired; refresh discovery")?;
    let rows = read(state, &candidate.source.workspace_id)?;
    if !rows.as_array().into_iter().flatten().any(|row| row["candidateRef"] == reference) {
        return Err("candidate process or terminal changed; refresh discovery".into());
    }
    let current = state.commune_candidates.lock().map_err(|_| "candidate authority unavailable")?
        .get(reference).filter(|current| current.source == candidate.source)
        .cloned().ok_or("candidate process or terminal changed; refresh discovery")?;
    process_identity::validate(&current.source.process).map_err(|error| error.to_string())?;
    Ok(current)
}

#[derive(Clone)]
struct Process { pid: u32, parent: Option<u32>, image: String, arguments: Vec<String> }
static SNAPSHOT: Mutex<Option<(Instant, Vec<u32>, Vec<Process>)>> = Mutex::new(None);
static ATTEMPTS:Mutex<Option<HashMap<String,(Instant,String)>>> = Mutex::new(None);
fn candidate_key(row:&Value)->String{format!("{}:{}:{}:{}",row["workspaceId"],row["paneId"],row["process"]["pid"],row["process"]["startInstance"])}

fn read_descriptor(file:&std::path::Path,process:&ProcessInstance)->Result<Value,String> {
    use std::io::Read;
    if !file.is_absolute(){return Err("connection descriptor must have an absolute path".into());}
    let mut bytes=Vec::new();
    std::fs::File::open(file).map_err(|_|"Agent has not published a connection endpoint; start a collaboration terminal with ridge agent, or use an advanced connection file")?
        .take(65537).read_to_end(&mut bytes).map_err(|_|"could not read the published endpoint")?;
    if bytes.len()>65536{return Err("connection descriptor exceeds its size budget".into());}
    let descriptor:Value=serde_json::from_slice(&bytes).map_err(|_|"invalid published connection descriptor")?;
    if descriptor["process"]!=serde_json::to_value(process).map_err(|error|error.to_string())? {return Err("published process instance changed; refresh discovery".into());}
    Ok(descriptor)
}

pub(crate) fn prepare(state:&AppState,args:&Value)->Result<Value,String> {
    let reference=if let Some(reference)=args["candidateRef"].as_str(){reference.to_owned()} else {
        let rows=read(state,args["workspaceId"].as_str().ok_or("workspaceId required")?)?;
        let candidates=rows.as_array().ok_or("candidate list unavailable")?.iter().filter(|row|row["paneId"]==args["paneId"]).collect::<Vec<_>>();
        if candidates.len()!=1{return Err("select one exact Agent candidate before connecting".into());}
        candidates[0]["candidateRef"].as_str().ok_or("candidate reference unavailable")?.to_owned()
    };
    let candidate=resolve(state,&reference)?;
    for (field,expected) in [("workspaceId",&candidate.source.workspace_id),("paneId",&candidate.source.pane_id)] {
        if !args[field].is_null()&&args[field]!=*expected{return Err("candidate belongs to another terminal".into());}
    }
    let catalog=crate::agent_profiles::catalog("list_agent_profiles",&json!({}))?;
    let profile=catalog.as_array().and_then(|profiles|profiles.iter().find(|profile|profile["id"]==candidate.name));
    let published=crate::managed_agent::descriptor_path(&candidate.source.process);
    let connection = profile.map(|profile| &profile["connection"]);
    let mut descriptor = if published.is_file() {
        read_descriptor(&published, &candidate.source.process)?
    } else if let Some(file) = connection.and_then(|connection| connection["descriptorFile"].as_str()) {
        let path=file.replace("{pid}",&candidate.source.process.pid.to_string());
        let mut descriptor=read_descriptor(std::path::Path::new(&path),&candidate.source.process)?;
        if let Some(protocol)=connection.and_then(|connection|connection["protocol"].as_str()) {
            if !descriptor["protocol"].is_null()&&descriptor["protocol"]!=protocol{return Err("published protocol does not match the configured connection".into());}
            descriptor["protocol"]=json!(protocol);
        }
        descriptor
    } else {
        candidate.declared.ok_or("无法确认原终端的会话归属 (cannot verify this terminal's session ownership). This Agent has no published or declared local endpoint. It remains groupable; use an explicit collaboration terminal or a verified advanced connection")?
    };
    let object=descriptor.as_object_mut().ok_or("invalid connection descriptor")?;
    object.remove("agentName");
    for (field,expected) in [("workspaceId",&candidate.source.workspace_id),("paneId",&candidate.source.pane_id)] {
        if !object.get(field).is_none_or(|value|value.is_null()||value==expected){return Err("published connection belongs to another terminal".into());}
        object.insert(field.into(),json!(expected));
    }
    object.insert("automatic".into(),json!(false));
    object.insert("expectedTerminalEpoch".into(),json!(candidate.source.terminal_epoch));
    if let Some(member)=args["memberAgentId"].as_str(){object.insert("memberAgentId".into(),json!(member));}
    // Parse at the native authority before any socket is opened. The actual
    // registration repeats source and OS socket ownership validation.
    let _:crate::commune::RegisterRequest=serde_json::from_value(descriptor.clone()).map_err(|error|error.to_string())?;
    descriptor["candidateRef"]=json!(reference);
    Ok(descriptor)
}

fn stem(value: &str) -> String {
    let name = value.rsplit(['/', '\\']).next().unwrap_or(value).to_ascii_lowercase();
    [".exe", ".cmd", ".ps1", ".js", ".mjs", ".cjs", ".py"]
        .iter().find_map(|suffix| name.strip_suffix(suffix)).unwrap_or(&name).to_owned()
}
fn script_entry(process: &Process) -> Option<&str> {
    let image = stem(&process.image);
    if !["node", "nodejs", "bun", "deno", "python", "python3"].contains(&image.as_str()) {
        return None;
    }
    let mut arguments = process.arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--" => return arguments.next().map(String::as_str),
            "-m" if image.starts_with("python") => return arguments.next().map(String::as_str),
            "run" if image == "deno" || image == "bun" => continue,
            "-r" | "--require" | "--loader" | "--import" | "--title" | "--conditions" => { arguments.next()?; }
            "--enable-source-maps" | "--no-warnings" | "--experimental-strip-types" | "-u" | "-B" => {}
            value if value.starts_with('-') && value.contains('=')
                && !value.starts_with("--eval=") && !value.starts_with("--print=") => {}
            value if value.starts_with('-') => return None,
            value => return Some(value),
        }
    }
    None
}
fn entry_matches(entry: &str, expected: &str) -> bool {
    if stem(entry) == expected { return true; }
    let normalized = entry.replace('\\', "/");
    let Some((_, package)) = normalized.rsplit_once("/node_modules/") else { return false; };
    let mut parts = package.split('/');
    let Some(first) = parts.next() else { return false; };
    let name = if first.starts_with('@') { parts.next().unwrap_or("") } else { first };
    name == expected
}
fn profile<'a>(process: &Process, profiles: &'a [AgentProfile]) -> Option<&'a AgentProfile> {
    let entry = script_entry(process);
    profiles.iter().find(|profile| profile.process_names.iter().any(|name| {
        let expected=stem(name);
        !expected.is_empty() && (stem(&process.image)==expected
            || entry.is_some_and(|entry| entry_matches(entry, &expected)))
    }))
}
fn declared_endpoint(
    row: &Process,
    profile: &AgentProfile,
    recipe: &Value,
    cwd: &str,
    process: &ProcessInstance,
) -> Option<Value> {
    use crate::commune_endpoint::{local_path, LocalEndpoint};
    let cwd = std::path::Path::new(cwd);
    if row.arguments.len() > 32
        || !profile
            .process_names
            .iter()
            .any(|name| stem(name) == stem(&row.image))
        || !local_path(cwd)
    {
        return None;
    }
    let command = recipe["command"]
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    if command.is_empty() {
        return None;
    }
    let mut args = row.arguments.iter().skip(1).map(String::as_str);
    if !command.iter().all(|part| args.next() == Some(*part)) {
        return None;
    }
    let endpoint_flag = recipe["endpointFlag"].as_str()?;
    let credential_flag = recipe["credentialFileFlag"].as_str();
    let auth_flag = recipe["authFlag"].as_str();
    let protocol = recipe["protocol"]
        .as_str()
        .filter(|protocol| matches!(*protocol, "acp" | "acp-ws" | "app-server"))?;
    let mut endpoint = None;
    let mut credential = None;
    let mut authenticated = false;
    while let Some(arg) = args.next() {
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg, None), |(flag, value)| (flag, Some(value)));
        let value = inline
            .or_else(|| args.next())
            .filter(|value| !value.is_empty() && !value.starts_with('-'))?;
        if flag == endpoint_flag && endpoint.is_none() {
            endpoint = Some(LocalEndpoint::try_from(value.to_owned()).ok()?);
        } else if Some(flag) == credential_flag
            && credential.is_none()
            && local_path(std::path::Path::new(value))
        {
            credential = Some(value);
        } else if Some(flag) == auth_flag && !authenticated && recipe["authValue"] == value {
            authenticated = true;
        } else {
            return None;
        }
    }
    if (authenticated && credential.is_none())
        || (protocol == "acp"
            && (credential.is_some() || matches!(&endpoint, Some(LocalEndpoint::WebSocket(..)))))
    {
        return None;
    }
    let cwd = cwd
        .canonicalize()
        .ok()
        .filter(|path| local_path(path))
        .unwrap_or_else(|| cwd.components().collect());
    let cwd = cwd.to_str()?;
    #[cfg(windows)]
    let cwd = cwd.strip_prefix(r"\\?\").unwrap_or(cwd);
    let mut descriptor = json!({"protocol":protocol, "endpoint":endpoint?.to_string(),
        "process":process, "endpointOwner":process, "cwd":cwd});
    if let Some(path) = credential {
        descriptor["bearerTokenFile"] = json!(path);
    }
    Some(descriptor)
}

fn snapshot(roots: Vec<u32>) -> Result<Vec<Process>, String> {
    let mut cache = SNAPSHOT
        .lock()
        .map_err(|_| "Agent process observations unavailable")?;
    if let Some((at, cached_roots, rows)) = cache.as_ref() {
        if at.elapsed() < Duration::from_millis(500) && cached_roots == &roots {
            return Ok(rows.clone());
        }
    }
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let parents = system
        .processes()
        .iter()
        .map(|(pid, process)| (pid.as_u32(), process.parent().map(|pid| pid.as_u32())))
        .collect::<HashMap<_, _>>();
    let local = system
        .processes()
        .iter()
        .filter(|(pid, process)| {
            process.thread_kind().is_none()
                && roots
                    .iter()
                    .any(|root| belongs(*root, pid.as_u32(), &parents))
        })
        .map(|(pid, _)| *pid)
        .collect::<Vec<_>>();
    // sysinfo's default process refresh omits argv. Read it only for owned pane trees.
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&local),
        ProcessRefreshKind::new().with_cmd(UpdateKind::OnlyIfNotSet),
    );
    let rows = system
        .processes()
        .iter()
        .filter(|(_, process)| process.thread_kind().is_none())
        .map(|(pid, process)| Process {
            pid: pid.as_u32(),
            parent: process.parent().map(|pid| pid.as_u32()),
            image: process
                .exe()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| process.name().to_string_lossy().into_owned()),
            arguments: process
                .cmd()
                .iter()
                .take(33)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        })
        .collect::<Vec<_>>();
    *cache = Some((Instant::now(), roots, rows.clone()));
    Ok(rows)
}

fn belongs(root: u32, target: u32, parents: &HashMap<u32,Option<u32>>) -> bool {
    let mut pid=target;let mut visited=HashSet::new();
    for _ in 0..32 {
        if pid==root {return true;}
        if !visited.insert(pid) {return false;}
        let Some(Some(parent))=parents.get(&pid) else {return false;};pid=*parent;
    }
    false
}
pub(crate) fn read(state: &AppState, workspace: &str) -> Result<Value,String> {
    let workspace=uuid::Uuid::parse_str(workspace).map_err(|_| "invalid workspaceId")?;
    let graph=state.workspaces.lock().map_err(|_| "workspace unavailable")?;
    let panes=graph.workspace(workspace).ok_or("workspace not found")?.pane_tree.panes.keys().copied().collect::<HashSet<_>>();
    drop(graph);
    let registered=state.roster.lock().map_err(|_| "Agent roster unavailable")?.agent_identities().into_iter()
        .filter(|member| member.workspace_id==workspace.to_string()&&member.online&&member.protocol_binding.is_some())
        .filter_map(|member|member.protocol_binding.as_ref().map(|binding|(member.pane_id.clone(),binding.process.pid,binding.process.start_instance.clone())))
        .collect::<HashSet<_>>();
    let catalog = crate::agent_profiles::list_profiles()?;
    let profiles: Vec<AgentProfile> = serde_json::from_value(catalog.clone()).map_err(|error| error.to_string())?;
    let recipes = catalog.as_array().ok_or("invalid Agent catalog")?.iter()
        .filter_map(|profile| Some((profile["id"].as_str()?, profile.get("listener")?)))
        .collect::<HashMap<_, _>>();
    let ptys = state.ptys.list();
    let mut roots = ptys.iter().filter_map(|pty| pty.workspace_id.and(pty.child_pid)).collect::<Vec<_>>();
    roots.sort_unstable();
    roots.dedup();
    let rows = snapshot(roots)?;
    let parents = rows.iter().map(|row| (row.pid, row.parent)).collect::<HashMap<_, _>>();
    let mut candidates = Vec::new();
    for pty in ptys {
        if pty.workspace_id!=Some(workspace)||!panes.contains(&pty.id) {continue;}
        let (Some(pid),Some(start_instance))=(pty.child_pid,pty.child_start_instance.clone()) else {continue;};
        let root=ProcessInstance{pid,start_instance};
        if process_identity::validate(&root).is_err() {continue;}
        let published_roots=rows.iter().filter(|row|belongs(root.pid,row.pid,&parents)).filter_map(|row| {
            let process=process_identity::instance(row.pid).ok()?;
            read_descriptor(&crate::managed_agent::descriptor_path(&process),&process).ok().map(|descriptor|(row.pid,descriptor))
        }).collect::<HashMap<_,_>>();
        for row in rows.iter().filter(|row|belongs(root.pid,row.pid,&parents)) {
            if published_roots.keys().any(|ancestor|*ancestor!=row.pid&&belongs(*ancestor,row.pid,&parents)){continue;}
            let known=profile(row,&profiles);
            let Ok(process)=process_identity::instance(row.pid) else {continue;};
            let published=published_roots.get(&row.pid);
            let Some(name)=known.map(|profile|profile.id.clone()).or_else(||published.as_ref().and_then(|descriptor|descriptor["agentName"].as_str()).map(str::to_owned)) else {continue;};
            // A wrapper with a recognized descendant represents one running CLI.
            if published.is_none() && rows.iter().any(|child|child.pid!=row.pid&&belongs(row.pid,child.pid,&parents)&&(self::profile(child,&profiles).is_some()||published_roots.contains_key(&child.pid))) {continue;}
            if !process_identity::descendant(&root,&process).unwrap_or(false) {continue;}
            let current=state.ptys.info(pty.id).map_err(|error|error.to_string())?;
            if current.child_pid!=Some(root.pid)||current.child_start_instance.as_deref()!=Some(&root.start_instance)
                ||current.workspace_id!=Some(workspace) {continue;}
            let mut candidate=json!({"workspaceId":workspace,"paneId":pty.id,"name":name,
                "process":process,"connectionState":"discovered","reason":"尚未取得此进程的协议端点与会话身份；历史、派发与审批尚未接入"});
            let Ok(view)=state.ptys.agent_terminal_view(pty.id,workspace,14) else {continue;};
            candidate["terminalEpoch"]=view["terminalEpoch"].clone();
            candidate["recentOutput"]=json!(view["text"].as_str().unwrap_or("").chars().take(4096).collect::<String>());
            candidate["protocolConnected"]=json!(registered.contains(&(pty.id.to_string(),process.pid,process.start_instance.clone())));
            let declared = known.and_then(|profile| {
                declared_endpoint(row, profile, recipes.get(profile.id.as_str())?, pty.cwd.as_deref()?, &process)
            });
            candidate["connectionReady"] = json!(published.is_some() || declared.is_some());
            if let Some(descriptor) = published {
                candidate["reason"] = if descriptor["sessionId"].is_string() {
                    json!("已发现此终端公布的真实会话，可一键接入")
                } else { json!("已发现此终端的协议端点，可查看已有会话") };
            }
            if declared.is_some() && published.is_none() {
                candidate["reason"] = json!("已发现此终端声明的本地协议端点，可查看已有会话");
            }
            if let Ok(attempts)=ATTEMPTS.lock(){if let Some((_,reason))=attempts.as_ref().and_then(|attempts|attempts.get(&candidate_key(&candidate))){candidate["reason"]=json!(reason);}}
            let source = crate::commune_groups::MemberSource {
                host_id: state.host_id.clone(), workspace_id: workspace.to_string(),
                pane_id: pty.id.to_string(), terminal_epoch: view["terminalEpoch"].as_str().ok_or("terminal epoch unavailable")?.into(),
                process: process.clone(),
            };
            let mut references = state.commune_candidates.lock().map_err(|_| "candidate authority unavailable")?;
            let reference = references.iter().find(|(_, value)| value.source == source).map(|(key, _)| key.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            references.insert(reference.clone(), Candidate { source, name, declared });
            candidate["candidateRef"] = json!(reference);
            drop(references);
            let mut member_ids = state.roster.lock().map_err(|_| "roster unavailable")?.agent_identities().into_iter()
                .filter(|identity| identity.workspace_id == workspace.to_string() && identity.pane_id == pty.id.to_string()
                    && identity.protocol_binding.as_ref().is_some_and(|binding| binding.process == process))
                .map(|identity| identity.agent_id.clone()).collect::<HashSet<_>>();
            if let Some(id) = state.groups.lock().map_err(|_| "group authority unavailable")?.member_for_source(&crate::commune_groups::MemberSource {
                host_id: state.host_id.clone(), workspace_id: workspace.to_string(), pane_id: pty.id.to_string(),
                terminal_epoch: view["terminalEpoch"].as_str().unwrap_or("").into(), process: process.clone(),
            })? { member_ids.insert(id); }
            if member_ids.len() == 1 { candidate["agentId"] = json!(member_ids.into_iter().next()); }
            candidates.push(candidate);
        }
    }
    let live = candidates.iter().filter_map(|row| row["candidateRef"].as_str()).collect::<HashSet<_>>();
    state.commune_candidates.lock().map_err(|_| "candidate authority unavailable")?
        .retain(|reference, candidate| candidate.source.workspace_id != workspace.to_string() || live.contains(reference.as_str()));
    candidates.sort_by_key(|row|format!("{}:{}",row["paneId"],row["process"]["pid"]));
    Ok(json!(candidates))
}

/// Remote may already view these terminals; discovery adds no process/control grant.
pub(crate) fn public_projection(rows: Value) -> Value {
    json!(rows.as_array().into_iter().flatten().map(|row| json!({
        "workspaceId":row["workspaceId"],"paneId":row["paneId"],"name":row["name"],
        "connectionState":"discovered","protocolConnected":row["protocolConnected"]==true,"candidateRef":row["candidateRef"],"agentId":row["agentId"],
        "recentOutput":row["recentOutput"].as_str().unwrap_or("").chars().take(4096).collect::<String>()
    })).collect::<Vec<_>>())
}

/// Explicit catalog opt-in authorizes attaching a running party's descriptor.
/// Discovery never invents session ids, probes arbitrary ports, or launches a CLI.
pub(crate) fn supervise(state:AppState){tokio::spawn(async move {
    while !state.shutting_down.load(std::sync::atomic::Ordering::Acquire) {
        let inspect=state.clone();
        let prepared=tokio::task::spawn_blocking(move||->Result<Vec<(String,crate::commune::RegisterRequest)>,String>{
            let catalog=crate::agent_profiles::catalog("list_agent_profiles",&json!({}))?;
            let configured=catalog.as_array().ok_or("invalid Agent catalog")?.iter().filter(|row|row["connection"]["autoConnect"]==true).collect::<Vec<_>>();
            if configured.is_empty(){return Ok(Vec::new());}
            let workspaces=inspect.workspaces.lock().map_err(|_|"workspace unavailable")?.workspace_ids().copied().collect::<Vec<_>>();
            let mut requests=Vec::new();
            for workspace in workspaces {
                let candidates=read(&inspect,&workspace.to_string())?;
                for candidate in candidates.as_array().ok_or("invalid candidates")? {
                    if !configured.iter().any(|profile|profile["id"]==candidate["name"]){continue;}
                    let retired=inspect.roster.lock().map_err(|_|"Agent roster unavailable")?;
                    if retired.agent_identities().into_iter().any(|member|retired.protocol_member_retired(&member.agent_id)&&member.pane_id==candidate["paneId"].as_str().unwrap_or("")&&member.protocol_binding.as_ref().is_some_and(|binding|serde_json::to_value(&binding.process).ok().as_ref()==Some(&candidate["process"]))){continue;}
                    drop(retired);
                    let key=candidate_key(candidate);
                    let throttled=ATTEMPTS.lock().map_err(|_|"discovery unavailable")?.as_ref().and_then(|attempts|attempts.get(&key)).is_some_and(|(at,_)|at.elapsed()<Duration::from_secs(30));
                    if throttled {continue;}
                    let descriptor=(||->Result<crate::commune::RegisterRequest,String>{
                        let mut descriptor=prepare(&inspect,candidate)?;
                        if !descriptor["sessionId"].as_str().is_some_and(|id| !id.trim().is_empty()) {
                            return Err("请先选择要接入的已有会话".into());
                        }
                        descriptor.as_object_mut().map(|object|object.remove("candidateRef"));
                        descriptor["automatic"]=json!(true);
                        serde_json::from_value(descriptor).map_err(|_|"invalid automatically prepared connection".into())
                    })();
                    let reason=match descriptor {Ok(request)=>{requests.push((key.clone(),request));"正在连接已发现的协议会话".to_owned()},Err(reason)=>reason};
                    let mut attempts=ATTEMPTS.lock().map_err(|_|"discovery unavailable")?;let attempts=attempts.get_or_insert_with(HashMap::new);
                    if attempts.len()>=512{attempts.retain(|_,(at,_)|at.elapsed()<Duration::from_secs(60));}
                    attempts.insert(key,(Instant::now(),reason));
                }
            }
            Ok(requests)
        }).await;
        match prepared {
            Ok(Ok(requests))=>for (key,request) in requests {
                let result=crate::commune::attach(&state,request).await;
                if let Ok(mut attempts)=ATTEMPTS.lock(){attempts.get_or_insert_with(HashMap::new).insert(key,(Instant::now(),match result{Ok(_)=>"协议会话已接入".into(),Err(reason)=>format!("协议接入失败：{reason}")}));}
                state.business_events.broadcast("commune-members-changed",Value::Null);
            },
            Ok(Err(reason))=>tracing::warn!(%reason,"Agent connection discovery unavailable"),
            Err(error)=>tracing::warn!(%error,"Agent discovery task failed"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
});}

#[cfg(test)]
mod tests {
    use super::*;
    fn observed(image: &str, arguments: &[&str]) -> Process {
        Process { pid: 3, parent: Some(2), image: image.into(), arguments: arguments.iter().map(|value| value.to_string()).collect() }
    }

    #[test]
    fn listener_declaration_requires_an_exact_command_and_one_local_endpoint() {
        let profiles = crate::agent_profiles::builtin_profiles();
        let codex = profiles.iter().find(|profile| profile.id == "codex").unwrap();
        let process = ProcessInstance { pid: 3, start_instance: "owned".into() };
        let recipe = json!({"command":["app-server"],"endpointFlag":"--listen","protocol":"app-server"});
        let cwd = std::env::current_dir().unwrap();
        let cwd = cwd.to_str().unwrap();
        let endpoint = format!("unix://{}", std::path::Path::new(cwd).join("control.sock").display());
        let explicit = observed("codex.exe", &["codex", "app-server", "--listen", &endpoint]);
        let descriptor = declared_endpoint(&explicit, codex, &recipe, cwd, &process).unwrap();
        assert_eq!(descriptor["endpoint"], endpoint);
        let trailing_separator = format!("{cwd}/");
        let equivalent = declared_endpoint(&explicit, codex, &recipe, &trailing_separator, &process).unwrap();
        assert_eq!(equivalent["cwd"], descriptor["cwd"]);
        assert!(descriptor.get("sessionId").is_none());
        let ws_endpoint = "ws://127.0.0.1:2419/ws";
        let explicit = observed("codex.exe", &["codex", "app-server", "--listen", ws_endpoint]);
        let descriptor = declared_endpoint(&explicit, codex, &recipe, cwd, &process).unwrap();
        assert_eq!(descriptor["endpoint"], ws_endpoint);
        for arguments in [
            vec!["codex", "exec", "--listen", &endpoint],
            vec!["codex", "app-server", "proxy", "--listen", &endpoint],
            vec!["codex", "app-server", "--listen", &endpoint, "--listen", "ws://127.0.0.1:1234"],
            vec!["codex", "app-server", "--listen", "ws://192.0.2.1:1234"],
            vec!["codex", "app-server", "--listen", "unix://"],
        ] {
            assert!(declared_endpoint(&observed("codex.exe", &arguments), codex, &recipe, cwd, &process).is_none());
        }
    }

    #[test]
    fn builtin_agents_and_acp_package_entrypoints_are_recognized() {
        let profiles = crate::agent_profiles::builtin_profiles();
        for (image, arguments, expected) in [
            ("codex.exe", vec!["codex.exe", "resume", "session"], "codex"),
            ("claude.exe", vec!["claude.exe"], "claude"),
            ("grok.exe", vec!["grok.exe", "agent", "stdio"], "grok"),
            ("opencode.exe", vec!["opencode.exe"], "opencode"),
            ("node.exe", vec!["node.exe", "--enable-source-maps", "C:/bin/opencode.mjs"], "opencode"),
            ("node", vec!["node", "/usr/lib/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js"], "claude"),
            ("node", vec!["node", "/usr/lib/node_modules/@agentclientprotocol/codex-acp/dist/index.js"], "codex"),
            ("bun", vec!["bun", "run", "/usr/bin/opencode.js"], "opencode"),
        ] {
            assert_eq!(profile(&observed(image, &arguments), &profiles).map(|profile| profile.id.as_str()), Some(expected), "{image} {arguments:?}");
        }
    }

    #[test]
    fn prompt_text_and_interpreter_option_values_never_identify_an_agent() {
        let profiles = crate::agent_profiles::builtin_profiles();
        for (image, arguments) in [
            ("echo.exe", vec!["echo.exe", "codex"]),
            ("node.exe", vec!["node.exe", "C:/tools/ordinary.js", "claude"]),
            ("node", vec!["node", "--title", "codex", "/tmp/ordinary.js"]),
            ("node", vec!["node", "--require", "grok", "/tmp/ordinary.js"]),
            ("node", vec!["node", "--eval", "opencode"]),
            ("node", vec!["node", "--eval=codex", "claude"]),
            ("python", vec!["python", "-c", "grok"]),
            ("python", vec!["python", "/tmp/ordinary.py", "/work/codex"]),
        ] {
            assert!(profile(&observed(image, &arguments), &profiles).is_none(), "{image} {arguments:?}");
        }
    }

    #[test]
    fn custom_catalog_matches_wrappers_without_brand_rules_and_parent_scope_is_exact() {
        let profile=AgentProfile{id:"custom".into(),process_names:vec!["user-agent".into()],executable:"user-agent".into(),resume_argv:vec![],yolo_args:vec![],yolo_position:"before".into()};
        let process=Process{pid:3,parent:Some(2),image:"node.exe".into(),arguments:vec!["node.exe".into(),"C:\\tools\\user-agent.js".into()]};
        assert!(self::profile(&process,&[profile]).is_some());
        let parents=HashMap::from([(1,None),(2,Some(1)),(3,Some(2)),(4,None)]);
        assert!(belongs(1,3,&parents));assert!(!belongs(4,3,&parents));
        assert!(!belongs(1,3,&HashMap::from([(3,Some(2)),(2,Some(3))])));
    }
}
