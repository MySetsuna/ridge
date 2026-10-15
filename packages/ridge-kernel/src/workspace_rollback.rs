//! Runtime-owned Git worktree checkpoint; no checkout, delete or Agent execution occurs here.
use serde::{Serialize,Deserialize};
use std::path::Path;
use uuid::Uuid;
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase")]
pub struct RollbackPatch {pub ts:u64,pub label:String,pub porcelain:String,#[serde(default,skip_serializing_if="Option::is_none")]pub porcelain_z:Option<String>,pub diff:String,pub paths:Vec<String>,#[serde(default,skip_serializing_if="Option::is_none")]pub workspace_root:Option<String>}
pub fn paths_from_porcelain(porcelain:&str)->Vec<String>{
 let mut paths=Vec::new();for line in porcelain.lines(){if line.len()<4{continue;}let rest=line[3..].trim();if rest.is_empty(){continue;}let path=rest.split_once(" -> ").map_or_else(||rest.trim_matches('"'),|(_,to)|to);let path=path.replace('\\',"/");if !path.is_empty()&&!paths.contains(&path){paths.push(path);}}paths
}
async fn git(root:&Path,args:&[&str])->Result<String,String>{
 let out=ridge_core::commands::git::run_git_guarded(root.to_string_lossy().into_owned(),args.iter().map(|arg|arg.to_string()).collect()).await.map_err(|error|error.to_string())?;
 if !out.status.success(){return Err(format!("git {} failed: {}",args.join(" "),String::from_utf8_lossy(&out.stderr)));}Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
pub(crate) async fn checkpoint(state:&crate::server::AppState,args:&serde_json::Value)->Result<serde_json::Value,String>{
 let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or("missing workspaceId")?).map_err(|_|"invalid workspaceId")?;
 let root=std::path::PathBuf::from(args["workspaceRoot"].as_str().ok_or("missing workspaceRoot")?).canonicalize().map_err(|error|error.to_string())?;
 if !state.fs_scope.is_allowed(&root.to_string_lossy()){return Err("checkpoint root outside sandbox".into());}
 if git(&root,&["rev-parse","--is-inside-work-tree"]).await?.trim()!="true"{return Err("not a git repository".into());}
 let porcelain=git(&root,&["status","--porcelain","--",".",":(exclude).ridge-rollback-archive"]).await?;let diff=git(&root,&["diff","HEAD","--",".",":(exclude).ridge-rollback-archive"]).await?;
 let porcelain_z=git(&root,&["status","--porcelain","-z","--",".",":(exclude).ridge-rollback-archive"]).await?;
 let (tracked,untracked)=nul_targets(&porcelain_z)?;let paths=tracked.into_iter().chain(untracked).collect();
 let label=args["label"].as_str().unwrap_or("manual");if label.len()>4096{return Err("checkpoint label too large".into());}
 let patch=RollbackPatch{ts:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error|error.to_string())?.as_millis()as u64,label:label.into(),paths,porcelain,porcelain_z:Some(porcelain_z),diff,workspace_root:Some(root.to_string_lossy().into_owned())};
 let value=serde_json::to_value(patch).map_err(|error|error.to_string())?;
 crate::workspace_memory::append_rollback_patch(workspace,value.clone())?;
 Ok(value)
}

