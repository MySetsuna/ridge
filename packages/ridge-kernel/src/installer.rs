//! Installation reserves both Runtime launch slots without stopping user work.
use anyhow::{bail, Context, Result};
use ridge_core::process_identity::{self, ProcessInstance};
use std::{fs::{self, OpenOptions}, io::Write, path::Path, time::Duration};
use crate::registry::{self, KernelBootGuard, KernelInstanceGuard};

fn reject_live_record(path: &Path, json: bool) -> Result<()> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("cannot inspect Runtime ownership; original preserved"),
    };
    let pid = if json {
        serde_json::from_slice::<registry::KernelEndpoint>(&bytes)
            .context("invalid Runtime endpoint; original preserved")?.pid
    } else {
        std::str::from_utf8(&bytes)?.trim().parse::<u32>()
            .context("invalid Runtime PID; original preserved")?
    };
    if pid == 0 { bail!("invalid Runtime PID; original preserved"); }
    match process_identity::instance(pid) {
        Ok(_) => bail!("Runtime PID {pid} remains active; explicitly stop the service before installation"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("cannot establish Runtime exit; original preserved"),
    }
}

fn reserve() -> Result<(KernelBootGuard, KernelInstanceGuard)> {
    let boot = KernelBootGuard::try_acquire()?.context("Runtime launch or installation in progress")?;
    let instance = KernelInstanceGuard::try_acquire()?
        .context("Runtime remains active; explicitly stop the service before installation")?;
    // Older services may publish a registry without taking the current lock.
    reject_live_record(&registry::kernel_json_path(), true)?;
    reject_live_record(&registry::kernel_pid_path(), false)?;
    Ok((boot, instance))
}

pub fn check() -> Result<()> { let _guards = reserve()?; Ok(()) }

fn process_finished(expected: &ProcessInstance) -> bool {
    match process_identity::instance(expected.pid) {
        Ok(current) => current != *expected,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

fn publish(path: &Path, message: &str) -> Result<()> {
    let temporary=path.with_extension(format!("{}.tmp",uuid::Uuid::new_v4()));
    let result=(|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(message.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::hard_link(&temporary,path)?;
        Ok(())
    })();
    let _=fs::remove_file(temporary);
    result
}

pub fn hold(parent_pid: u32, status: &Path, release: &Path) -> Result<()> {
    hold_until(parent_pid, status, release, |_, _| Ok(|| false))
}

pub fn hold_msi(parent_pid: u32, status: &Path, release: &Path) -> Result<()> {
    hold_msi_product(parent_pid, status, release, None)
}

pub fn hold_msi_product(parent_pid: u32, status: &Path, release: &Path, product: Option<&str>) -> Result<()> {
    #[cfg(windows)] {
        let transaction = MsiTransaction::open()?;
        if transaction.finished()? { bail!("no Windows Installer transaction is executing"); }
        // A missing exit callback must not reserve the Runtime for the lifetime
        // of the MSI service. Unknown mutex state retains the reservation.
        hold_until(parent_pid, status, release, |parent, own| {
            let lease = product.map(|product| MsiLease::open(product, parent, own)).transpose()?;
            Ok(move || {
                let released = lease.as_ref().is_some_and(MsiLease::poll);
                released || transaction.finished().unwrap_or(false)
            })
        })
    }
    #[cfg(not(windows))] {
        let _ = (parent_pid, status, release, product);
        bail!("MSI reservations are available only on Windows")
    }
}

