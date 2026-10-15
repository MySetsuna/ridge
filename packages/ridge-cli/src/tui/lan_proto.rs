//! Shared Host control envelopes. Terminal input/output use ridge-term's canonical protocol.
use serde_json::json;

pub fn list_panes()->String {json!({"type":"list-panes"}).to_string()}
pub fn create_pane()->String {json!({"type":"create-pane"}).to_string()}
pub fn claim_pane(pane_id:&str,rows:u16,cols:u16,seq:u64)->String{
    json!({"type":"claim-pane","paneId":pane_id,"rows":rows,"cols":cols,"pixelWidth":0,"pixelHeight":0,"seq":seq}).to_string()
}
