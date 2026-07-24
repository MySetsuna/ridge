//! Windows Job Object helpers for G1 stage 3 (V-G1-JOB).
//! Pre-create a job and assign the PTY child so freeze can target the whole tree later.
//! Non-Windows: stubs that no-op.

#[cfg(windows)]
use std::ptr;

/// Opaque job handle wrapper (Windows only meaningful).
pub struct JobHandle {
    #[cfg(windows)]
    raw: isize,
}

// SAFETY: Job handle is process-local; we only use it from the creating process.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            if self.raw != 0 {
                CloseHandle(self.raw);
            }
        }
    }
}

/// Create a job object suitable for holding a PTY child process tree.
pub fn create_job() -> Result<JobHandle, String> {
    #[cfg(windows)]
    {
        unsafe {
            let h = CreateJobObjectW(ptr::null_mut(), ptr::null());
            if h == 0 {
                return Err("CreateJobObjectW failed".into());
            }
            Ok(JobHandle { raw: h })
        }
    }
    #[cfg(not(windows))]
    {
        Ok(JobHandle {})
    }
}

/// Assign process `pid` to `job`. Windows only; no-op success elsewhere.
pub fn assign_pid(job: &JobHandle, pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("invalid pid 0".into());
    }
    #[cfg(windows)]
    {
        unsafe {
            let proc = OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_SUSPEND_RESUME,
                0,
                pid,
            );
            if proc == 0 {
                return Err(format!("OpenProcess pid={pid} failed for job assign"));
            }
            let ok = AssignProcessToJobObject(job.raw, proc);
            CloseHandle(proc);
            if ok == 0 {
                return Err(format!("AssignProcessToJobObject pid={pid} failed"));
            }
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (job, pid);
        Ok(())
    }
}

/// Freeze primary shell pid (job membership retained for tree-wide freeze later).
pub fn freeze_job_primary(job: &JobHandle, primary_pid: u32) -> Result<(), String> {
    let _ = job;
    super::os_freeze::freeze_pid(primary_pid)
}

pub fn thaw_job_primary(job: &JobHandle, primary_pid: u32) -> Result<(), String> {
    let _ = job;
    super::os_freeze::thaw_pid(primary_pid)
}

/// Product freeze entry used by [`super::suspend::suspend_with_os`].
///
/// - `Some(job)` → [`freeze_job_primary`] (spawn-time Job Object membership).
/// - `None` → **direct** [`super::os_freeze::freeze_pid`] — never `create_job()`.
///   Creating a throwaway job on this path was a regression: CreateJobObject failure
///   blocked freezes that OS APIs would accept, and the matching thaw path could
///   fail to unfreeze if create_job failed on resume.
pub fn try_freeze_primary(job: Option<&JobHandle>, pid: Option<u32>) -> Result<Option<u32>, String> {
    let Some(pid) = pid else {
        return Ok(None);
    };
    match job {
        Some(j) => freeze_job_primary(j, pid)?,
        None => super::os_freeze::freeze_pid(pid)?,
    }
    Ok(Some(pid))
}

/// Product thaw entry paired with [`try_freeze_primary`].
/// `None` job → direct [`super::os_freeze::thaw_pid`] (no create_job).
pub fn try_thaw_primary(job: Option<&JobHandle>, pid: Option<u32>) -> Result<(), String> {
    let Some(pid) = pid else {
        return Ok(());
    };
    match job {
        Some(j) => thaw_job_primary(j, pid),
        None => super::os_freeze::thaw_pid(pid),
    }
}

#[cfg(windows)]
const PROCESS_SET_QUOTA: u32 = 0x0100;
#[cfg(windows)]
const PROCESS_TERMINATE: u32 = 0x0001;
#[cfg(windows)]
const PROCESS_SUSPEND_RESUME: u32 = 0x0800;

// Match os_freeze.rs HANDLE = isize to avoid clashing_extern_declarations.
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(attrs: *mut core::ffi::c_void, name: *const u16) -> isize;
    fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    fn CloseHandle(h: isize) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_job_succeeds() {
        let j = create_job().expect("create job");
        // assign current process may fail (already in a job on some CI) — must not panic
        let pid = std::process::id();
        let _ = assign_pid(&j, pid);
    }

    #[test]
    fn assign_pid_zero_err() {
        let j = create_job().unwrap();
        assert!(assign_pid(&j, 0).is_err());
    }

    #[test]
    fn freeze_job_primary_rejects_pid_zero() {
        let j = create_job().unwrap();
        assert!(freeze_job_primary(&j, 0).is_err());
        assert!(thaw_job_primary(&j, 0).is_err());
    }

    #[test]
    fn try_freeze_primary_none_ok_and_zero_err() {
        assert_eq!(try_freeze_primary(None, None).unwrap(), None);
        assert!(try_freeze_primary(None, Some(0)).is_err());
        // unlikely pid: must not panic (OS may Err)
        let _ = try_freeze_primary(None, Some(1));
    }

    #[test]
    fn try_freeze_primary_with_job_handle() {
        let j = create_job().unwrap();
        assert!(try_freeze_primary(Some(&j), Some(0)).is_err());
        let _ = try_freeze_primary(Some(&j), Some(1));
    }

    /// None path must use os_freeze only — same error as freeze_pid, never CreateJobObject.
    #[test]
    fn try_freeze_thaw_none_matches_os_freeze_no_create_job() {
        let freeze_err = super::super::os_freeze::freeze_pid(0).unwrap_err();
        let via = try_freeze_primary(None, Some(0)).unwrap_err();
        assert_eq!(via, freeze_err, "None freeze must be pure os_freeze");

        let thaw_err = super::super::os_freeze::thaw_pid(0).unwrap_err();
        let via_t = try_thaw_primary(None, Some(0)).unwrap_err();
        assert_eq!(via_t, thaw_err, "None thaw must be pure os_freeze");

        // thaw must succeed path-wise for None without needing a job object
        assert!(try_thaw_primary(None, None).is_ok());
    }
}