fn hold_until<S, F>(parent_pid: u32, status: &Path, release: &Path, start: S) -> Result<()>
where S: FnOnce(&ProcessInstance, &ProcessInstance) -> Result<F>, F: FnMut() -> bool {
    if !status.is_absolute() || !release.is_absolute() || status == release
        || status.parent() != release.parent() {
        bail!("installer handshake paths must be distinct absolute siblings");
    }
    let parent: ProcessInstance = process_identity::instance(parent_pid)?;
    let own = process_identity::instance(std::process::id())?;
    if parent == own || !process_identity::descendant(&parent, &own)? {
        bail!("installer parent is not an OS-attested ancestor");
    }
    let guards = match reserve() {
        Ok(guards) => guards,
        Err(error) => { publish(status, &format!("ERROR: {error:#}"))?; return Err(error); }
    };
    let mut transaction_finished = match start(&parent, &own) {
        Ok(supervisor) => supervisor,
        Err(error) => { publish(status, &format!("ERROR: {error:#}"))?; return Err(error); }
    };
    if release.try_exists()? || status.with_extension("done").try_exists()? {
        bail!("installer release or completion already exists");
    }
    process_identity::validate(&parent)?;
    publish(status, "READY")?;
    while !process_finished(&parent) && !release.try_exists()? && !transaction_finished() {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(transaction_finished);
    drop(guards);
    publish(&status.with_extension("done"), "DONE")?;
    Ok(())
}

#[cfg(windows)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MsiReceipt {
    version: u8,
    product: String,
    parent: ProcessInstance,
    owner: ProcessInstance,
    port: u16,
    nonce: String,
}

#[cfg(windows)]
fn receipt_path() -> std::path::PathBuf { registry::ridge_data_dir().join("installer-msi.json") }

#[cfg(windows)]
fn read_receipt() -> Result<MsiReceipt> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(receipt_path())?.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 { bail!("invalid MSI reservation record; original preserved"); }
    let value: MsiReceipt = serde_json::from_slice(&bytes)?;
    if value.version != 1 || value.port == 0 || value.nonce.len() != 32
        || !value.nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid MSI reservation record; original preserved");
    }
    Ok(value)
}

#[cfg(windows)]
struct MsiLease { listener: std::net::TcpListener, receipt: MsiReceipt, record: Option<std::fs::File> }

#[cfg(windows)]
impl MsiLease {
    fn open(product: &str, parent: &ProcessInstance, owner: &ProcessInstance) -> Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        if receipt_path().try_exists()? {
            let prior = read_receipt()?;
            if !process_finished(&prior.owner) { bail!("MSI reservation ownership is unresolved; original preserved"); }
            fs::rename(receipt_path(), receipt_path().with_extension(format!("stale-{}", uuid::Uuid::new_v4())))?;
        }
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let receipt = MsiReceipt { version: 1, product: uuid::Uuid::parse_str(product)?.to_string(),
            parent: parent.clone(), owner: owner.clone(), port: listener.local_addr()?.port(),
            nonce: uuid::Uuid::new_v4().simple().to_string() };
        publish(&receipt_path(), &serde_json::to_string(&receipt)?)?;
        let record = OpenOptions::new().read(true).share_mode(1).open(receipt_path())?;
        Ok(Self { listener, receipt, record: Some(record) })
    }

    fn poll(&self) -> bool {
        use std::io::Read;
        let Ok((mut stream, remote)) = self.listener.accept() else { return false; };
        let accepted = (|| -> Result<bool> {
            stream.set_read_timeout(Some(Duration::from_millis(250)))?;
            stream.set_write_timeout(Some(Duration::from_millis(250)))?;
            let local = stream.local_addr()?;
            let (std::net::SocketAddr::V4(local), std::net::SocketAddr::V4(remote)) = (local, remote) else { bail!("invalid MSI peer"); };
            let peer = process_identity::connection_owner(remote, local)?;
            if !process_identity::descendant(&self.receipt.parent, &peer)? { bail!("MSI peer is outside the transaction"); }
            let mut request = [0; 33]; stream.read_exact(&mut request)?;
            if &request[1..] != self.receipt.nonce.as_bytes() || !matches!(request[0], b'V' | b'R') { bail!("invalid MSI peer proof"); }
            process_identity::validate(&self.receipt.parent)?;
            stream.write_all(b"1")?;
            Ok(request[0] == b'R')
        })();
        accepted.unwrap_or(false)
    }
}

