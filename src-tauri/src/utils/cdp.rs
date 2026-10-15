//! Explicit debug CDP isolation; release paths and OS integrations stay unchanged.
use std::path::PathBuf;
fn choose(default:PathBuf,override_dir:Option<PathBuf>,debug:bool)->PathBuf {
    if debug {
        if let Some(dir)=override_dir {
            assert!(dir.is_absolute(),"RIDGE_CDP_NATIVE_DATA_DIR must be absolute");
            return dir;
        }
    }
    default
}
pub fn isolated()->bool {
    cfg!(debug_assertions) && std::env::var_os("RIDGE_CDP_NATIVE_DATA_DIR").is_some()
}
pub fn local_data_dir()->PathBuf {
    choose(dirs::data_local_dir().unwrap_or_else(||PathBuf::from(".")).join("ridge"),
        std::env::var_os("RIDGE_CDP_NATIVE_DATA_DIR").map(PathBuf::from),cfg!(debug_assertions))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_and_unconfigured_debug_keep_existing_data_root() {
        let default=PathBuf::from("existing");
        assert_eq!(choose(default.clone(),None,true),default);
        assert_eq!(choose(default.clone(),Some(std::env::temp_dir()),false),default);
    }
    #[test]
    fn explicit_debug_root_is_used() {
        let root=std::env::temp_dir().join("owned-native-data");
        assert_eq!(choose(PathBuf::from("existing"),Some(root.clone()),true),root);
    }
    #[test]
    #[should_panic(expected="must be absolute")]
    fn relative_debug_root_fails_before_user_data_is_touched() {
        choose(PathBuf::from("existing"),Some(PathBuf::from("relative")),true);
    }
}
