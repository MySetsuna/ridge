//! Deterministic, bounded projection of reported protocol content; no filesystem/network reads.
use ridge_mcp::server::McpSessionState;
use serde_json::{json, Value};

fn short(value: &Value, field: &str, chars: usize) -> Option<String> {
    value[field]
        .as_str()
        .map(|value| value.chars().take(chars).collect())
}
fn content(block: &Value, tool: bool) -> Option<Value> {
    match block["type"].as_str()? {
        "text" if tool && block["text"].is_string() => {
            Some(json!({"kind":"tool_output","preview":short(block,"text",1024)}))
        }
        "resource_link" if block["uri"].is_string() && block["name"].is_string() => Some(
            json!({"kind":"resource_link","uri":short(block,"uri",2048),"name":short(block,"name",256),"mimeType":short(block,"mimeType",128)}),
        ),
        "resource"
            if block["resource"]["uri"].is_string()
                && (block["resource"]["text"].is_string()
                    || block["resource"]["blob"].is_string()) =>
        {
            Some(
                json!({"kind":"embedded_resource","uri":short(&block["resource"],"uri",2048),"mimeType":short(&block["resource"],"mimeType",128),"preview":short(&block["resource"],"text",1024)}),
            )
        }
        "image" | "audio" if block["data"].is_string() && block["mimeType"].is_string() => Some(
            json!({"kind":block["type"],"mimeType":short(block,"mimeType",128),"uri":short(block,"uri",2048),"embedded":true}),
        ),
        _ => None,
    }
}
pub fn observe(
    hub: &McpSessionState,
    context: &Value,
    event: &Value,
    length: u64,
) -> Result<(), String> {
    let Some(delivery) = context["deliveryId"].as_str() else {
        return Ok(());
    };
    if event["phase"] != "live" {
        return Ok(());
    }
    let update = &event["params"]["update"];
    let sequence = event["sequence"]
        .as_u64()
        .ok_or("missing observed sequence")?;
    let kind = update["sessionUpdate"].as_str().unwrap_or("");
    let tool = if matches!(kind, "tool_call" | "tool_call_update") {
        update["toolCallId"].as_str()
    } else {
        None
    };
    if tool.is_some_and(|tool| tool.is_empty() || tool.len() > 2048) {
        return Err("protocol tool ID outside projection budget".into());
    }
    let status = update["status"]
        .as_str()
        .filter(|status| matches!(*status, "pending" | "in_progress" | "completed" | "failed"));
    let blocks=match kind {
        "agent_message_chunk" => Some(vec![content(&update["content"],false)]),
        "tool_call" | "tool_call_update" if tool.is_some() => update["content"].as_array().map(|items|items.iter().take(256).map(|item|match item["type"].as_str() {
            Some("content")=>content(&item["content"],true),
            Some("diff") if item["path"].is_string() && item["newText"].is_string()=>Some(json!({"kind":"diff","uri":short(item,"path",2048),"preview":short(item,"newText",1024)})),
            Some("terminal") if item["terminalId"].is_string()=>Some(json!({"kind":"terminal_reference","name":short(item,"terminalId",256)})),
            _=>None,
        }).collect()),
        _=>return Ok(()),
    };
    let binding = &context["to"];
    let cursor = crate::commune_history::event_cursor(
        binding["workspaceId"]
            .as_str()
            .ok_or("missing output workspace")?,
        binding["agentId"].as_str().ok_or("missing output member")?,
        length,
        sequence,
        true,
    )?;
    let descriptors = blocks.map(|blocks| {
        blocks
            .into_iter()
            .enumerate()
            .filter_map(|(index, block)| {
                block.map(|mut descriptor| {
                    descriptor["id"] = json!(format!(
                        "{delivery}:{}:{index}",
                        tool.map(|tool| format!("tool:{}:{tool}", tool.len()))
                            .unwrap_or_else(|| format!("event:{sequence}"))
                    ));
                    descriptor["sequence"] = json!(sequence);
                    descriptor["toolCallId"] = json!(tool);
                    descriptor["textCursor"] = json!(cursor);
                    descriptor["timestamp"] = event["timestamp"].clone();
                    descriptor["source"] =
                        json!("ACP event observed during owned prompt; result not verified");
                    descriptor
                })
            })
            .collect()
    });
    if tool.is_none()
        && descriptors
            .as_ref()
            .is_none_or(|items: &Vec<Value>| items.is_empty())
    {
        return Ok(());
    }
    if descriptors.is_none() && status.is_none() {
        return Ok(());
    }
    hub.record_protocol_outputs(binding, delivery, sequence, tool, status, descriptors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_text_and_unknown_blocks_are_not_deliverables() {
        assert!(content(&json!({"type":"text","text":"task complete"}), false).is_none());
        assert!(content(&json!({"type":"unknown","rawInput":"secret"}), true).is_none());
        assert!(!content(&json!({"type":"resource_link","uri":"file:///output","name":"output","rawInput":"secret"}),true).unwrap().to_string().contains("secret"));
    }
}