#[cfg(windows)]
impl Drop for MsiLease {
    fn drop(&mut self) {
        drop(self.record.take());
        if read_receipt().is_ok_and(|current| current.owner == self.receipt.owner && current.nonce == self.receipt.nonce) {
            let _ = fs::remove_file(receipt_path());
        }
    }
}

#[cfg(windows)]
fn contact_msi(parent: &ProcessInstance, product: &str, release: bool) -> Result<MsiReceipt> {
    use std::{io::Read, net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream}};
    let receipt = read_receipt()?;
    if receipt.parent != *parent || receipt.product != product { bail!("MSI reservation belongs to a different transaction"); }
    process_identity::validate(&receipt.owner)?;
    // Initial ancestry was attested before READY and the receipt is held
    // read-only. Its launching CLI has since exited; bind the live holder by
    // exact socket ownership instead of reconstructing a dead ancestor chain.
    let mut stream = TcpStream::connect_timeout(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, receipt.port)), Duration::from_secs(1))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?; stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let (SocketAddr::V4(local), SocketAddr::V4(remote)) = (stream.local_addr()?, stream.peer_addr()?) else { bail!("invalid MSI holder endpoint"); };
    if process_identity::connection_owner(remote, local)? != receipt.owner { bail!("MSI holder process instance changed"); }
    stream.write_all(&[if release { b'R' } else { b'V' }])?;
    stream.write_all(receipt.nonce.as_bytes())?;
    let mut reply = [0]; stream.read_exact(&mut reply)?;
    if reply != *b"1" { bail!("MSI reservation was not acknowledged"); }
    process_identity::validate(parent)?;
    Ok(receipt)
}

#[cfg(windows)]
struct MsiTransaction(*mut std::ffi::c_void);

#[cfg(windows)]
mod msi_os {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
        pub fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        pub fn ReleaseMutex(handle: *mut c_void) -> i32;
        pub fn CloseHandle(handle: *mut c_void) -> i32;
        pub fn GetSystemDirectoryW(buffer: *mut u16, length: u32) -> u32;
    }
}

#[cfg(windows)]
impl MsiTransaction {
    fn open() -> Result<Self> {
        let name: Vec<u16> = "Global\\_MSIExecute\0".encode_utf16().collect();
        let handle = unsafe { msi_os::OpenMutexW(0x0010_0001, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("cannot observe Windows Installer transaction");
        }
        Ok(Self(handle))
    }

    fn finished(&self) -> Result<bool> {
        let state = unsafe { msi_os::WaitForSingleObject(self.0, 0) };
        match state {
            258 => Ok(false), // WAIT_TIMEOUT: execution still owns the mutex.
            0 | 0x80 => {
                if unsafe { msi_os::ReleaseMutex(self.0) } == 0 {
                    return Err(std::io::Error::last_os_error()).context("cannot release observed MSI mutex");
                }
                if state == 0x80 { bail!("MSI transaction ownership was abandoned; completion is unknown"); }
                Ok(true)
            }
            _ => Err(std::io::Error::last_os_error()).context("cannot establish MSI transaction completion"),
        }
    }
}

#[cfg(windows)]
impl Drop for MsiTransaction {
    fn drop(&mut self) { unsafe { msi_os::CloseHandle(self.0); } }
}

/// The MSI Binary custom actions run this same Runtime executable. No shell or
/// desktop process supplies service PIDs, paths, or a second ownership rule.
pub fn msi_action(action: &str, product: &str, user: &str, upgrading: &str) -> Result<()> {
    #[cfg(windows)] {
        let result = msi_action_windows(action, product, user, upgrading);
        if let Err(error) = &result {
            if let Some(cache) = dirs::cache_dir() {
                let directory = cache.join("ridge");
                if fs::create_dir_all(&directory).is_ok() {
                    let message: String = format!("MSI {action}: {error:#}\n").chars().take(4096).collect();
                    let _ = fs::write(directory.join("installer-last-error.txt"), message);
                }
            }
        }
        result
    }
    #[cfg(not(windows))] {
        let _ = (action, product, user, upgrading);
        bail!("MSI actions are available only on Windows")
    }
}

