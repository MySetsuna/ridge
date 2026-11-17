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
    hold_until(parent_pid, status, release, || false)
}

pub fn hold_msi(parent_pid: u32, status: &Path, release: &Path) -> Result<()> {
    #[cfg(windows)] {
        let transaction = MsiTransaction::open()?;
        if transaction.finished()? { bail!("no Windows Installer transaction is executing"); }
        // A missing exit callback must not reserve the Runtime for the lifetime
        // of the MSI service. Unknown mutex state retains the reservation.
        hold_until(parent_pid, status, release, || transaction.finished().unwrap_or(false))
    }
    #[cfg(not(windows))] {
        let _ = (parent_pid, status, release);
        bail!("MSI reservations are available only on Windows")
    }
}

fn hold_until(parent_pid: u32, status: &Path, release: &Path, transaction_finished: impl Fn() -> bool) -> Result<()> {
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
    if release.try_exists()? || status.with_extension("done").try_exists()? {
        bail!("installer release or completion already exists");
    }
    process_identity::validate(&parent)?;
    publish(status, "READY")?;
    while process_identity::validate(&parent).is_ok() && !release.try_exists()? && !transaction_finished() {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(guards);
    publish(&status.with_extension("done"), "DONE")?;
    Ok(())
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
