//! CLI shell projection of the kernel PTY registry.
//!
//! PTY spawn/write/resize/destroy semantics live in `ridge-kernel`; rdg owns
//! only terminal presentation and its lossless output receiver.

use std::sync::Arc;

use anyhow::Result;
use ridge_kernel::pty::PtyRegistry;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct PtyBridge {
    registry: Arc<PtyRegistry>,
    id: Uuid,
}

impl PtyBridge {
    pub fn spawn(
        shell: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let registry = Arc::new(PtyRegistry::default());
        let (id, output) = registry.spawn_with_output(shell, cwd)?;
        Ok((Self { registry, id }, output))
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        self.registry.write(self.id, data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.registry.resize(self.id, cols, rows)
    }
}

impl Drop for PtyBridge {
    fn drop(&mut self) {
        let _ = self.registry.destroy(self.id);
    }
}