#[cfg(windows)]
fn msi_service(user: &str) -> Result<ProcessInstance> {
    use std::os::windows::ffi::OsStringExt;
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let own = process_identity::instance(std::process::id())?;
    let mut buffer = [0u16; 32768];
    let length = unsafe { msi_os::GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() { bail!("cannot establish MSI system directory"); }
    let expected = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length])).join("msiexec.exe").canonicalize()?;
    let mut system = System::new();
    let mut pid = own.pid;
    let mut service = None;
    for _ in 0..64 {
        let instance = process_identity::instance(pid)?;
        let id = Pid::from_u32(pid);
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[id]), ProcessRefreshKind::new().with_exe(UpdateKind::Always).with_user(UpdateKind::Always));
        let process = system.process(id).context("cannot inspect MSI ancestry")?;
        let executable = process.exe().context("cannot establish MSI ancestor image")?.canonicalize()?;
        let is_msi = executable.to_string_lossy().eq_ignore_ascii_case(&expected.to_string_lossy());
        if service.is_some() && !is_msi { break; }
        let sid = process.user_id().context("cannot establish MSI account")?.to_string();
        if pid == own.pid && (sid != user || sid == "S-1-5-18") {
            bail!("installer account differs from the Runtime owner; original preserved");
        }
        if sid == "S-1-5-18" && is_msi {
            service = Some(instance.clone());
        }
        let next = process.parent().map(|parent| parent.as_u32());
        process_identity::validate(&instance)?;
        let Some(next) = next.filter(|next| *next != pid && *next != 0) else { break; };
        pid = next;
    }
    let service = service.context("installer is not inside an OS-attested MSI execution service")?;
    if !process_identity::descendant(&service, &own)? { bail!("MSI execution service instance changed"); }
    Ok(service)
}

