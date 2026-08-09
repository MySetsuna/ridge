//! Agent launch-profile domain data owned by the Ridge kernel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub process_names: Vec<String>,
    pub executable: String,
    pub resume_argv: Vec<String>,
    pub yolo_args: Vec<String>,
    #[serde(default = "default_yolo_position")]
    pub yolo_position: String,
}

fn default_yolo_position() -> String {
    "before".into()
}

pub fn builtin_profiles() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: "claude".into(),
            process_names: vec!["claude".into(), "claude-code".into()],
            executable: "claude".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            yolo_args: vec!["--dangerously-skip-permissions".into()],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "codex".into(),
            process_names: vec!["codex".into()],
            executable: "codex".into(),
            resume_argv: vec!["resume".into(), "{session}".into()],
            yolo_args: vec!["--dangerously-bypass-approvals-and-sandbox".into()],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "grok".into(),
            process_names: vec!["grok".into()],
            executable: "grok".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            yolo_args: vec!["--always-approve".into()],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "gemini".into(),
            process_names: vec!["gemini".into()],
            executable: "gemini".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            yolo_args: vec!["--yolo".into()],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "cursor-agent".into(),
            process_names: vec!["cursor-agent".into()],
            executable: "cursor-agent".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            yolo_args: vec![],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "aider".into(),
            process_names: vec!["aider".into()],
            executable: "aider".into(),
            resume_argv: vec![],
            yolo_args: vec!["--yes".into()],
            yolo_position: "before".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_profiles_keep_resume_contracts() {
        let codex = builtin_profiles()
            .into_iter()
            .find(|p| p.id == "codex")
            .unwrap();
        assert_eq!(codex.resume_argv, ["resume", "{session}"]);
    }
}
