//! Host-facing delivery adapter contract for the Message Hub.
//!
//! MCP pull is the durable default. Runtime API, A2A, and PTY fallback are
//! opt-in only after the host proves the corresponding capability; this keeps
//! the Hub honest when an Agent merely happens to have a recognizable name.

use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HubDeliveryAdapter {
    RuntimeApi,
    A2a,
    McpPull,
    PtyFallback,
}

impl HubDeliveryAdapter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeApi => "runtime_api",
            Self::A2a => "a2a",
            Self::McpPull => "mcp_pull",
            Self::PtyFallback => "pty",
        }
    }

    pub const fn reliability(self) -> &'static str {
        match self {
            Self::RuntimeApi => "durable",
            Self::A2a | Self::McpPull => "at_least_once",
            Self::PtyFallback => "best_effort",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubPtySafety {
    pub agent_idle: bool,
    pub terminal_mode_agent_prompt: bool,
    pub pending_approval: bool,
    pub foreground_is_target_agent: bool,
    pub user_input_competing: bool,
}

impl HubPtySafety {
    pub const fn is_safe(self) -> bool {
        self.agent_idle
            && self.terminal_mode_agent_prompt
            && !self.pending_approval
            && self.foreground_is_target_agent
            && !self.user_input_competing
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryProbe {
    pub runtime_api: bool,
    pub a2a: bool,
    pub mcp_pull: bool,
    pub pty: HubPtySafety,
}

pub fn choose_delivery_adapter(probe: DeliveryProbe) -> Option<HubDeliveryAdapter> {
    if probe.runtime_api {
        return Some(HubDeliveryAdapter::RuntimeApi);
    }
    if probe.a2a {
        return Some(HubDeliveryAdapter::A2a);
    }
    if probe.mcp_pull {
        return Some(HubDeliveryAdapter::McpPull);
    }
    if probe.pty.is_safe() {
        return Some(HubDeliveryAdapter::PtyFallback);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryOutcome {
    pub adapter: HubDeliveryAdapter,
    pub accepted: bool,
    pub remote_id: Option<String>,
    pub acknowledged: bool,
}

/// Maximum number of entries retained by one in-process Agent Runtime/A2A
/// subscription. `try_send` keeps the MCP request path non-blocking; a full
/// route fails visibly and leaves the durable Hub receipt available for pull.
pub const DELIVERY_ROUTE_CAP: usize = 256;

/// A PTY proof is a short-lived observation, not a permanent capability.
/// Hosts must refresh it before this bound or delivery falls back to MCP pull.
pub const PTY_SAFETY_MAX_AGE: Duration = Duration::from_secs(3);

struct DeliveryRoute {
    generation: u64,
    lease: String,
    sender: SyncSender<Value>,
}

struct PtySafetyRoute {
    generation: u64,
    lease: String,
    safety: HubPtySafety,
    observed_at: Instant,
}

/// Host-owned delivery endpoints for runtimes that live in the same process.
///
/// The registry is deliberately transport-neutral: a Runtime API or A2A
/// integration registers a bounded receiver after it has authenticated the
/// Agent identity. The Hub only advertises a route when generation and lease
/// match, so reconnects cannot inherit an old subscription.
pub struct DeliveryRegistry {
    routes: Mutex<HashMap<(HubDeliveryAdapter, String), DeliveryRoute>>,
    pty_safety: Mutex<HashMap<String, PtySafetyRoute>>,
}

impl Default for DeliveryRegistry {
    fn default() -> Self {
        Self {
            routes: Mutex::new(HashMap::new()),
            pty_safety: Mutex::new(HashMap::new()),
        }
    }
}

impl DeliveryRegistry {
    pub fn register(
        &self,
        adapter: HubDeliveryAdapter,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
    ) -> Result<Receiver<Value>, String> {
        if !matches!(
            adapter,
            HubDeliveryAdapter::RuntimeApi | HubDeliveryAdapter::A2a
        ) {
            return Err("only Runtime API and A2A routes may register".into());
        }
        let agent_id = agent_id.into();
        let lease = lease.into();
        if agent_id.trim().is_empty() || lease.trim().is_empty() || generation == 0 {
            return Err("delivery route requires agent_id, generation, and lease".into());
        }
        let (sender, receiver) = sync_channel(DELIVERY_ROUTE_CAP);
        let mut routes = self
            .routes
            .lock()
            .map_err(|_| "delivery registry lock poisoned".to_string())?;
        let key = (adapter, agent_id);
        if let Some(current) = routes.get(&key) {
            if current.generation >= generation {
                return Err(if current.generation == generation {
                    "delivery route already registered".into()
                } else {
                    "stale delivery route registration rejected".into()
                });
            }
            // A reconnect may publish its new fenced route before the old
            // process finishes teardown. Replacing the old sender atomically
            // disconnects its receiver; generation/lease checks still fence
            // every late send and stale teardown against the new route.
        }
        routes.insert(
            key,
            DeliveryRoute {
                generation,
                lease,
                sender,
            },
        );
        Ok(receiver)
    }

    pub fn unregister(
        &self,
        adapter: HubDeliveryAdapter,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        let mut routes = self
            .routes
            .lock()
            .map_err(|_| "delivery registry lock poisoned".to_string())?;
        let key = (adapter, agent_id.to_string());
        let Some(route) = routes.get(&key) else {
            return Ok(false);
        };
        if route.generation != generation || route.lease != lease {
            return Err("stale delivery route teardown rejected".into());
        }
        routes.remove(&key);
        Ok(true)
    }

    /// Publish the complete PTY safety proof for one live Agent generation.
    ///
    /// This is deliberately separate from Runtime API/A2A route registration:
    /// a host may prove that PTY input is safe without exposing an in-process
    /// receiver, and a receiver teardown must never leave an old PTY proof
    /// available for a later generation. Same-generation refresh is
    /// idempotent and replaces the snapshot atomically.
    pub fn register_pty_safety(
        &self,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
        safety: HubPtySafety,
    ) -> Result<(), String> {
        let agent_id = agent_id.into();
        let lease = lease.into();
        if agent_id.trim().is_empty() || lease.trim().is_empty() || generation == 0 {
            return Err("PTY safety proof requires agent_id, generation, and lease".into());
        }
        let mut proofs = self
            .pty_safety
            .lock()
            .map_err(|_| "PTY safety registry lock poisoned".to_string())?;
        if let Some(current) = proofs.get(&agent_id) {
            if current.generation > generation {
                return Err("stale PTY safety proof rejected".into());
            }
            if current.generation == generation && current.lease != lease {
                return Err("PTY safety proof lease mismatch".into());
            }
        }
        proofs.insert(
            agent_id,
            PtySafetyRoute {
                generation,
                lease,
                safety,
                observed_at: Instant::now(),
            },
        );
        Ok(())
    }

    /// Remove a PTY proof only with the current generation/lease. A stale
    /// teardown must not erase a newer proof.
    pub fn unregister_pty_safety(
        &self,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        let mut proofs = self
            .pty_safety
            .lock()
            .map_err(|_| "PTY safety registry lock poisoned".to_string())?;
        let Some(current) = proofs.get(agent_id) else {
            return Ok(false);
        };
        if current.generation != generation || current.lease != lease {
            return Err("stale PTY safety teardown rejected".into());
        }
        proofs.remove(agent_id);
        Ok(true)
    }

    pub fn probe(&self, target: &Value) -> DeliveryProbe {
        let Some((agent_id, generation, lease)) = target_identity(target) else {
            return DeliveryProbe::default();
        };
        let Ok(routes) = self.routes.lock() else {
            return DeliveryProbe::default();
        };
        let matches = |adapter| {
            routes
                .get(&(adapter, agent_id.to_string()))
                .is_some_and(|route| route.generation == generation && route.lease == lease)
        };
        let pty = self
            .pty_safety
            .lock()
            .ok()
            .and_then(|proofs| {
                proofs.get(agent_id).and_then(|proof| {
                    (proof.generation == generation
                        && proof.lease == lease
                        && proof.observed_at.elapsed() <= PTY_SAFETY_MAX_AGE)
                        .then_some(proof.safety)
                })
            })
            .unwrap_or_default();
        DeliveryProbe {
            runtime_api: matches(HubDeliveryAdapter::RuntimeApi),
            a2a: matches(HubDeliveryAdapter::A2a),
            pty,
            ..DeliveryProbe::default()
        }
    }

    pub fn deliver(
        &self,
        adapter: HubDeliveryAdapter,
        target: &Value,
        entry: &Value,
    ) -> Result<DeliveryOutcome, String> {
        let Some((agent_id, generation, lease)) = target_identity(target) else {
            return Err("delivery target lacks agent_id, generation, or lease".into());
        };
        let routes = self
            .routes
            .lock()
            .map_err(|_| "delivery registry lock poisoned".to_string())?;
        let route = routes
            .get(&(adapter, agent_id.to_string()))
            .ok_or_else(|| "delivery route is not registered".to_string())?;
        if route.generation != generation || route.lease != lease {
            return Err("delivery route generation or lease is stale".into());
        }
        // Keep the registry lock through `try_send`: unregister cannot commit
        // while a delivery is in flight, so route teardown is a real fence.
        match route.sender.try_send(entry.clone()) {
            Ok(()) => Ok(DeliveryOutcome {
                adapter,
                accepted: true,
                remote_id: entry
                    .get("messageId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                acknowledged: false,
            }),
            Err(TrySendError::Full(_)) => Err("delivery route queue is full".into()),
            Err(TrySendError::Disconnected(_)) => Err("delivery route is disconnected".into()),
        }
    }
}

fn target_identity(target: &Value) -> Option<(&str, u64, &str)> {
    let agent_id = target
        .get("agentId")
        .or_else(|| target.get("agent_id"))
        .and_then(Value::as_str)?;
    let generation = target.get("generation").and_then(Value::as_u64)?;
    let lease = target.get("lease").and_then(Value::as_str)?;
    Some((agent_id, generation, lease))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::TryRecvError;

    use super::*;

    #[test]
    fn selection_is_strictly_ordered_and_pty_is_five_condition_gated() {
        assert_eq!(
            choose_delivery_adapter(DeliveryProbe {
                runtime_api: true,
                a2a: true,
                mcp_pull: true,
                ..Default::default()
            }),
            Some(HubDeliveryAdapter::RuntimeApi)
        );
        assert_eq!(
            choose_delivery_adapter(DeliveryProbe {
                a2a: true,
                mcp_pull: true,
                ..Default::default()
            }),
            Some(HubDeliveryAdapter::A2a)
        );
        assert_eq!(
            choose_delivery_adapter(DeliveryProbe {
                mcp_pull: true,
                ..Default::default()
            }),
            Some(HubDeliveryAdapter::McpPull)
        );
        let safe = HubPtySafety {
            agent_idle: true,
            terminal_mode_agent_prompt: true,
            foreground_is_target_agent: true,
            ..Default::default()
        };
        assert_eq!(
            choose_delivery_adapter(DeliveryProbe {
                pty: safe,
                ..Default::default()
            }),
            Some(HubDeliveryAdapter::PtyFallback)
        );
        for field in [
            HubPtySafety {
                agent_idle: false,
                ..safe
            },
            HubPtySafety {
                terminal_mode_agent_prompt: false,
                ..safe
            },
            HubPtySafety {
                pending_approval: true,
                ..safe
            },
            HubPtySafety {
                foreground_is_target_agent: false,
                ..safe
            },
            HubPtySafety {
                user_input_competing: true,
                ..safe
            },
        ] {
            assert_eq!(
                choose_delivery_adapter(DeliveryProbe {
                    pty: field,
                    ..Default::default()
                }),
                None
            );
        }
    }

    #[test]
    fn registered_route_is_fenced_and_non_blocking() {
        let registry = DeliveryRegistry::default();
        let receiver = registry
            .register(HubDeliveryAdapter::RuntimeApi, "agent-a", 2, "lease-2")
            .unwrap();
        let target = serde_json::json!({
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        });
        assert!(registry.probe(&target).runtime_api);
        assert!(
            !registry
                .probe(&serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 1,
                    "lease": "lease-2"
                }))
                .runtime_api
        );
        let entry = serde_json::json!({"messageId": "message-1"});
        let outcome = registry
            .deliver(HubDeliveryAdapter::RuntimeApi, &target, &entry)
            .unwrap();
        assert_eq!(outcome.remote_id.as_deref(), Some("message-1"));
        assert_eq!(receiver.recv().unwrap(), entry);
        assert!(registry
            .deliver(
                HubDeliveryAdapter::RuntimeApi,
                &serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 1,
                    "lease": "lease-2"
                }),
                &serde_json::json!({"messageId": "stale"})
            )
            .is_err());
    }

    #[test]
    fn route_teardown_requires_current_fence() {
        let registry = DeliveryRegistry::default();
        let _receiver = registry
            .register(HubDeliveryAdapter::A2a, "agent-a", 3, "lease-3")
            .unwrap();
        assert!(registry
            .unregister(HubDeliveryAdapter::A2a, "agent-a", 2, "lease-3")
            .is_err());
        assert!(registry
            .unregister(HubDeliveryAdapter::A2a, "agent-a", 3, "lease-3")
            .unwrap());
    }

    #[test]
    fn pty_safety_proof_is_generation_and_lease_fenced() {
        let registry = DeliveryRegistry::default();
        let safe = HubPtySafety {
            agent_idle: true,
            terminal_mode_agent_prompt: true,
            foreground_is_target_agent: true,
            ..Default::default()
        };
        let current = serde_json::json!({
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        });
        assert_eq!(registry.probe(&current).pty, HubPtySafety::default());
        assert_eq!(choose_delivery_adapter(registry.probe(&current)), None);

        registry
            .register_pty_safety("agent-a", 2, "lease-2", safe)
            .unwrap();
        assert_eq!(registry.probe(&current).pty, safe);
        assert_eq!(
            choose_delivery_adapter(registry.probe(&current)),
            Some(HubDeliveryAdapter::PtyFallback)
        );

        let stale = serde_json::json!({
            "agentId": "agent-a",
            "generation": 1,
            "lease": "lease-1"
        });
        assert_eq!(registry.probe(&stale).pty, HubPtySafety::default());
        assert_eq!(choose_delivery_adapter(registry.probe(&stale)), None);
        assert!(registry
            .register_pty_safety("agent-a", 2, "other-lease", safe)
            .is_err());

        registry
            .register_pty_safety("agent-a", 3, "lease-3", safe)
            .unwrap();
        assert!(registry
            .register_pty_safety("agent-a", 2, "lease-2", safe)
            .is_err());
        assert!(registry
            .unregister_pty_safety("agent-a", 2, "lease-2")
            .is_err());
        assert!(registry
            .unregister_pty_safety("agent-a", 3, "lease-3")
            .unwrap());
        assert_eq!(
            registry
                .probe(&serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 3,
                    "lease": "lease-3"
                }))
                .pty,
            HubPtySafety::default()
        );
    }

    #[test]
    fn pty_safety_proof_rejects_invalid_identity_and_refreshes_current_snapshot() {
        let registry = DeliveryRegistry::default();
        let safe = HubPtySafety {
            agent_idle: true,
            terminal_mode_agent_prompt: true,
            foreground_is_target_agent: true,
            ..Default::default()
        };
        assert!(registry
            .register_pty_safety("", 1, "lease-1", safe)
            .is_err());
        assert!(registry
            .register_pty_safety("agent-a", 0, "lease-1", safe)
            .is_err());
        assert!(registry
            .register_pty_safety("agent-a", 1, "", safe)
            .is_err());

        registry
            .register_pty_safety("agent-a", 1, "lease-1", safe)
            .unwrap();
        let unsafe_snapshot = HubPtySafety {
            user_input_competing: true,
            ..safe
        };
        registry
            .register_pty_safety("agent-a", 1, "lease-1", unsafe_snapshot)
            .unwrap();
        assert_eq!(
            registry
                .probe(&serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 1,
                    "lease": "lease-1"
                }))
                .pty,
            unsafe_snapshot
        );
        assert_eq!(
            registry.unregister_pty_safety("agent-a", 1, "lease-1"),
            Ok(true)
        );
        assert_eq!(
            registry.unregister_pty_safety("agent-a", 1, "lease-1"),
            Ok(false)
        );
    }

    #[test]
    fn expired_pty_safety_proof_falls_back_to_pull() {
        let registry = DeliveryRegistry::default();
        registry.pty_safety.lock().unwrap().insert(
            "agent-a".into(),
            PtySafetyRoute {
                generation: 1,
                lease: "lease-1".into(),
                safety: HubPtySafety {
                    agent_idle: true,
                    terminal_mode_agent_prompt: true,
                    foreground_is_target_agent: true,
                    ..Default::default()
                },
                observed_at: Instant::now() - PTY_SAFETY_MAX_AGE - Duration::from_millis(1),
            },
        );
        let target = serde_json::json!({
            "agentId": "agent-a",
            "generation": 1,
            "lease": "lease-1"
        });
        let probe = registry.probe(&target);
        assert_eq!(probe.pty, HubPtySafety::default());
        assert_eq!(choose_delivery_adapter(probe), None);
    }

    #[test]
    fn newer_generation_replaces_old_route_and_fences_late_teardown() {
        let registry = DeliveryRegistry::default();
        let old_receiver = registry
            .register(HubDeliveryAdapter::RuntimeApi, "agent-a", 3, "lease-3")
            .unwrap();
        let new_receiver = registry
            .register(HubDeliveryAdapter::RuntimeApi, "agent-a", 4, "lease-4")
            .unwrap();

        assert!(matches!(
            old_receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(
            !registry
                .probe(&serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 3,
                    "lease": "lease-3"
                }))
                .runtime_api
        );
        assert!(
            registry
                .probe(&serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 4,
                    "lease": "lease-4"
                }))
                .runtime_api
        );
        assert!(registry
            .register(HubDeliveryAdapter::RuntimeApi, "agent-a", 2, "lease-2")
            .is_err());

        let entry = serde_json::json!({ "messageId": "message-4" });
        assert!(registry
            .deliver(
                HubDeliveryAdapter::RuntimeApi,
                &serde_json::json!({
                    "agentId": "agent-a",
                    "generation": 4,
                    "lease": "lease-4"
                }),
                &entry,
            )
            .is_ok());
        assert_eq!(new_receiver.try_recv().unwrap(), entry);
        assert!(registry
            .unregister(HubDeliveryAdapter::RuntimeApi, "agent-a", 3, "lease-3")
            .is_err());
        assert!(registry
            .unregister(HubDeliveryAdapter::RuntimeApi, "agent-a", 4, "lease-4")
            .unwrap());
    }
}
