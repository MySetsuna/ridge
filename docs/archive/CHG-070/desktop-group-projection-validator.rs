/// Validate the small workspace-memory projection before accepting a Remote
/// write. Unknown fields remain forward-compatible, but IDs/names/members are
/// bounded so a controller cannot turn the sidecar into an unbounded payload.
fn validate_teammate_groups(groups: &Value) -> Result<(), String> {
    let Some(items) = groups.as_array() else {
        return Err("groups must be an array".into());
    };
    if items.len() > 128 {
        return Err("too many groups".into());
    }
    for (index, item) in items.iter().enumerate() {
        validate_teammate_group(item, index)?;
    }
    Ok(())
}

fn validate_teammate_group(item: &Value, index: usize) -> Result<(), String> {
    let Some(group) = item.as_object() else {
        return Err(format!("group {index} must be an object"));
    };
    validate_group_fields(group, index)?;
    validate_group_members(group, index)?;
    if group
        .get("leaderAgentId")
        .is_some_and(|leader| !leader.is_null() && leader.as_str().is_none())
    {
        return Err(format!("group {index} has invalid leaderAgentId"));
    }
    Ok(())
}

fn validate_group_fields(
    group: &serde_json::Map<String, Value>,
    index: usize,
) -> Result<(), String> {
    for key in ["id", "name", "color"] {
        let Some(value) = group.get(key).and_then(Value::as_str) else {
            return Err(format!("group {index} missing {key}"));
        };
        if value.trim().is_empty() || value.len() > 128 {
            return Err(format!("group {index} invalid {key}"));
        }
    }
    Ok(())
}

fn validate_group_members(
    group: &serde_json::Map<String, Value>,
    index: usize,
) -> Result<(), String> {
    let Some(members) = group.get("memberAgentIds").and_then(Value::as_array) else {
        return Err(format!("group {index} memberAgentIds must be an array"));
    };
    if members.len() > 256 {
        return Err(format!("group {index} has too many members"));
    }
    for member in members {
        let Some(id) = member.as_str() else {
            return Err(format!("group {index} has invalid member id"));
        };
        if id.trim().is_empty() || id.len() > 128 {
            return Err(format!("group {index} has invalid member id"));
        }
    }
    Ok(())
}

    fn remote_group_projection_validation_keeps_contract_bounded() {
        let valid = serde_json::json!([{
            "id": "g1",
            "name": "Build",
            "color": "#60a5fa",
            "memberAgentIds": ["claude-a"],
            "leaderAgentId": "claude-a"
        }]);
        assert!(validate_teammate_groups(&valid).is_ok());
        assert!(validate_teammate_groups(&serde_json::json!({})).is_err());
        assert!(validate_teammate_groups(&serde_json::json!([{
            "id": "g1", "name": "Build", "color": "#60a5fa", "memberAgentIds": [1]
        }]))
        .is_err());
    }

