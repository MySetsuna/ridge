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
}
