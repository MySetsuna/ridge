use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[tokio::test]
async fn probe_eof_propagation() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().expect("spawn");
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Hold stdin open so the child doesn't exit on its own.
    let _stdin = child.stdin.take();

    eprintln!("[probe] killing child");
    child.kill().await.unwrap();
    drop(child);
    eprintln!("[probe] child dropped, attempting read");

    let r = tokio::time::timeout(Duration::from_secs(3), reader.fill_buf()).await;
    eprintln!("[probe] result: {r:?}");
    assert!(matches!(r, Ok(Ok(&[]))), "expected Ok(Ok(empty)) = EOF, got {r:?}");
}
