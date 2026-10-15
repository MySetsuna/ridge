mod common { pub mod runtime_fixture; }
use common::runtime_fixture as runtime;
use std::{fs, path::{Path, PathBuf}, process::{Child, Command, Stdio}, time::{Duration, Instant}};

struct Helper(Child);
impl Drop for Helper { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
fn command(root: &Path) -> Command {
    let mut command=Command::new(env!("CARGO_BIN_EXE_ridge-kernel"));
    command.env("RIDGE_KERNEL_DATA_DIR",root).stdout(Stdio::null()).stderr(Stdio::null());
    command
}
fn root() -> PathBuf {
    let root=std::env::temp_dir().join(format!("ridge-installer-{}",uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap(); root
}
fn hold(root: &Path, name: &str, parent: u32) -> Helper {
    Helper(command(root).args(["--installer-parent",&parent.to_string(),"--installer-status"])
        .arg(root.join(format!("{name}.status"))).arg("--installer-release")
        .arg(root.join(format!("{name}.release"))).spawn().unwrap())
}
fn until(mut check: impl FnMut() -> bool) {
    let deadline=Instant::now()+Duration::from_secs(12);
    while !check() { assert!(Instant::now()<deadline,"installer handshake deadline"); std::thread::sleep(Duration::from_millis(20)); }
}

#[tokio::test]
async fn installation_holds_both_locks_and_releases_without_touching_originals() {
    let root=root(); let original=b"original user data";
    fs::write(root.join("preserve.bin"),original).unwrap();
    let mut guard=hold(&root,"guard",std::process::id());
    until(||fs::read_to_string(root.join("guard.status")).is_ok_and(|s|s=="READY"));
    assert!(!command(&root).arg("--installer-check").status().unwrap().success());
    let mut contender=Helper(command(&root).spawn().unwrap());
    std::thread::sleep(Duration::from_millis(300));
    assert!(!root.join("kernel.json").exists());
    assert!(!root.join("projects.sqlite3").exists());
    contender.0.kill().unwrap(); contender.0.wait().unwrap();
    fs::write(root.join("guard.release"),b"RELEASE").unwrap();
    until(||guard.0.try_wait().unwrap().is_some());
    assert!(guard.0.wait().unwrap().success());
    assert_eq!(fs::read_to_string(root.join("guard.done")).unwrap(),"DONE");
    assert_eq!(fs::read(root.join("preserve.bin")).unwrap(),original);
    assert!(command(&root).arg("--installer-check").status().unwrap().success());
    let owned=runtime::start(&root).await;
    let registry=fs::read(root.join("kernel.json")).unwrap();
    let mut rejected=hold(&root,"busy",std::process::id());
    until(||rejected.0.try_wait().unwrap().is_some());
    assert!(!rejected.0.wait().unwrap().success());
    assert!(fs::read_to_string(root.join("busy.status")).unwrap().starts_with("ERROR:"));
    assert_eq!(fs::read(root.join("kernel.json")).unwrap(),registry);
    assert!(ridge_kernel::client::health_ok(&owned.endpoint));
    drop(owned);
}

#[test]
fn invalid_or_legacy_live_ownership_and_unrelated_parent_fail_closed() {
    let root=root();
    let original=b"{interrupted endpoint";
    fs::write(root.join("kernel.json"),original).unwrap();
    assert!(!command(&root).arg("--installer-check").status().unwrap().success());
    assert_eq!(fs::read(root.join("kernel.json")).unwrap(),original);
    let endpoint=ridge_kernel::registry::KernelEndpoint{pid:std::process::id(),port:9,token:"owned".into(),started_at_unix:0};
    fs::write(root.join("kernel.json"),serde_json::to_vec(&endpoint).unwrap()).unwrap();
    assert!(!command(&root).arg("--installer-check").status().unwrap().success());
    let mut own_parent=hold(&root,"invalid",0);
    until(||own_parent.0.try_wait().unwrap().is_some());
    assert!(!own_parent.0.wait().unwrap().success());
    assert!(!root.join("invalid.status").exists());
}

#[test]
fn installer_parent_exit_releases_reservation() {
    let root=root();
    let mut parent=Command::new(std::env::current_exe().unwrap())
        .args(["--ignored","--exact","installer_parent_fixture"])
        .env("RIDGE_INSTALLER_PARENT_FIXTURE",&root)
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    until(||parent.try_wait().unwrap().is_some());
    assert!(parent.wait().unwrap().success());
    until(||root.join("orphan.done").exists());
    assert!(command(&root).arg("--installer-check").status().unwrap().success());
    assert!(!root.join("kernel.json").exists());
}

#[test]
#[ignore = "owned subprocess entry; exercised by installer_parent_exit_releases_reservation"]
fn installer_parent_fixture() {
    let root=PathBuf::from(std::env::var_os("RIDGE_INSTALLER_PARENT_FIXTURE").unwrap());
    let _guard=hold(&root,"orphan",std::process::id());
    until(||fs::read_to_string(root.join("orphan.status")).is_ok_and(|s|s=="READY"));
    // Simulate installer termination without a release handshake or Rust destructors.
    std::process::exit(0);
}
