//! Pane display mode（terminal vs editor）。
//!
//! 从 `src-tauri/src/types.rs` 整体移入（D11 Wave A / P1）。桌面经
//! `crate::types::PaneMode` re-export 保持调用点不变。**serde 表示逐字不变**——
//! 它随 `Pane` 落进 `.ridge` 持久化（externally-tagged：`"Terminal"` /
//! `{"Editor":{"file_path":…,"language":…}}`），改动会破坏旧工作区文件。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PaneMode {
    Terminal,
    Editor {
        file_path: Option<PathBuf>,
        language: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_is_externally_tagged_unit() {
        // `.ridge` 落盘形态：裸字符串。改成 internally-tagged 会破坏旧文件。
        assert_eq!(
            serde_json::to_string(&PaneMode::Terminal).unwrap(),
            "\"Terminal\""
        );
        let back: PaneMode = serde_json::from_str("\"Terminal\"").unwrap();
        assert!(matches!(back, PaneMode::Terminal));
    }

    #[test]
    fn editor_serde_golden_round_trips() {
        let json = "{\"Editor\":{\"file_path\":\"/tmp/a.rs\",\"language\":\"rust\"}}";
        let back: PaneMode = serde_json::from_str(json).unwrap();
        match &back {
            PaneMode::Editor {
                file_path,
                language,
            } => {
                assert_eq!(file_path.as_ref().unwrap().to_str(), Some("/tmp/a.rs"));
                assert_eq!(language, "rust");
            }
            _ => panic!("expected Editor"),
        }
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn editor_with_no_file_path_round_trips() {
        let json = "{\"Editor\":{\"file_path\":null,\"language\":\"plaintext\"}}";
        let back: PaneMode = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }
}
