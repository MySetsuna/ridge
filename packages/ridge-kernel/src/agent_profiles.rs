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
            // Keep process discovery, but do not guess a session-store or
            // resume CLI contract until a real Gemini session fixture exists.
            resume_argv: vec![],
            yolo_args: vec!["--yolo".into()],
            yolo_position: "before".into(),
        },
        AgentProfile {
            id: "cursor-agent".into(),
            process_names: vec!["cursor-agent".into()],
            executable: "cursor-agent".into(),
            // Cursor Agent history is read by the host adapter when its local
            // `agent-transcripts` JSONL source exists; resume remains disabled
            // until a stable CLI contract is verified.
            resume_argv: vec![],
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

    #[test]
    fn unverified_builtins_fail_closed_for_resume() {
        let profiles = builtin_profiles();
        for id in ["gemini", "cursor-agent", "aider"] {
            let profile = profiles.iter().find(|profile| profile.id == id).unwrap();
            assert!(
                profile.resume_argv.is_empty(),
                "unexpected resume contract for {id}"
            );
        }
    }
}

static OVERRIDE_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
const OVERRIDE_FILE: &str = "agent-profile-overrides.json";
fn read_overrides() -> Result<serde_json::Value, String> {
    match std::fs::read(crate::registry::ridge_data_dir().join(OVERRIDE_FILE)) {
        Ok(bytes) => {
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| format!("invalid Agent catalog; original preserved: {error}"))?;
            validate_overrides(&value)?;
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!([])),
        Err(error) => Err(error.to_string()),
    }
}
fn validate_overrides(value: &serde_json::Value) -> Result<(), String> {
    let profiles: Vec<AgentProfile> = serde_json::from_value(value.clone()).map_err(|error| format!("invalid Agent catalog; original preserved: {error}"))?;
    if profiles.iter().any(|profile| profile.id.trim().is_empty()) {return Err("Agent profile ID must not be empty".into());}
    Ok(())
}
pub fn merge_profiles(overrides: &[AgentProfile]) -> Vec<AgentProfile> {
    let mut profiles = builtin_profiles().into_iter().map(|profile| (profile.id.clone(), profile)).collect::<std::collections::BTreeMap<_,_>>();
    for profile in overrides { if !profile.id.trim().is_empty() {profiles.insert(profile.id.clone(),profile.clone());} }
    profiles.into_values().collect()
}
pub(crate) fn catalog(method: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::{json, Value};
    let _guard = OVERRIDE_WRITES.lock().map_err(|_| "Agent catalog unavailable")?;
    if method == "read_agent_profile_overrides" {return read_overrides();}
    if method == "save_agent_profile_overrides" {
        let mut next = args["overrides"].clone(); validate_overrides(&next)?;
        let previous = read_overrides()?;
        for profile in next.as_array_mut().ok_or("overrides must be an array")? {
            if let Some(old) = previous.as_array().unwrap().iter().find(|old| old["id"] == profile["id"]) {
                for (key,value) in old.as_object().ok_or("invalid catalog row")? {
                    if !["id","processNames","executable","resumeArgv","yoloArgs","yoloPosition"].contains(&key.as_str())
                        && profile.get(key).is_none() {profile[key]=value.clone();}
                }
            }
        }
        crate::workspace_history::write_at(&crate::registry::ridge_data_dir().join(OVERRIDE_FILE),
            &serde_json::to_vec_pretty(&next).map_err(|error|error.to_string())?)?;
        return Ok(Value::Null);
    }
    let overrides = if args["overrides"].is_null() {read_overrides()?} else {validate_overrides(&args["overrides"])?;args["overrides"].clone()};
    let mut merged = builtin_profiles().into_iter().map(|profile| Ok((profile.id.clone(),serde_json::to_value(profile).map_err(|error|error.to_string())?)))
        .collect::<Result<std::collections::BTreeMap<_,_>,String>>()?;
    for profile in overrides.as_array().ok_or("overrides must be an array")? {
        let typed: AgentProfile=serde_json::from_value(profile.clone()).map_err(|error|error.to_string())?;
        let id=typed.id.clone();
        let mut normalized=serde_json::to_value(typed).map_err(|error|error.to_string())?;
        for (key,value) in profile.as_object().ok_or("invalid catalog row")? {normalized[key]=value.clone();}
        merged.insert(id,normalized);
    }
    Ok(json!(merged.into_values().collect::<Vec<_>>()))
}