#[cfg(windows)]
fn msi_action_windows(action: &str, product: &str, user: &str, upgrading: &str) -> Result<()> {
    let product = uuid::Uuid::parse_str(product)?.to_string();
    let owner_product = if upgrading.is_empty() { product.clone() } else { uuid::Uuid::parse_str(upgrading)?.to_string() };
    if std::env::var_os("RIDGE_KERNEL_DATA_DIR").is_some() {
        bail!("MSI cannot establish the installed Runtime namespace with a directory override");
    }
    if std::env::var_os("RIDGE_TEST_ALLOW_NON_BREAKAWAY").is_some() {
        bail!("MSI requires production process-tree isolation");
    }
    if !dirs::data_local_dir().is_some_and(|directory| directory.is_absolute()) {
        bail!("cannot establish the Runtime owner's data directory");
    }
    let parent = msi_service(user)?;
    let transaction = MsiTransaction::open()?;
    if transaction.finished()? { bail!("MSI execution transaction has already ended"); }
    match action {
        "verify" => { contact_msi(&parent, &owner_product, false)?; }
        "release" => {
            // The old product's nested uninstall borrows the outer upgrade's
            // reservation; only that outer product may release it after commit.
            if read_receipt()?.product == product { contact_msi(&parent, &product, true)?; }
        }
        "reserve" if !upgrading.is_empty() => { contact_msi(&parent, &owner_product, false)?; }
        "reserve" => {
            let directory = registry::ridge_data_dir().join(format!("installer-msi-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(registry::ridge_data_dir())?;
            fs::create_dir(&directory)?;
            let status = directory.join("runtime.status");
            let release = directory.join("runtime.release");
            let parent_arg = parent.pid.to_string();
            let child = crate::client::spawn_detached_with_pid(&std::env::current_exe()?, &[
                "--installer-msi", "--installer-parent", &parent_arg,
                "--installer-product", &product,
                "--installer-status", status.to_str().context("unsupported installer status path")?,
                "--installer-release", release.to_str().context("unsupported installer release path")?,
            ]).map_err(anyhow::Error::msg)?;
            let child_instance = process_identity::instance(child).ok();
            let ready = (|| -> Result<()> {
                for _ in 0..100 {
                    match fs::read_to_string(&status) {
                        Ok(value) if value == "READY" => { contact_msi(&parent, &product, false)?; return Ok(()); }
                        Ok(value) if !value.is_empty() => bail!("{value}"),
                        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
                        _ => {}
                    }
                    if process_identity::instance(child).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
                        bail!("Runtime reservation helper exited; original installation preserved");
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                bail!("Runtime reservation timed out; original installation preserved")
            })();
            if ready.is_err() {
                let _ = publish(&release, "ABORT");
                for _ in 0..20 {
                    if child_instance.as_ref().is_some_and(process_finished)
                        || process_identity::instance(child).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
                        clean_msi_handshake(&status, &release); break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            ready?;
        }
        _ => bail!("invalid MSI lifecycle action"),
    }
    Ok(())
}

pub fn clean_msi_handshake(status: &Path, release: &Path) {
    let Some(directory) = status.parent().filter(|directory| Some(*directory) == release.parent()) else { return; };
    let owned = directory.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("installer-msi-")).is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
    if !owned || directory.parent() != Some(registry::ridge_data_dir().as_path())
        || status.file_name().is_none_or(|name| name != "runtime.status")
        || release.file_name().is_none_or(|name| name != "runtime.release") { return; }
    let Ok(actual) = directory.canonicalize() else { return; };
    let Ok(root) = registry::ridge_data_dir().canonicalize() else { return; };
    if actual.parent() != Some(root.as_path()) || fs::symlink_metadata(directory).is_ok_and(|meta| meta.file_type().is_symlink()) { return; }
    for file in [status.to_path_buf(), release.to_path_buf(), status.with_extension("done")] { let _ = fs::remove_file(file); }
    let _ = fs::remove_dir(directory);
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{ffi::c_void, sync::mpsc, thread};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> *mut c_void;
    }

    fn ended_transaction(abandon: bool) -> MsiTransaction {
        // Own a unique OS mutex, never the real Windows Installer resource.
        let name = format!("Local\\RidgeMsiTest-{}\0", uuid::Uuid::new_v4());
        let (created, receive) = mpsc::channel();
        let (finish, ending) = mpsc::channel();
        let owner = thread::spawn(move || {
            let wide: Vec<u16> = name.encode_utf16().collect();
            let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };
            assert!(!handle.is_null());
            created.send(handle as usize).unwrap();
            ending.recv().unwrap();
            if !abandon { assert_ne!(unsafe { msi_os::ReleaseMutex(handle) }, 0); }
            // Preserve the handle for the observer; thread exit abandons ownership.
        });
        let transaction = MsiTransaction(receive.recv().unwrap() as *mut c_void);
        assert!(!transaction.finished().unwrap(), "live owner must retain the reservation");
        finish.send(()).unwrap();
        owner.join().unwrap();
        transaction
    }

    #[test]
    fn orderly_mutex_release_proves_transaction_completion() {
        let transaction = ended_transaction(false);
        assert!(transaction.finished().unwrap());
        assert!(transaction.finished().unwrap());
    }

    #[test]
    fn abandoned_mutex_remains_unknown_on_later_polls() {
        let transaction = ended_transaction(true);
        assert!(transaction.finished().is_err());
        assert!(transaction.finished().is_err(), "a later poll must not erase abandoned transaction evidence");
    }
}
