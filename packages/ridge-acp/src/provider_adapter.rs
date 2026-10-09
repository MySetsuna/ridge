//! M4 live provider adapter — `ProviderSpec` → `SpawnSpec` with binary + args.
//!
//! Each `ProviderKind` maps to a real binary + wire framing + model args.
//! Today:
//! - `Mock`        → `fake_acp_agent` (ContentLength) — CI fixture
//! - `OpencodeAcp` → `opencode-acp` (NdJson, --model) — REAL opencode binary
//! - `Anthropic`   → `opencode-acp --model <anthropic-model>` (NdJson) — proxy
//! - `OpenAi`      → `opencode-acp --model <openai-model>` (NdJson) — proxy
//!
//! `Anthropic` / `OpenAi` route through `opencode-acp` because we lack a
//! direct SDK for those providers in this slice; opencode-acp's model
//! negotiation handles provider selection via the same ACP wire shape.

use std::path::PathBuf;

use crate::provider::ProviderSpec;
use crate::spawn::SpawnSpec;
use crate::transport::Framing;

/// What we actually launch on disk for a given `ProviderSpec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub framing: Framing,
    /// Human label surfaced in error messages and capability gaps.
    pub label: String,
}

/// M4 capability gap — surfaced when a provider claims something its
/// binary doesn't actually support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityGap {
    pub provider_kind: crate::provider::ProviderKind,
    pub capability: &'static str,
    pub binary: String,
    pub reason: String,
}

/// Resolve a `ProviderSpec` into the concrete binary we will spawn.
///
/// Errors only if a binary is missing from PATH; never blocks Mock.
/// `RIDGE_OPENCODE_ACP` env var overrides the opencode-acp path for local
/// swaps (matching the bridge's `RIDGE_ACP_PROVIDER_PATH`).
pub fn resolve_provider_launch(spec: &ProviderSpec) -> Result<ProviderLaunch, CapabilityGap> {
    use crate::provider::ProviderKind;
    match spec.kind {
        ProviderKind::Mock => Ok(ProviderLaunch {
            program: std::env::var("CARGO_BIN_EXE_fake_acp_agent")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("fake_acp_agent")),
            args: vec![],
            framing: Framing::ContentLength,
            label: "fake_acp_agent".into(),
        }),
        ProviderKind::Anthropic | ProviderKind::OpenAi | ProviderKind::OpencodeAcp => {
            let program = std::env::var("RIDGE_OPENCODE_ACP")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("opencode-acp"));
            let mut args = vec!["--model".to_string(), spec.model.id.clone()];
            if let Some(effort) = spec.model.effort {
                args.push("--effort".to_string());
                args.push(effort.as_str().to_string());
            }
            Ok(ProviderLaunch {
                program,
                args,
                framing: Framing::NdJson,
                label: format!("opencode-acp({:?})", spec.kind),
            })
        }
    }
}

/// Apply the launch resolution to a `SpawnSpec`. Pure helper.
pub fn into_spawn_spec(spec: &ProviderSpec) -> Result<SpawnSpec, CapabilityGap> {
    let launch = resolve_provider_launch(spec)?;
    Ok(SpawnSpec {
        program: launch.program,
        args: launch.args,
        cwd: spec.config.get("cwd").and_then(|v| v.as_str()).map(PathBuf::from),
        provider: spec.clone(),
        framing: launch.framing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ModelSpec, ProviderKind, ProviderSpec};

    fn mock_spec() -> ProviderSpec {
        ProviderSpec {
            kind: ProviderKind::Mock,
            model: ModelSpec::mock_default(),
            config: Default::default(),
        }
    }

    #[test]
    fn mock_resolves_to_fake_acp_agent() {
        let launch = resolve_provider_launch(&mock_spec()).unwrap();
        assert!(launch.program.ends_with("fake_acp_agent"));
        assert_eq!(launch.framing, Framing::ContentLength);
    }

    #[test]
    fn anthropic_routes_to_opencode_acp_with_model_arg() {
        let mut s = mock_spec();
        s.kind = ProviderKind::Anthropic;
        s.model.id = "claude-sonnet-4-5".into();
        let launch = resolve_provider_launch(&s).unwrap();
        assert!(launch.program.ends_with("opencode-acp"));
        assert!(launch.args.contains(&"--model".to_string()));
        assert!(launch.args.contains(&"claude-sonnet-4-5".to_string()));
        assert_eq!(launch.framing, Framing::NdJson);
    }

    #[test]
    fn openai_routes_to_opencode_acp_with_model_arg() {
        let mut s = mock_spec();
        s.kind = ProviderKind::OpenAi;
        s.model.id = "gpt-4o".into();
        let launch = resolve_provider_launch(&s).unwrap();
        assert!(launch.program.ends_with("opencode-acp"));
        assert!(launch.args.contains(&"gpt-4o".to_string()));
    }

    #[test]
    fn effort_becomes_arg() {
        use crate::provider::Effort;
        let mut s = mock_spec();
        s.kind = ProviderKind::Anthropic;
        s.model.effort = Some(Effort::High);
        let launch = resolve_provider_launch(&s).unwrap();
        assert!(launch.args.contains(&"--effort".to_string()));
        assert!(launch.args.contains(&"high".to_string()));
    }
}
