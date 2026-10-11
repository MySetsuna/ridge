//! One detect-or-spawn path for desktop, CLI and detached Host shells.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::client::{running_endpoint, spawn_detached, wait_for_running, KERNEL_HOST_ARG};
use crate::registry::{read_endpoint, KernelBootGuard, KernelEndpoint};

static BOOT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const BOOT_TIMEOUT: Duration = Duration::from_secs(8);

fn select_host_binary(
    explicit: Option<PathBuf>,
    current: Result<PathBuf, std::io::Error>,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let current = current.map_err(|error| format!("locate kernel host: {error}"))?;
    let sibling = current.with_file_name(if cfg!(windows) {
        "ridge-kernel.exe"
    } else {
        "ridge-kernel"
    });
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Ok(current)
    }
}

pub fn ensure_kernel_running() -> Result<KernelEndpoint, String> {
    let _local_guard = BOOT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "kernel boot lock poisoned".to_string())?;
    if let Some(endpoint) = running_endpoint() {
        return Ok(endpoint);
    }

    let deadline = Instant::now() + BOOT_TIMEOUT;
    let _boot_guard = loop {
        if let Some(endpoint) = running_endpoint() {
            return Ok(endpoint);
        }
        match KernelBootGuard::try_acquire() {
            Ok(Some(guard)) => break guard,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(80)),
            Ok(None) => {
                return Err("another kernel is booting but did not become healthy in time".into())
            }
            Err(error) => return Err(format!("acquire kernel boot slot: {error}")),
        }
    };
    // Recheck after reserving the cross-process boot slot. Never clear a live
    // instance merely because its transport is temporarily unhealthy.
    if let Some(endpoint) = running_endpoint() {
        return Ok(endpoint);
    }
    if let Some(endpoint) = read_endpoint() {
        let cleared = crate::registry::try_clear_registry_if_instance_free(endpoint.pid)
            .map_err(|error| format!("probe kernel instance lock: {error}"))?;
        if !cleared {
            return Err(format!("live kernel PID {} is unhealthy or protocol-incompatible; refusing a second instance", endpoint.pid));
        }
    }
    let binary = select_host_binary(
        std::env::var_os("RIDGE_KERNEL_HOST_BINARY")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
        std::env::current_exe(),
    )?;
    tracing::info!(path = %binary.display(), "spawning detached kernel host");
    spawn_detached(&binary, &[KERNEL_HOST_ARG])?;
    // Hold the boot slot through readiness; dropping it immediately after
    // spawn lets a competing shell launch a second, not-yet-published kernel.
    wait_for_running(BOOT_TIMEOUT).ok_or_else(|| "kernel did not become healthy in time".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_binary_is_not_replaced_by_a_sibling() {
        let selected = select_host_binary(
            Some(PathBuf::from("configured-kernel")),
            Err(std::io::Error::other("current executable unavailable")),
        )
        .unwrap();
        assert_eq!(selected, PathBuf::from("configured-kernel"));
    }

    #[test]
    fn packaged_kernel_precedes_embedded_shell() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-binary-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let current = root.join("desktop-shell");
        assert_eq!(
            select_host_binary(None, Ok(current.clone())).unwrap(),
            current
        );
        let sibling = current.with_file_name(if cfg!(windows) {
            "ridge-kernel.exe"
        } else {
            "ridge-kernel"
        });
        std::fs::write(&sibling, []).unwrap();
        assert_eq!(select_host_binary(None, Ok(current)).unwrap(), sibling);
    }
}
