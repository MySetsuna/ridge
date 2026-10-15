pub fn get_orchestration_health() -> Result<Value, String> {
    let pending = get_pending_hitl_count()?;
    let suspended:u64=super::runtime::request("get_automation_pause_count",json!({}),true,super::runtime::window_connection("automation-pause"))?;
    let mut health = crate::teammate::orch_health::orchestration_health(hitl::is_enabled()?, pending as u64,suspended);
    // Transitional diagnostics retain their owners; approval truth belongs to Runtime.
    let enabled = health["hitlEnabled"].as_bool().unwrap_or(false) || pending > 0;
    let degraded = crate::teammate::orch_health::compute_degraded(
        health["suspendedAgents"].as_u64().unwrap_or(0), pending as u64, enabled);
    let level = crate::teammate::orch_health::control_plane_level(degraded,
        health["foreignAttached"].as_u64().unwrap_or(0), health["outboundHostsConnected"].as_u64().unwrap_or(0));
    health["pendingHitl"] = json!(pending);
    health["hitlEnabled"] = json!(enabled);
    health["degraded"] = json!(degraded);
    health["level"] = json!(level);
    health["pollHintMs"] = json!(crate::teammate::orch_health::poll_hint_ms(level));
    Ok(health)
}

