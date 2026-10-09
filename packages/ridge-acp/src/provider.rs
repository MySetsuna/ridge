//! Provider / Model — pure data, no SDK. MOCK_FIXTURE is the only wired provider;
//! Anthropic / OpenAI live wiring lives in NEXT_SLICE (needs authorized creds).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Mock,
    Anthropic,
    #[serde(alias = "openai")]
    OpenAi,
    /// M4: explicit opencode-acp provider (real binary; NdJson framing).
    OpencodeAcp,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::OpencodeAcp => "opencode_acp",
        }
    }
}

/// M3: effort level (some providers support it; mock ignores).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    #[default]
    Medium,
    High,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: String,
    /// `base | skilled | expert` — reuses `AgentTier` semantics, not duplicated.
    pub tier: String,
    /// M3: optional effort hint. None means "provider default".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

impl ModelSpec {
    pub fn mock_default() -> Self {
        Self {
            id: "mock-claude-haiku".into(),
            tier: "base".into(),
            effort: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSpec {
    pub kind: ProviderKind,
    pub model: ModelSpec,
    /// Free-form provider configuration (API key env var name, base URL, etc.).
    /// MOCK_FIXTURE ignores it.
    #[serde(default)]
    pub config: serde_json::Map<String, serde_json::Value>,
}

impl ProviderSpec {
    pub fn mock_default() -> Self {
        Self {
            kind: ProviderKind::Mock,
            model: ModelSpec::mock_default(),
            config: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_metadata_uses_canonical_tag_and_accepts_legacy_alias() {
        assert_eq!(serde_json::to_value(ProviderKind::OpenAi).unwrap(), serde_json::json!("open_ai"));
        assert_eq!(serde_json::from_value::<ProviderKind>(serde_json::json!("openai")).unwrap(), ProviderKind::OpenAi);
    }

    #[test]
    fn provider_kind_tag_roundtrip() {
        let k = ProviderKind::Anthropic;
        let s = serde_json::to_string(&k).unwrap();
        assert_eq!(s, "\"anthropic\"");
        let back: ProviderKind = serde_json::from_str(&s).unwrap();
        assert_eq!(back, k);
    }

    #[test]
    fn provider_spec_json() {
        let p = ProviderSpec::mock_default();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["kind"], "mock");
        assert_eq!(v["model"]["tier"], "base");
    }

    #[test]
    fn effort_default_roundtrip() {
        let m = ModelSpec {
            id: "x".into(),
            tier: "base".into(),
            effort: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: ModelSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
        assert!(back.effort.is_none());
    }
}
