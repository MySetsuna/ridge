//! Tauri-side proxy for PTYs owned by the long-lived ridge-kernel process.
//!
//! The proxy deliberately implements `portable_pty`'s existing master/writer
//! seams. The rest of the desktop terminal path therefore keeps its parser,
//! scrollback, delta and lifecycle guards while the child process and PTY
//! handles live outside the WebView process.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::Arc;

#[cfg(unix)]
use std::os::fd::RawFd;

use anyhow::Error as AnyhowError;
use parking_lot::Mutex;
use portable_pty::{MasterPty, PtySize};
use ridge_kernel::client::{
    attach_domain_pty_output, detach_domain_pty_output, poll_domain_pty_output, resize_domain_pty,
    resync_domain_pty_output, write_domain_pty, KernelPtyOutput,
};
use ridge_kernel::registry::KernelEndpoint;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct KernelPtyRef {
    pub endpoint: KernelEndpoint,
    pub id: Uuid,
    /// Resume cursor used only when this proxy is rebuilt after a desktop
    /// restart. New PTYs use `None` and replay the bounded retained window.
    pub after_seq: Option<u64>,
}

impl KernelPtyRef {
    pub fn destroy(&self) -> Result<(), String> {
        ridge_kernel::client::destroy_domain_pty(&self.endpoint, self.id)
    }

    pub fn clear(&self) -> Result<(), String> {
        ridge_kernel::client::clear_domain_pty(&self.endpoint, self.id)
    }

    pub fn scrollback(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        ridge_kernel::client::scrollback_domain_pty(&self.endpoint, self.id, max_bytes)
    }
}

pub struct KernelPtyMaster {
    reference: KernelPtyRef,
    size: Mutex<PtySize>,
}

impl KernelPtyMaster {
    pub fn new(reference: KernelPtyRef, size: PtySize) -> Self {
        Self {
            reference,
            size: Mutex::new(size),
        }
    }
}

impl MasterPty for KernelPtyMaster {
    fn resize(&self, size: PtySize) -> Result<(), AnyhowError> {
        resize_domain_pty(
            &self.reference.endpoint,
            self.reference.id,
            size.cols,
            size.rows,
        )
        .map_err(pty_error)
        .map(|_| {
            *self.size.lock() = size;
        })
    }

    fn get_size(&self) -> Result<PtySize, AnyhowError> {
        Ok(*self.size.lock())
    }

    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>, AnyhowError> {
        let lease_id = attach_domain_pty_output(
            &self.reference.endpoint,
            self.reference.id,
            self.reference.after_seq,
        )
        .map_err(pty_error)?;
        Ok(Box::new(KernelPtyReader {
            reference: self.reference.clone(),
            lease_id,
            pending: VecDeque::new(),
            detached: false,
        }))
    }

    fn take_writer(&self) -> Result<Box<dyn Write + Send>, AnyhowError> {
        Ok(Box::new(KernelPtyWriter {
            reference: self.reference.clone(),
        }))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<std::os::raw::c_int> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<RawFd> {
        None
    }
}

pub struct KernelPtyWriter {
    reference: KernelPtyRef,
}

impl Write for KernelPtyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_domain_pty(&self.reference.endpoint, self.reference.id, buf)
            .map(|_| buf.len())
            .map_err(io_error)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct KernelPtyReader {
    reference: KernelPtyRef,
    lease_id: Uuid,
    pending: VecDeque<u8>,
    detached: bool,
}

impl Read for KernelPtyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.pending.is_empty() {
                let count = buf.len().min(self.pending.len());
                for slot in &mut buf[..count] {
                    *slot = self.pending.pop_front().expect("pending length checked");
                }
                return Ok(count);
            }
            match poll_domain_pty_output(
                &self.reference.endpoint,
                self.reference.id,
                self.lease_id,
                1000,
                64,
            )
            .map_err(io_error)?
            {
                KernelPtyOutput::Data(data) if !data.is_empty() => {
                    self.pending.extend(data);
                }
                KernelPtyOutput::Data(_) | KernelPtyOutput::Timeout => continue,
                KernelPtyOutput::Lagged => {
                    resync_domain_pty_output(
                        &self.reference.endpoint,
                        self.reference.id,
                        self.lease_id,
                    )
                    .map_err(io_error)?;
                }
            }
        }
    }
}

impl Drop for KernelPtyReader {
    fn drop(&mut self) {
        if !self.detached {
            let _ = detach_domain_pty_output(
                &self.reference.endpoint,
                self.reference.id,
                self.lease_id,
            );
            self.detached = true;
        }
    }
}

fn io_error(error: String) -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, error)
}

fn pty_error(error: String) -> AnyhowError {
    io_error(error).into()
}

pub fn make_master(
    reference: KernelPtyRef,
    cols: u16,
    rows: u16,
) -> Arc<Mutex<Box<dyn MasterPty + Send>>> {
    Arc::new(Mutex::new(Box::new(KernelPtyMaster::new(
        reference,
        PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        },
    ))))
}

pub fn make_writer(reference: KernelPtyRef) -> Arc<Mutex<Box<dyn Write + Send>>> {
    Arc::new(Mutex::new(Box::new(KernelPtyWriter { reference })))
}
