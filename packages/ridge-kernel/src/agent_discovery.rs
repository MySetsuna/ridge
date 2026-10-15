//! Process observations are candidates, never protocol membership or conversation identity.
use crate::{agent_profiles::AgentProfile, server::AppState};
use ridge_core::process_identity::{self, ProcessInstance};
use ridge_core::EventSink;
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, sync::Mutex, time::{Duration, Instant}};
use sysinfo::{ProcessesToUpdate, System};

#[derive(Clone)]
struct Process { pid: u32, parent: Option<u32>, image: String, arguments: Vec<String> }
static SNAPSHOT: Mutex<Option<(Instant, Vec<Process>)>> = Mutex::new(None);
static ATTEMPTS:Mutex<Option<HashMap<String,(Instant,String)>>> = Mutex::new(None);
fn candidate_key(row:&Value)->String{format!("{}:{}:{}:{}",row["workspaceId"],row["paneId"],row["process"]["pid"],row["process"]["startInstance"])}

fn stem(value: &str) -> String {
    value.rsplit(['/', '\\']).next().unwrap_or(value).to_ascii_lowercase()
        .trim_end_matches(".exe").trim_end_matches(".cmd").trim_end_matches(".ps1")
        .trim_end_matches(".js").to_owned()
}
fn profile<'a>(process: &Process, profiles: &'a [AgentProfile]) -> Option<&'a AgentProfile> {
    profiles.iter().find(|profile| profile.process_names.iter().any(|name| {
        let expected=stem(name);
        !expected.is_empty() && (stem(&process.image)==expected
            || process.arguments.iter().take(8).filter(|arg| !arg.starts_with('-')).any(|arg| stem(arg)==expected))
    }))
}
fn snapshot() -> Result<Vec<Process>, String> {
    let mut cache=SNAPSHOT.lock().map_err(|_| "Agent process observations unavailable")?;
    if let Some((at,rows))=cache.as_ref() { if at.elapsed()<Duration::from_millis(500) {return Ok(rows.clone());} }
    let mut system=System::new();system.refresh_processes(ProcessesToUpdate::All);
    let rows=system.processes().iter().map(|(pid,process)|Process {
        pid:pid.as_u32(),parent:process.parent().map(|pid|pid.as_u32()),
        image:process.name().to_string_lossy().into_owned(),
        arguments:process.cmd().iter().take(8).map(|arg|arg.to_string_lossy().into_owned()).collect(),
    }).collect::<Vec<_>>();
    *cache=Some((Instant::now(),rows.clone()));Ok(rows)
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
    let profiles:Vec<AgentProfile>=serde_json::from_value(crate::agent_profiles::catalog("list_agent_profiles",&json!({}))?)
        .map_err(|error|error.to_string())?;
    let rows=snapshot()?;let parents=rows.iter().map(|row|(row.pid,row.parent)).collect::<HashMap<_,_>>();
    let mut candidates=Vec::new();
    for pty in state.ptys.list() {
        if pty.workspace_id!=Some(workspace)||!panes.contains(&pty.id) {continue;}
        let (Some(pid),Some(start_instance))=(pty.child_pid,pty.child_start_instance.clone()) else {continue;};
        let root=ProcessInstance{pid,start_instance};
        if process_identity::validate(&root).is_err() {continue;}
        for row in rows.iter().filter(|row|belongs(root.pid,row.pid,&parents)) {
            let Some(profile)=profile(row,&profiles) else {continue;};
            // A wrapper with a recognized descendant represents one running CLI.
            if rows.iter().any(|child|child.pid!=row.pid&&belongs(row.pid,child.pid,&parents)&&self::profile(child,&profiles).is_some()) {continue;}
            let Ok(process)=process_identity::instance(row.pid) else {continue;};
            if !process_identity::descendant(&root,&process).unwrap_or(false) {continue;}
            let current=state.ptys.info(pty.id).map_err(|error|error.to_string())?;
            if current.child_pid!=Some(root.pid)||current.child_start_instance.as_deref()!=Some(&root.start_instance)
                ||current.workspace_id!=Some(workspace) {continue;}
            let mut candidate=json!({"workspaceId":workspace,"paneId":pty.id,"name":profile.id,
                "process":process,"connectionState":"discovered","reason":"尚未取得此进程的协议端点与会话身份；历史、派发与审批尚未接入"});
            let Ok(view)=state.ptys.agent_terminal_view(pty.id,workspace,14) else {continue;};
            candidate["terminalEpoch"]=view["terminalEpoch"].clone();
            candidate["recentOutput"]=json!(view["text"].as_str().unwrap_or("").chars().take(4096).collect::<String>());
            candidate["protocolConnected"]=json!(registered.contains(&(pty.id.to_string(),process.pid,process.start_instance.clone())));
            if let Ok(attempts)=ATTEMPTS.lock(){if let Some((_,reason))=attempts.as_ref().and_then(|attempts|attempts.get(&candidate_key(&candidate))){candidate["reason"]=json!(reason);}}
            candidates.push(candidate);
        }
    }
    candidates.sort_by_key(|row|format!("{}:{}",row["paneId"],row["process"]["pid"]));
    Ok(json!(candidates))
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
                    let Some(profile)=configured.iter().find(|profile|profile["id"]==candidate["name"]) else {continue;};
                    let retired=inspect.roster.lock().map_err(|_|"Agent roster unavailable")?;
                    if retired.agent_identities().into_iter().any(|member|retired.protocol_member_retired(&member.agent_id)&&member.pane_id==candidate["paneId"].as_str().unwrap_or("")&&member.protocol_binding.as_ref().is_some_and(|binding|serde_json::to_value(&binding.process).ok().as_ref()==Some(&candidate["process"]))){continue;}
                    drop(retired);
                    let key=candidate_key(candidate);
                    let throttled=ATTEMPTS.lock().map_err(|_|"discovery unavailable")?.as_ref().and_then(|attempts|attempts.get(&key)).is_some_and(|(at,_)|at.elapsed()<Duration::from_secs(30));
                    if throttled {continue;}
                    let descriptor=(||->Result<crate::commune::RegisterRequest,String>{
                        let connection=&profile["connection"];
                        let file=connection["descriptorFile"].as_str().ok_or("自动接入缺少 descriptorFile 配置")?;
                        let file=file.replace("{pid}",&candidate["process"]["pid"].to_string());
                        let file=std::path::Path::new(&file);
                        if !file.is_absolute(){return Err("会话描述路径须为绝对路径".into());}
                        use std::io::Read;
                        let mut bytes=Vec::new();std::fs::File::open(file).map_err(|_|"等待运行方提供会话连接描述")?.take(65537).read_to_end(&mut bytes).map_err(|_|"无法读取会话连接描述")?;
                        if bytes.len()>65536{return Err("会话连接描述超出大小限制".into());}
                        let mut descriptor:Value=serde_json::from_slice(&bytes).map_err(|_|"会话连接描述不是有效 JSON")?;
                        if descriptor["process"]!=candidate["process"] {return Err("连接描述的进程实例与当前终端不符".into());}
                        for field in ["workspaceId","paneId"] {if !descriptor[field].is_null()&&descriptor[field]!=candidate[field]{return Err("连接描述属于另一工作区或终端".into());}descriptor[field]=candidate[field].clone();}
                        if !connection["protocol"].is_string(){return Err("自动接入缺少协议类型".into());}
                        if !descriptor["protocol"].is_null()&&descriptor["protocol"]!=connection["protocol"]{return Err("连接描述协议与清单配置不符".into());}
                        descriptor["protocol"]=connection["protocol"].clone();
                        descriptor["automatic"]=json!(true);
                        serde_json::from_value(descriptor).map_err(|_|"会话连接描述字段无效".into())
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
