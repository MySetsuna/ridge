//! Native presentation adapter for the shared terminal producer.

pub struct PaneParser(ridge_term::producer::TerminalProducer);
impl PaneParser {
    pub fn new(rows: u16, cols: u16, history: usize) -> Self {
        Self(ridge_term::producer::TerminalProducer::with_epoch(
            rows,
            cols,
            history,
            uuid::Uuid::new_v4().to_string(),
        ))
    }
}
impl std::ops::Deref for PaneParser {
    type Target = ridge_term::producer::TerminalProducer;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for PaneParser {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ridge_term::term::Terminal;
    #[test]
    #[ignore = "spawns a bounded real PowerShell ConPTY for multiline rendering acceptance"]
    fn powershell_multiline_input_preserves_display_order() {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        use std::io::{Read, Write};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        struct ChildGuard(Box<dyn portable_pty::Child + Send + Sync>);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
            }
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 12,
                cols: 60,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new("pwsh.exe");
        command.args(["-NoLogo", "-NoProfile"]);
        command.env("TERM", "xterm-256color");
        let _child = ChildGuard(pair.slave.spawn_command(command).unwrap());
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0; 8192];
            while let Ok(size) = reader.read(&mut buffer) {
                if size == 0 || send.send(buffer[..size].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut producer = PaneParser::new(12, 60, 1000);
        let mut mirror = Terminal::new(12, 60, 1000);
        let mut raw = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            let bytes = receive
                .recv_timeout(Duration::from_millis(100))
                .unwrap_or_default();
            raw.extend_from_slice(&bytes);
            mirror.apply_frame(&producer.feed_and_diff(&bytes)).unwrap();
            let reply = producer.take_pending_response();
            if !reply.is_empty() {
                writer.write_all(&reply).unwrap();
                writer.flush().unwrap();
            }
            if String::from_utf8_lossy(&raw).contains("> ") {
                break;
            }
        }
        assert!(
            String::from_utf8_lossy(&raw).contains("> "),
            "PowerShell prompt ready"
        );
        let mut input = "$ridge_lines = @(\n".to_owned();
        for line in 1..=24 {
            input.push_str(&format!("    'RIDGE_LINE_{line:02}_C:\\long-folder\\long-folder\\long-folder\\long-folder'\n"));
        }
        input.push(')');
        let paste = ridge_term::input::wrap_paste(&input, producer.modes().bracketed_paste);
        println!(
            "Real paste: bracketed={}, bytes={}",
            producer.modes().bracketed_paste,
            paste.len()
        );
        writer.write_all(&paste).unwrap();
        writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut quiet_since = Instant::now();
        while Instant::now() < deadline {
            match receive.recv_timeout(Duration::from_millis(100)) {
                Ok(bytes) => {
                    raw.extend_from_slice(&bytes);
                    mirror.apply_frame(&producer.feed_and_diff(&bytes)).unwrap();
                    let reply = producer.take_pending_response();
                    if !reply.is_empty() {
                        writer.write_all(&reply).unwrap();
                        writer.flush().unwrap();
                    }
                    quiet_since = Instant::now();
                }
                Err(_) if quiet_since.elapsed() > Duration::from_secs(1) => break,
                Err(_) => {}
            }
        }
        let dump = |terminal: &Terminal| {
            let mut rows = Vec::new();
            for index in 0..terminal.scrollback_len() {
                let row = terminal.grid().scrollback.get(index).unwrap();
                rows.push(
                    row.cells
                        .iter()
                        .filter(|cell| cell.width != 0)
                        .map(|cell| cell.ch)
                        .collect::<String>(),
                );
            }
            rows.extend(terminal.dump_visible_text());
            rows.join("\n")
        };
        let native = dump(producer.terminal());
        let artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("artifacts/terminal-five-fixes");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("powershell-raw.bin"), &raw).unwrap();
        std::fs::write(artifacts.join("powershell-native.txt"), &native).unwrap();
        println!(
            "PowerShell capture: {} bytes, {} ESC controls",
            raw.len(),
            raw.iter().filter(|byte| **byte == 27).count()
        );
        assert_eq!(
            dump(&mirror),
            native,
            "native delta mirror retains the real PTY rows"
        );
        let mut direct = Terminal::new(12, 60, 1000);
        direct.feed(&raw);
        assert_eq!(dump(&direct), native, "raw and native parsing agree");
        let mut previous = 0;
        for line in 1..=24 {
            let marker = format!("RIDGE_LINE_{line:02}");
            let position = native
                .find(&marker)
                .unwrap_or_else(|| panic!("missing {marker}: {native}"));
            assert!(
                position >= previous,
                "multiline display reversed at {marker}: {native}"
            );
            previous = position;
        }
    }
}