static ROLLBACKS:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
fn rollback_targets(porcelain:&str)->Result<(Vec<String>,Vec<String>),String>{
 let mut tracked=Vec::new();let mut untracked=Vec::new();
 for line in porcelain.lines(){if line.len()<4{continue;}let code=line.get(..2).ok_or("invalid rollback status")?;let rest=line.get(3..).ok_or("invalid rollback path")?.trim();
  let raw=if code.starts_with('R')||code.starts_with('C'){rest.split_once(" -> ").ok_or("ambiguous rollback rename")?.1}else{rest};
  let path=if raw.starts_with('"'){serde_json::from_str::<String>(raw).map_err(|_|"unsupported quoted rollback path; original preserved")?}else{raw.into()};
  if path.is_empty()||path.replace('\\',"/").split('/').any(|part|part==".."||part==".git"||part==".ridge-rollback-archive")||std::path::Path::new(&path).components().any(|part|!matches!(part,std::path::Component::Normal(_))){return Err("rollback path outside permitted worktree".into());}
  if code=="??"{untracked.push(path);}else{tracked.push(path);}
 }Ok((tracked,untracked))
}
fn confined(root:&Path,path:&Path)->Result<(),String>{
 let mut existing=path;while !existing.exists(){existing=existing.parent().ok_or("rollback path unavailable")?;}
 let resolved=existing.canonicalize().map_err(|error|error.to_string())?;
 if !resolved.starts_with(root)||resolved==root&&path==root{return Err("rollback path outside requested root".into());}Ok(())
}
fn confined_entry(root:&Path,path:&Path)->Result<(),String>{
 if std::fs::symlink_metadata(path).is_ok_and(|metadata|metadata.file_type().is_symlink()) {
  let parent=path.parent().ok_or("rollback link parent missing")?.canonicalize().map_err(|error|error.to_string())?;
  if parent.starts_with(root){return Ok(());}return Err("rollback link outside requested root".into());
 }

 confined(root,path)
}
pub(crate) async fn rollback(state:&crate::server::AppState,args:&serde_json::Value)->Result<serde_json::Value,String>{
 let _guard=ROLLBACKS.lock().await;
 let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or("missing workspaceId")?).map_err(|_|"invalid workspaceId")?;
 let root=std::path::PathBuf::from(args["workspaceRoot"].as_str().ok_or("missing workspaceRoot")?).canonicalize().map_err(|error|error.to_string())?;
 if !state.fs_scope.is_allowed(&root.to_string_lossy()){return Err("rollback root outside sandbox".into());}
 if git(&root,&["rev-parse","--is-inside-work-tree"]).await?.trim()!="true"{return Err("not a git repository".into());}
 let doc=crate::workspace_memory::dispatch("get_workspace_memory",&serde_json::json!({"workspaceId":workspace}),true)?;
 let patch:RollbackPatch=serde_json::from_value(doc["rollbackPatches"].as_array().and_then(|rows|rows.last()).ok_or("no rollbackPatches in workspace memory")?.clone()).map_err(|error|error.to_string())?;
 if patch.workspace_root.as_deref().is_some_and(|expected|std::path::Path::new(expected)!=root){return Err("checkpoint belongs to another worktree; no rollback changes made".into());}
 let (tracked,untracked)=if let Some(status)=patch.porcelain_z.as_deref(){nul_targets(status)?}else{rollback_targets(&patch.porcelain)?};
 for path in tracked.iter().chain(untracked.iter()){validate_relative(path)?;}
 for path in &tracked{confined(&root,&root.join(path))?;}for path in &untracked{confined_entry(&root,&root.join(path))?;}
 let archive=root.join(".ridge-rollback-archive").join(Uuid::new_v4().to_string());confined(&root,&archive)?;
 std::fs::create_dir_all(&archive).map_err(|error|error.to_string())?;confined(&root,&archive)?;
 let before=git(&root,&["diff","--binary","HEAD","--",".",":(exclude).ridge-rollback-archive"]).await?;
 ridge_core::workspace::file::write_atomic(&archive.join("before.patch"),before.as_bytes()).map_err(|error|error.to_string())?;
 let index=git(&root,&["diff","--cached","--binary","HEAD","--",".",":(exclude).ridge-rollback-archive"]).await?;
 let worktree=git(&root,&["diff","--binary","--",".",":(exclude).ridge-rollback-archive"]).await?;
 ridge_core::workspace::file::write_atomic(&archive.join("before-index.patch"),index.as_bytes()).map_err(|error|error.to_string())?;
 ridge_core::workspace::file::write_atomic(&archive.join("before-worktree.patch"),worktree.as_bytes()).map_err(|error|error.to_string())?;

 ridge_core::workspace::file::write_atomic(&archive.join("manifest.json"),&serde_json::to_vec_pretty(&serde_json::json!({"workspaceId":workspace,"checkpoint":patch,"untracked":untracked})).map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
 let mut archived_untracked=0;
 for (index,path) in untracked.iter().enumerate(){let source=root.join(path);match std::fs::symlink_metadata(&source){Ok(_)=>{},Err(error)if error.kind()==std::io::ErrorKind::NotFound=>continue,Err(error)=>return Err(error.to_string())};let destination=archive.join(format!("untracked-{index}"));confined_entry(&root,&source)?;confined(&root,&destination)?;std::fs::rename(&source,&destination).map_err(|error|format!("rollback archive move failed; recovery material retained at {}: {error}",archive.display()))?;archived_untracked+=1;}
 if !tracked.is_empty(){let mut command=vec!["--literal-pathspecs","checkout","HEAD","--"];command.extend(tracked.iter().map(String::as_str));git(&root,&command).await.map_err(|error|format!("rollback checkout failed; recovery material retained at {}: {error}",archive.display()))?;}
 Ok(serde_json::json!({"archivePath":archive,"restoredTracked":tracked.len(),"archivedUntracked":archived_untracked}))
}

fn validate_relative(path:&str)->Result<(),String>{
 if path.is_empty()||path.replace('\\',"/").split('/').any(|part|part==".."||part==".git"||part==".ridge-rollback-archive")||std::path::Path::new(path).components().any(|part|!matches!(part,std::path::Component::Normal(_))){return Err("rollback path outside permitted worktree".into());}Ok(())
}
fn nul_targets(status:&str)->Result<(Vec<String>,Vec<String>),String>{
 if !status.is_empty()&&!status.ends_with('\0'){return Err("incomplete NUL rollback status; original preserved".into());}
 let mut records=status.split('\0');let mut tracked=Vec::new();let mut untracked=Vec::new();
 while let Some(record)=records.next(){if record.is_empty(){continue;}let code=record.get(..2).ok_or("invalid NUL rollback status")?;if record.as_bytes().get(2)!=Some(&b' '){return Err("invalid NUL rollback status".into());}let path=record.get(3..).ok_or("invalid NUL rollback path")?;validate_relative(path)?;
  if code=="??"{untracked.push(path.to_owned());}else{tracked.push(path.to_owned());}
  if code.contains('R')||code.contains('C'){let source=records.next().filter(|source|!source.is_empty()).ok_or("incomplete rollback rename")?;validate_relative(source)?;}
 }Ok((tracked,untracked))
}
