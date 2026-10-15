//! Bounded, opt-in presentation diagnostics. No terminal content or credentials.
use std::{sync::{OnceLock,atomic::{AtomicUsize,Ordering}},time::Instant};
const MAX_SAMPLES:usize=65536;
static ENABLED:OnceLock<bool>=OnceLock::new();
static SAMPLES:AtomicUsize=AtomicUsize::new(0);

pub(crate) fn begin()->Option<Instant>{
    (*ENABLED.get_or_init(||std::env::var("RIDGE_CLI_PERF_TRACE").is_ok_and(|value|value=="1"))).then(Instant::now)
}
fn record(mut value:serde_json::Value){
    value["pid"]=serde_json::json!(std::process::id());
    let count=SAMPLES.fetch_add(1,Ordering::Relaxed);
    if count<MAX_SAMPLES{tracing::info!("RIDGE_CLI_STAGE {}",value);}
    else if count==MAX_SAMPLES{tracing::info!("RIDGE_CLI_STAGE {}",serde_json::json!({"event":"truncated","limit":MAX_SAMPLES,"pid":std::process::id()}));}
}
pub(crate) fn finish(stage:&str,start:Option<Instant>,bytes:usize,queue_depth:usize){
    if let Some(start)=start{record(serde_json::json!({"event":"sample","stage":stage,"durationMs":start.elapsed().as_secs_f64()*1000.0,"bytes":bytes,"queueDepth":queue_depth}));}
}
pub(crate) fn error(code:&str){
    if begin().is_some(){record(serde_json::json!({"event":"error","code":code}));}
}
