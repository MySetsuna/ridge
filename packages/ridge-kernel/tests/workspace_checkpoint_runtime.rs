//! Actual Git reads commit one durable checkpoint; this test performs no rollback.
#[path="common/runtime_fixture.rs"]mod fixture;
use fixture::{start,attach,rpc};
use serde_json::json;
use uuid::Uuid;
fn git(root:&std::path::Path,args:&[&str]){let out=std::process::Command::new("git").arg("-C").arg(root).arg("-c").arg("core.hooksPath=disabled-owned-hooks").arg("-c").arg("commit.gpgSign=false").arg("-c").arg("core.fsmonitor=false").args(args).output().unwrap();assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));if args==["init"]{git(root,&["config","core.fsmonitor","false"]);}}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn checkpoint_commits_actual_git_state_preserves_memory_and_refuses_damage() {
 let root=std::env::temp_dir().join(format!("ridge-checkpoint-owner-{}",Uuid::new_v4()));let repo=root.join("repo");std::fs::create_dir_all(&repo).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);
 std::fs::write(repo.join("tracked.txt"),"base\n").unwrap();git(&repo,&["add","tracked.txt"]);git(&repo,&["commit","-m","owned baseline"]);std::fs::write(repo.join("tracked.txt"),"changed\n").unwrap();std::fs::write(repo.join("untracked.txt"),"retained file").unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();
 assert!(rpc(&mut socket,1,"set_workspace_memory",json!({"workspaceId":wid,"goal":"preserve"})).await["error"].is_null());
 let result=rpc(&mut socket,2,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo,"label":"owned"})).await;assert!(result["error"].is_null(),"{result}");assert!(result["result"]["diff"].as_str().unwrap().contains("+changed"));assert!(result["result"]["paths"].as_array().unwrap().contains(&json!("untracked.txt")));
 let memory=rpc(&mut socket,3,"get_workspace_memory",json!({"workspaceId":wid})).await;assert_eq!(memory["result"]["goal"],"preserve");assert_eq!(memory["result"]["rollbackPatches"][0],result["result"]);assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"changed\n");assert!(repo.join("untracked.txt").exists());

 let outside=std::env::temp_dir().join(format!("ridge-checkpoint-outside-{}",Uuid::new_v4()));std::fs::create_dir_all(&outside).unwrap();std::fs::write(outside.join("marker"),"untouched").unwrap();
 let denied=rpc(&mut socket,5,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":outside})).await;assert!(denied["error"].is_object(),"{denied}");assert_eq!(std::fs::read_to_string(outside.join("marker")).unwrap(),"untouched");

 git(&repo,&["add","tracked.txt"]);std::fs::write(repo.join("tracked.txt"),"worktree-version\n").unwrap();
 let rolled=rpc(&mut socket,6,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(rolled["error"].is_null(),"{rolled}");
 assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"base\n");assert!(!repo.join("untracked.txt").exists());
 let archive=std::path::PathBuf::from(rolled["result"]["archivePath"].as_str().unwrap());assert!(archive.starts_with(repo.canonicalize().unwrap()));assert_eq!(std::fs::read_to_string(archive.join("untracked-0")).unwrap(),"retained file");assert!(std::fs::read_to_string(archive.join("before.patch")).unwrap().contains("+worktree-version"));
 assert!(std::fs::read_to_string(archive.join("before-index.patch")).unwrap().contains("+changed"));assert!(std::fs::read_to_string(archive.join("before-worktree.patch")).unwrap().contains("+worktree-version"));
 let current=rpc(&mut socket,7,"read_workspace_memory_document",json!({"workspaceId":wid})).await["result"].clone();let mut forged=current.clone();forged["rollbackPatches"][0]["porcelain"]=json!(format!("?? ../../{}/marker",outside.file_name().unwrap().to_string_lossy()));forged["rollbackPatches"][0]["porcelainZ"]=json!(format!("?? ../../{}/marker\0",outside.file_name().unwrap().to_string_lossy()));
 assert!(rpc(&mut socket,8,"commit_workspace_memory_document",json!({"workspaceId":wid,"expected":current,"value":forged})).await["error"].is_null());
 let refused=rpc(&mut socket,9,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(refused["error"].is_object(),"{refused}");assert_eq!(std::fs::read_to_string(outside.join("marker")).unwrap(),"untouched");assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"base\n");
 let file=root.join("workspace-memory").join(format!("{wid}.json"));std::fs::write(&file,b"{broken").unwrap();let failed=rpc(&mut socket,4,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(failed["error"].is_object());assert_eq!(std::fs::read(&file).unwrap(),b"{broken");assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"base\n");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn rollback_uses_literal_paths_without_restoring_matching_unlisted_files() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-literal-{}",Uuid::new_v4()));let repo=root.join("repo");std::fs::create_dir_all(&repo).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);
 for name in ["[ab].txt","a.txt","b.txt"]{std::fs::write(repo.join(name),"base\n").unwrap();}git(&repo,&["add","."]);git(&repo,&["commit","-m","owned baseline"]);
 std::fs::write(repo.join("[ab].txt"),"literal change\n").unwrap();let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();
 let checkpoint=rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(checkpoint["error"].is_null(),"{checkpoint}");
 std::fs::write(repo.join("a.txt"),"unlisted a\n").unwrap();std::fs::write(repo.join("b.txt"),"unlisted b\n").unwrap();
 let result=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(result["error"].is_null(),"{result}");
 assert_eq!(std::fs::read_to_string(repo.join("[ab].txt")).unwrap(),"base\n");assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(),"unlisted a\n");assert_eq!(std::fs::read_to_string(repo.join("b.txt")).unwrap(),"unlisted b\n");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn nul_checkpoint_restores_unicode_and_archives_names_exactly() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-nul-{}",Uuid::new_v4()));let repo=root.join("repo");std::fs::create_dir_all(&repo).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);
 let tracked="中文文件.txt";std::fs::write(repo.join(tracked),"base\n").unwrap();git(&repo,&["add","."]);git(&repo,&["commit","-m","owned baseline"]);std::fs::write(repo.join(tracked),"changed\n").unwrap();
 let untracked=if cfg!(windows){"literal - arrow.txt"}else{"literal -> arrow\nname.txt"};std::fs::write(repo.join(untracked),"retained exact filename").unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();let snapshot=rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(snapshot["error"].is_null(),"{snapshot}");assert!(snapshot["result"]["paths"].as_array().unwrap().contains(&json!(tracked)));
 let result=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(result["error"].is_null(),"{result}");assert_eq!(std::fs::read_to_string(repo.join(tracked)).unwrap(),"base\n");assert!(!repo.join(untracked).exists());let archive=std::path::PathBuf::from(result["result"]["archivePath"].as_str().unwrap());assert_eq!(std::fs::read_to_string(archive.join("untracked-0")).unwrap(),"retained exact filename");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn rollback_checkpoint_cannot_be_applied_to_a_different_allowed_repo() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-origin-{}",Uuid::new_v4()));let a=root.join("a");let b=root.join("b");
 for repo in [&a,&b] {std::fs::create_dir_all(repo).unwrap();git(repo,&["init"]);git(repo,&["config","core.autocrlf","false"]);git(repo,&["config","user.email","owned@test.invalid"]);git(repo,&["config","user.name","Owned fixture"]);std::fs::write(repo.join("same-name.txt"),"base\n").unwrap();git(repo,&["add","."]);git(repo,&["commit","-m","owned baseline"]);}
 std::fs::write(a.join("same-name.txt"),"repo a changed\n").unwrap();std::fs::write(b.join("same-name.txt"),"repo b must remain\n").unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();let snapshot=rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":a})).await;assert!(snapshot["error"].is_null(),"{snapshot}");assert_eq!(snapshot["result"]["workspaceRoot"],json!(a.canonicalize().unwrap()));
 let refused=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":b})).await;assert!(refused["error"].is_object(),"{refused}");assert_eq!(std::fs::read_to_string(b.join("same-name.txt")).unwrap(),"repo b must remain\n");assert!(!b.join(".ridge-rollback-archive").exists());
 let accepted=rpc(&mut socket,3,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":a})).await;assert!(accepted["error"].is_null(),"{accepted}");assert_eq!(std::fs::read_to_string(a.join("same-name.txt")).unwrap(),"base\n");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn recovery_archive_failure_refuses_rollback_before_worktree_mutation() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-archive-failure-{}",Uuid::new_v4()));let repo=root.join("repo");std::fs::create_dir_all(&repo).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);
 std::fs::write(repo.join("tracked.txt"),"base\n").unwrap();git(&repo,&["add","."]);git(&repo,&["commit","-m","owned baseline"]);std::fs::write(repo.join("tracked.txt"),"changed must remain\n").unwrap();std::fs::write(repo.join("untracked.txt"),"untracked must remain").unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();assert!(rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await["error"].is_null());
 let blocked=repo.join(".ridge-rollback-archive");std::fs::write(&blocked,"owned archive blocker").unwrap();
 let refused=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(refused["error"].is_object(),"{refused}");assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"changed must remain\n");assert_eq!(std::fs::read_to_string(repo.join("untracked.txt")).unwrap(),"untracked must remain");assert_eq!(std::fs::read_to_string(&blocked).unwrap(),"owned archive blocker");
 std::fs::rename(&blocked,repo.join("owned-archive-blocker.saved")).unwrap();let accepted=rpc(&mut socket,3,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(accepted["error"].is_null(),"{accepted}");assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"base\n");let archive=std::path::PathBuf::from(accepted["result"]["archivePath"].as_str().unwrap());assert_eq!(std::fs::read_to_string(archive.join("untracked-0")).unwrap(),"untracked must remain");assert!(repo.join("owned-archive-blocker.saved").exists());
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn rollback_archives_broken_links_and_counts_only_existing_entries() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-links-{}",Uuid::new_v4()));let repo=root.join("repo");std::fs::create_dir_all(&repo).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);std::fs::write(repo.join("tracked.txt"),"base\n").unwrap();git(&repo,&["add","."]);git(&repo,&["commit","-m","owned baseline"]);
 let target=std::path::PathBuf::from("missing-target");
 #[cfg(windows)]std::os::windows::fs::symlink_file(&target,repo.join("dangling.txt")).unwrap();
 #[cfg(not(windows))]std::os::unix::fs::symlink(&target,repo.join("dangling.txt")).unwrap();
 std::fs::write(repo.join("gone.txt"),"temporary entry").unwrap();let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();let snapshot=rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(snapshot["error"].is_null(),"{snapshot}");std::fs::rename(repo.join("gone.txt"),root.join("gone-retained.txt")).unwrap();
 let result=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(result["error"].is_null(),"{result}");assert_eq!(result["result"]["archivedUntracked"],1);assert!(std::fs::symlink_metadata(repo.join("dangling.txt")).is_err());
 let archive=std::path::PathBuf::from(result["result"]["archivePath"].as_str().unwrap());assert!(std::fs::read_dir(&archive).unwrap().flatten().any(|entry|std::fs::read_link(entry.path()).ok().as_ref()==Some(&target)));assert_eq!(std::fs::read_to_string(root.join("gone-retained.txt")).unwrap(),"temporary entry");
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn rollback_archive_directory_cannot_redirect_recovery_outside_worktree() {
 let root=std::env::temp_dir().join(format!("ridge-rollback-archive-link-{}",Uuid::new_v4()));let repo=root.join("repo");let outside=root.join("outside");std::fs::create_dir_all(&repo).unwrap();std::fs::create_dir_all(&outside).unwrap();git(&repo,&["init"]);git(&repo,&["config","core.autocrlf","false"]);git(&repo,&["config","user.email","owned@test.invalid"]);git(&repo,&["config","user.name","Owned fixture"]);std::fs::write(repo.join("tracked.txt"),"base\n").unwrap();git(&repo,&["add","."]);git(&repo,&["commit","-m","owned baseline"]);std::fs::write(repo.join("tracked.txt"),"preserve dirty\n").unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;let wid=Uuid::new_v4();assert!(rpc(&mut socket,1,"checkpoint_workspace_rollback",json!({"workspaceId":wid,"workspaceRoot":repo})).await["error"].is_null());
 #[cfg(windows)]std::os::windows::fs::symlink_dir(&outside,repo.join(".ridge-rollback-archive")).unwrap();
 #[cfg(not(windows))]std::os::unix::fs::symlink(&outside,repo.join(".ridge-rollback-archive")).unwrap();
 let refused=rpc(&mut socket,2,"rollback_workspace",json!({"workspaceId":wid,"workspaceRoot":repo})).await;assert!(refused["error"].is_object(),"{refused}");assert_eq!(std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),"preserve dirty\n");assert_eq!(std::fs::read_dir(outside).unwrap().count(),0);
}
