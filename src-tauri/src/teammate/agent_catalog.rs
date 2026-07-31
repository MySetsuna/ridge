//! Agent 识别 / 恢复 / YOLO 启动参数的单一配置表（内置默认 + 可序列化覆盖）。
//!
//! 发现（process name）、历史 resume 计划、设置面板都读这里，避免多处魔法字符串。
//! 用户覆盖同时驻内存 + `%LOCALAPPDATA%/ridge/agent-profile-overrides.json`，
//! 供 autodiscover 在无前端参数时读取（设置只写 localStorage 则发现永不认自定义进程名）。

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// 进程内覆盖缓存。`None` = 尚未从磁盘 hydrate。
static PROFILE_OVERRIDES: Mutex<Option<Vec<AgentProfile>>> = Mutex::new(None);

/// 内置流行 agent 默认行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    /// 稳定 id（展示与配置键）：`claude` / `codex` / `grok` …
    pub id: String,
    /// 进程名 stem 子串（小写匹配，可含多个别名）。
    pub process_names: Vec<String>,
    /// 启动可执行名（PATH / 用户安装）。
    pub executable: String,
    /// resume argv 模板：`{session}` 替换为会话 id。
    pub resume_argv: Vec<String>,
    /// YOLO 模式追加参数（开开关时接在 resume 前或后，见 `yolo_position`）。
    pub yolo_args: Vec<String>,
    /// `before` = yolo 参数在 resume 参数之前；`after` = 之后。
    #[serde(default = "default_yolo_position")]
    pub yolo_position: String,
}

fn default_yolo_position() -> String {
    "before".into()
}

/// 内置默认表：基础流行 agent。识别只认 process_names。
pub fn builtin_profiles() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: "claude".into(),
            process_names: vec!["claude".into(), "claude-code".into()],
            executable: "claude".into(),
            resume_argv: vec!["--resume".into(), "{session}".into()],
            // Claude Code：跳过权限提示（yolo）
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
            // Grok Build：自动批准工具
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
    let mut map: std::collections::BTreeMap<String, AgentProfile> = builtin_profiles()
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    for o in overrides {
        if o.id.trim().is_empty() {
            continue;
        }
        map.insert(o.id.clone(), o.clone());
    }
    map.into_values().collect()
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

/// 覆盖文件路径：`<data_local>/ridge/agent-profile-overrides.json`。
pub fn profile_overrides_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
        .join("agent-profile-overrides.json")
}

fn read_overrides_disk() -> Vec<AgentProfile> {
    let path = profile_overrides_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<AgentProfile>>(&raw).unwrap_or_default()
}

/// 当前用户覆盖（懒加载磁盘）。autodiscover / scan 走此入口。
pub fn load_profile_overrides() -> Vec<AgentProfile> {
    let mut guard = PROFILE_OVERRIDES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = guard.as_ref() {
        return cached.clone();
    }
    let from_disk = read_overrides_disk();
    *guard = Some(from_disk.clone());
    from_disk
}

/// 仅内存（单测 / 热更新）。不写盘。
pub fn set_profile_overrides_in_memory(overrides: Vec<AgentProfile>) {
    let mut guard = PROFILE_OVERRIDES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *guard = Some(overrides);
}

/// 持久化并刷新内存。成功后调用方应失效 autodiscover TTL 缓存。
pub fn save_profile_overrides(overrides: Vec<AgentProfile>) -> Result<(), String> {
    let path = profile_overrides_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create override dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(&overrides)
        .map_err(|e| format!("serialize overrides: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write overrides: {e}"))?;
    set_profile_overrides_in_memory(overrides);
    Ok(())
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
        let prev = load_profile_overrides();
        set_profile_overrides_in_memory(vec![AgentProfile {
            id: "custom-x".into(),
            process_names: vec!["custom-x-bin".into()],
            executable: "custom-x-bin".into(),
            resume_argv: vec![],
            yolo_args: vec![],
            yolo_position: "before".into(),
        }]);
        let loaded = load_profile_overrides();
        set_profile_overrides_in_memory(prev);
        assert!(known_process_names(&loaded)
            .iter()
            .any(|n| n == "custom-x-bin"));
    }
}
