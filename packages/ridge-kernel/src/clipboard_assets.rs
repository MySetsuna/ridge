//! Clipboard attachment files belong to the terminal host, not its display process.
use crate::server::AppState;
use ridge_core::{CoreError,CoreResult,Ctx};
use serde_json::{json,Value};
use base64::Engine;
fn image_dir()->std::path::PathBuf{
    if std::env::var_os("RIDGE_KERNEL_DATA_DIR").is_some(){crate::registry::ridge_data_dir().join("clipboard")}else{std::env::temp_dir().join("wind-clipboard")}
}
/// Preserve the desktop's one-hour grace period; only the Runtime removes old attachments.
pub(crate) fn cleanup_old_images(){
    cleanup_dir(&image_dir(),std::time::SystemTime::now());
}
fn cleanup_dir(root:&std::path::Path,now:std::time::SystemTime){
    let Ok(entries)=std::fs::read_dir(root) else{return;};
    for entry in entries.flatten(){
        let name=entry.file_name();let name=name.to_string_lossy();
        if !name.starts_with("clip-")||!name.ends_with(".png"){continue;}
        let Ok(meta)=entry.metadata() else{continue;};
        if !meta.is_file(){continue;}
        let Ok(modified)=meta.modified() else{continue;};
        if now.duration_since(modified).is_ok_and(|age|age>std::time::Duration::from_secs(3600)){
            if let Err(error)=std::fs::remove_file(entry.path()){tracing::warn!(%error,"clipboard attachment cleanup failed");}
        }
    }
}
pub(crate) fn dispatch(state:&AppState,method:&str,args:&Value,ctx:&Ctx)->Option<CoreResult<Value>>{
    if !matches!(method,"save_clipboard_image_to_temp"|"resolve_pasted_image_path"){return None;}
    Some((||{
        if method=="resolve_pasted_image_path"{
            let text=args["text"].as_str().ok_or_else(||CoreError::InvalidArgs("text required".into()))?;
            let candidate=text.trim().trim_matches('"').trim();
            if candidate.contains(['\n','\r'])||!ridge_core::clipboard::is_image_ext(candidate){return Ok(Value::Null);}
            if !state.fs_scope.is_allowed(candidate)||!ctx.capabilities().root_scope().is_allowed(candidate){return Err(CoreError::OutsideSandbox);}
            return Ok(json!(ridge_core::clipboard::resolve_pasted_image_path(text)));
        }
        let encoded=args["pngBase64"].as_str().ok_or_else(||CoreError::InvalidArgs("pngBase64 required".into()))?;
        if encoded.len()>14*1024*1024{return Err(CoreError::InvalidArgs("clipboard image exceeds bound".into()));}
        let bytes=base64::engine::general_purpose::STANDARD.decode(encoded.split_once(',').map(|(_,data)|data).unwrap_or(encoded)).map_err(|_|CoreError::InvalidArgs("invalid image base64".into()))?;
        if bytes.len()>10*1024*1024||!bytes.starts_with(b"\x89PNG\r\n\x1a\n"){return Err(CoreError::InvalidArgs("clipboard image must be bounded PNG data".into()));}
        let root=image_dir();
        std::fs::create_dir_all(&root).map_err(|error|CoreError::Io(error.to_string()))?;
        let path=root.join(format!("clip-{}.png",uuid::Uuid::new_v4()));
        use std::io::Write;
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error|CoreError::Io(error.to_string()))?;
        file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|error|CoreError::Io(error.to_string()))?;
        Ok(json!(path))
    })())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn cleanup_preserves_recent_images_and_unrelated_files(){
        let root=std::env::temp_dir().join(format!("ridge-clipboard-cleanup-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let now=std::time::SystemTime::now();
        for name in ["clip-old.png","clip-recent.png","unrelated.png"]{
            let file=std::fs::File::create(root.join(name)).unwrap();
            let age=if name=="clip-recent.png"{30}else{7200};
            file.set_times(std::fs::FileTimes::new().set_modified(now-std::time::Duration::from_secs(age))).unwrap();
        }
        cleanup_dir(&root,now);
        assert!(!root.join("clip-old.png").exists());
        assert!(root.join("clip-recent.png").exists());
        assert!(root.join("unrelated.png").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
