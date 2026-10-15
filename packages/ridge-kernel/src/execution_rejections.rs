//! Runtime-owned external rejection cards; reports never authorize or retry execution.
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};
#[derive(Default)]
pub(crate) struct Rejections(Mutex<(VecDeque<Value>, VecDeque<Value>)>);
impl Rejections {
    pub(crate) fn report(&self, args: &Value) -> Result<String, String> {
        let mut card = json!({"id":uuid::Uuid::new_v4().to_string(),"kind":"external_rejection","level":"Dangerous"});
        for field in ["initiator", "action", "executor", "policySource", "requestId", "reason", "nextStep"] {
            let value = args[field].as_str().filter(|value| field == "action" || !value.trim().is_empty())
                .ok_or_else(|| format!("missing rejection field: {field}"))?;
            if value.len() > 65536 { return Err("execution rejection field too large".into()); }
            card[field] = json!(value);
        }
        let id = card["id"].as_str().unwrap().to_owned();
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?.as_millis() as u64;
        let audit = json!({"id":id,"ts":ts,"source":"external-execution-rejection","initiator":"external-agent",
            "verdict":"rejected-before-ridge","riskLevel":"Unknown","reasonSummary":"External execution gateway rejected operation",
            "outcome":"rejected-before-ridge"});
        let mut reports = self.0.lock().map_err(|_| "execution rejection registry unavailable")?;
        reports.0.push_back(card);
        reports.1.push_back(audit);
        while reports.0.len() > 50 { reports.0.pop_front(); }
        while reports.1.len() > 50 { reports.1.pop_front(); }
        Ok(id)
    }
    pub(crate) fn list(&self) -> Result<Value, String> {
        Ok(json!(self.0.lock().map_err(|_| "execution rejection registry unavailable")?.0))
    }
    pub(crate) fn audit(&self) -> Result<Vec<Value>, String> {
        Ok(self.0.lock().map_err(|_| "execution rejection registry unavailable")?.1.iter().cloned().collect())
    }
    pub(crate) fn dismiss(&self, id: &str) -> Result<bool, String> {
        let mut reports = self.0.lock().map_err(|_| "execution rejection registry unavailable")?;
        let Some(index) = reports.0.iter().position(|report| report["id"] == id) else { return Ok(false); };
        reports.0.remove(index);
        Ok(true)
    }
}
