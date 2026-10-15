//! Compatibility projection over the Runtime Agent catalog; no desktop file writer.
//! Resume-plan helpers remain transitional and do not grant Commune protocol control.

#[cfg(test)]
use std::sync::Mutex;

/// 进程内覆盖缓存。`None` = 尚未从磁盘 hydrate。
#[cfg(test)]
static PROFILE_OVERRIDES: Mutex<Option<Vec<AgentProfile>>> = Mutex::new(None);

pub use ridge_kernel::agent_profiles::AgentProfile;

/// 内置默认表：基础流行 agent。识别只认 process_names。
pub fn builtin_profiles() -> Vec<AgentProfile> {
    ridge_kernel::agent_profiles::builtin_profiles()
}

/// 发现用进程名单：合并内置 + 用户覆盖（按 id 覆盖整行）。
pub fn known_process_names(overrides: &[AgentProfile]) -> Vec<String> {
    let merged = merge_profiles(overrides);
    let mut names = Vec::new();
    for p in merged {
        for n in p.process_names {
            let n = n.to_ascii_lowercase();
            if !n.is_empty() && !names.contains(&n) {
                names.push(n);
            }
        }
    }
    names
}

pub fn merge_profiles(overrides: &[AgentProfile]) -> Vec<AgentProfile> {
    ridge_kernel::agent_profiles::merge_profiles(overrides)
}

pub fn find_profile<'a>(
    profiles: &'a [AgentProfile],
    agent_or_stem: &str,
) -> Option<&'a AgentProfile> {
    let key = agent_or_stem.to_ascii_lowercase();
    profiles.iter().find(|p| {
        p.id.eq_ignore_ascii_case(&key)
            || p.process_names
                .iter()
                .any(|n| key.contains(&n.to_ascii_lowercase()) || n.eq_ignore_ascii_case(&key))
    })
}

/// 产出恢复启动计划（cwd + executable + argv），yolo 开关控制是否插入 yolo_args。
pub fn plan_resume(
    profile: &AgentProfile,
    session_id: &str,
    cwd: &str,
    yolo: bool,
) -> (String, Vec<String>, String) {
    let mut argv: Vec<String> = profile
        .resume_argv
        .iter()
        .map(|part| part.replace("{session}", session_id))
        .collect();
    if yolo && !profile.yolo_args.is_empty() {
        if profile.yolo_position.eq_ignore_ascii_case("after") {
            argv.extend(profile.yolo_args.iter().cloned());
        } else {
            let mut with_yolo = profile.yolo_args.clone();
            with_yolo.append(&mut argv);
            argv = with_yolo;
        }
    }
    (profile.executable.clone(), argv, cwd.to_string())
}

#[cfg(not(test))]
pub fn load_profile_overrides() -> Result<Vec<AgentProfile>,String> {
    crate::commands::runtime::request("read_agent_profile_overrides", serde_json::json!({}), true,
        crate::commands::runtime::window_connection("agent-catalog"))
}
#[cfg(test)]
pub fn load_profile_overrides() -> Result<Vec<AgentProfile>,String> {
    Ok(PROFILE_OVERRIDES.lock().map_err(|_| "test catalog unavailable")?.clone().unwrap_or_default())
}
#[cfg(test)]
pub fn set_profile_overrides_in_memory(overrides: Vec<AgentProfile>) {
    *PROFILE_OVERRIDES.lock().unwrap() = Some(overrides);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_include_grok_and_codex() {
        let names = known_process_names(&[]);
        assert!(names.iter().any(|n| n == "grok"));
        assert!(names.iter().any(|n| n == "codex"));
        assert!(names.iter().any(|n| n == "claude"));
    }

    #[test]
    fn plan_resume_grok_yolo_injects_always_approve() {
        let profiles = builtin_profiles();
        let p = find_profile(&profiles, "grok").unwrap();
        let (exe, argv, cwd) = plan_resume(p, "sess-1", r"C:\code\wind", true);
        assert_eq!(exe, "grok");
        assert_eq!(cwd, r"C:\code\wind");
        assert!(argv.iter().any(|a| a == "--always-approve"));
        assert!(argv.iter().any(|a| a == "--resume"));
        assert!(argv.iter().any(|a| a == "sess-1"));
    }

    #[test]
    fn plan_resume_codex_without_yolo_is_clean() {
        let profiles = builtin_profiles();
        let p = find_profile(&profiles, "codex").unwrap();
        let (_, argv, _) = plan_resume(p, "codex-1", "/repo", false);
        assert_eq!(argv, vec!["resume".to_string(), "codex-1".to_string()]);
        assert!(!argv.iter().any(|a| a.contains("dangerously")));
    }

    #[test]
    fn user_override_replaces_process_names() {
        let overrides = vec![AgentProfile {
            id: "grok".into(),
            process_names: vec!["grok".into(), "my-grok".into()],
            executable: "grok".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            yolo_args: vec!["--always-approve".into()],
            yolo_position: "before".into(),
        }];
        let names = known_process_names(&overrides);
        assert!(names.iter().any(|n| n == "my-grok"));
    }

    #[test]
    fn in_memory_overrides_roundtrip_for_discovery() {
        let prev = load_profile_overrides().unwrap();
        set_profile_overrides_in_memory(vec![AgentProfile {
            id: "custom-x".into(),
            process_names: vec!["custom-x-bin".into()],
            executable: "custom-x-bin".into(),
            resume_argv: vec![],
            yolo_args: vec![],
            yolo_position: "before".into(),
        }]);
        let loaded = load_profile_overrides().unwrap();
        set_profile_overrides_in_memory(prev);
        assert!(known_process_names(&loaded)
            .iter()
            .any(|n| n == "custom-x-bin"));
    }
}
