//! Single persistent TOTP and trust owner shared by every shell and Host transport.
use crate::server::AppState;
use ridge_core::{CoreError, CoreResult, EventSink};
use ridge_remote::auth::RemoteAuth;
use serde_json::Value;
use std::sync::Arc;

pub(crate) fn initialize() -> Result<Arc<RemoteAuth>,String> {
    let owned = std::env::var_os("RIDGE_CDP_NATIVE_DATA_DIR").filter(|_|cfg!(debug_assertions))
        .or_else(||std::env::var_os("RIDGE_KERNEL_DATA_DIR").filter(|root|!root.is_empty()));
    let (seeds,grants) = if let Some(root)=owned {
        let root=std::path::PathBuf::from(root);
        if !root.is_absolute() {return Err("authentication namespace must be absolute".into());}
        (root.join("totp"),root.join("grants"))
    } else {
        (ridge_core::RemoteTotp::seed_directory().ok_or("TOTP namespace unavailable")?,
         ridge_core::grant_store::grants_dir().ok_or("grants namespace unavailable")?)
    };
    RemoteAuth::persistent(seeds,grants).map(Arc::new)
}
pub(crate) fn dispatch(state:&AppState,method:&str,args:&Value,native:bool)->Option<CoreResult<Value>> {
    if !matches!(method,"remote_totp_info"|"verify_remote_totp"|"verify_remote_totp_bind"|"remote_reset_totp"|"remote_set_totp_identity"|"totp_trust_check"|"totp_trust_record"|"totp_trust_revoke_all") {return None;}
    Some((|| {
        if !native {return Err(CoreError::InvalidArgs("authentication requires native trust".into()));}
        // Preserve existing JS argument spelling while shell adapters use camelCase.
        let mut args=args.clone();
        if args["ctrlPubB64"].is_null() {args["ctrlPubB64"]=args["ctrl_pub_b64"].clone();}
        let value=state.remote_auth.call(method,args).map_err(CoreError::InvalidArgs)?;
        if matches!(method,"remote_reset_totp"|"remote_set_totp_identity") {
            state.business_events.broadcast("remote-totp-changed",Value::Null);
        }
        Ok(value)
    })())
}
