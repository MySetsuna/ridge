//! Host admission shared by terminal transports. Commit only after the sink accepts bytes.
use std::collections::HashMap;
use crate::terminal_protocol::{validate_input, PaneRef, TerminalInput, TerminalInputAck};

#[derive(Default)]
pub struct TerminalInputLedger {
    sources: HashMap<String, (u64, Vec<u8>)>,
}

impl TerminalInputLedger {
    pub fn apply(
        &mut self, input: &TerminalInput, pane: &PaneRef, epoch: &str,
        activation: Option<u64>, write: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<TerminalInputAck, String> {
        validate_input(input).map_err(|error| error.to_string())?;
        if &input.pane != pane || input.terminal_epoch != epoch || activation.is_some_and(|id| id != input.activation_id) {
            return Err("terminal input binding changed".into());
        }
        if let Some((sequence, bytes)) = self.sources.get(&input.source_id) {
            if input.sequence == *sequence && input.bytes == *bytes {
                return Ok(TerminalInputAck { sequence: input.sequence, already_applied: true });
            }
            if input.sequence != sequence.saturating_add(1) {
                return Err("terminal input sequence stale, reused or noncontiguous".into());
            }
        } else if input.sequence != 1 || self.sources.len() >= 64 {
            return Err("terminal input source unavailable".into());
        }
        write(&input.bytes)?;
        self.sources.insert(input.source_id.clone(), (input.sequence, input.bytes.clone()));
        Ok(TerminalInputAck { sequence: input.sequence, already_applied: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> TerminalInput {
        TerminalInput { pane: PaneRef::new("ws", "pane"), terminal_epoch: "epoch".into(), activation_id: 1,
            source_id: "source".into(), sequence: 1, bytes: vec![27, 91, 77, 32, 255, 128] }
    }
    #[test]
    fn binary_retry_is_exactly_once_and_conflicts_never_write() {
        let mut ledger = TerminalInputLedger::default(); let mut writes = vec![]; let mut request = input();
        let ack = ledger.apply(&request, &request.pane, "epoch", Some(1), |bytes| { writes.push(bytes.to_vec()); Ok(()) }).unwrap();
        assert!(!ack.already_applied); assert_eq!(writes, vec![request.bytes.clone()]);
        assert!(ledger.apply(&request, &request.pane, "epoch", Some(1), |_| panic!("duplicate write")).unwrap().already_applied);
        request.bytes[4] = 254;
        assert!(ledger.apply(&request, &request.pane, "epoch", Some(1), |_| panic!("conflicting write")).is_err());
        request.sequence = 3;
        assert!(ledger.apply(&request, &request.pane, "epoch", Some(1), |_| panic!("gap write")).is_err());
    }
    #[test]
    fn stale_binding_and_failed_sink_do_not_commit() {
        let mut ledger = TerminalInputLedger::default(); let request = input();
        assert!(ledger.apply(&request, &request.pane, "replacement", Some(1), |_| panic!("stale lifetime")).is_err());
        assert!(ledger.apply(&request, &request.pane, "epoch", Some(2), |_| panic!("stale activation")).is_err());
        assert!(ledger.apply(&request, &request.pane, "epoch", Some(1), |_| Err("closed".into())).is_err());
        assert!(!ledger.apply(&request, &request.pane, "epoch", Some(1), |_| Ok(())).unwrap().already_applied);
    }
}
