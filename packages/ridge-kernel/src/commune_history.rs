//! Read-only, snapshot-paged Commune protocol and Runtime history.
use crate::{registry::ridge_data_dir, server::AppState};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ridge_core::teammate::communication::AgentIdentity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};
use uuid::Uuid;

const TEXT_CHARS: usize = 4096;
const MAX_RECORD_BYTES: u64 = 24 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bound {
    member_id: String,
    length: u64,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Key(u64, String, u64);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    version: u8,
    workspace: String,
    native: bool,
    query: String,
    bounds: Vec<Bound>,
    after: Option<Key>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextPage {
    version: u8,
    workspace: String,
    native: bool,
    bound: Bound,
    sequence: u64,
    offset: usize,
}
pub(crate) fn event_cursor(workspace: &str, member: &str, length: u64, sequence: u64, native: bool) -> Result<String,String> {
    encode("text1:",&TextPage {version:1,workspace:workspace.into(),native,bound:Bound {member_id:member.into(),length},sequence,offset:0})
}
fn encode<T: Serialize>(prefix: &str, value: &T) -> Result<String, String> {
    Ok(format!(
        "{prefix}{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
fn decode<T: for<'a> Deserialize<'a>>(prefix: &str, value: &str) -> Result<T, String> {
    if value.len() > 128 * 1024 {
        return Err("history cursor too large".into());
    }
    let raw = URL_SAFE_NO_PAD
        .decode(value.strip_prefix(prefix).ok_or("invalid history cursor")?)
        .map_err(|_| "invalid history cursor")?;
    serde_json::from_slice(&raw).map_err(|_| "invalid history cursor".into())
}
fn path(member: &str) -> Result<std::path::PathBuf, String> {
    Uuid::parse_str(member).map_err(|_| "invalid history member ID")?;
    Ok(ridge_data_dir()
        .join("commune-history")
        .join(format!("{member}.jsonl")))
}
fn records(
    path: &Path,
    length: u64,
    mut visit: impl FnMut(u64, Value) -> Result<(), String>,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() < length {
        return Err("history snapshot was truncated; original cursor retained".into());
    }
    let mut reader = BufReader::new(file.take(length));
    let mut ordinal = 0u64;
    let mut previous = 0u64;
    loop {
        let mut bytes = Vec::new();
        let count = reader
            .by_ref()
            .take(MAX_RECORD_BYTES + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if count as u64 > MAX_RECORD_BYTES || bytes.last() != Some(&b'\n') {
            return Err("incomplete or oversized history record; preserved for recovery".into());
        }
        let record: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("invalid history record: {e}"))?;
        ordinal += 1;
        let sequence = record["sequence"].as_u64().unwrap_or(ordinal);
        if sequence <= previous {
            return Err("history sequence conflict; preserved for recovery".into());
        }
        previous = sequence;
        visit(sequence, record)?;
    }
    Ok(())
}
#[derive(Default)]
struct Replay {
    id: Option<String>,
    committed: u64,
    agent: Option<String>,
    items:std::collections::HashMap<String,u64>,
}
fn replay(path: &Path, length: u64) -> Result<Replay, String> {
    let mut latest = Replay::default();
    records(path, length, |sequence, record| {
        if record["kind"] == "replay_committed" {
            latest.id = record["replayId"].as_str().map(str::to_owned);
            latest.committed = sequence;
            latest.agent = record["agentInfo"]["title"]
                .as_str()
                .or(record["agentInfo"]["name"].as_str())
                .map(str::to_owned);
        }
        Ok(())
    })?;
    records(path,length,|sequence,record|{
        if record["params"]["protocol"]=="app-server" && record["params"]["update"]["sessionUpdate"]=="protocol_item"
            && record["params"]["update"]["method"]!="item/started"
            && (if record["phase"]=="replay" {latest.id.as_deref()==record["replayId"].as_str()}else{latest.id.is_none()||sequence>latest.committed}) {
            if let Some(id)=record["params"]["update"]["item"]["id"].as_str(){latest.items.insert(id.to_owned(),sequence);}
        }
        Ok(())
    })?;
    Ok(latest)
}
fn visible(sequence: u64, record: &Value, replay: &Replay) -> bool {
    if record["kind"] == "replay_begin" || record["kind"] == "replay_committed" {
        return false;
    }
    if record["params"]["protocol"]=="app-server" && (record["params"]["wire"]["method"].as_str().is_some_and(|method|method.ends_with("/delta")||method.ends_with("Delta")) || record["params"]["update"]["method"]=="item/started") {return false;}
    if record["params"]["protocol"]=="app-server" && record["params"]["update"]["sessionUpdate"]=="protocol_item" {
        if let Some(id)=record["params"]["update"]["item"]["id"].as_str(){if replay.items.get(id)!=Some(&sequence){return false;}}
    }
    if record["kind"] != "session_update" && record["params"]["update"].is_null() {
        return true;
    }
    if record["phase"] == "replay" {
        return replay.id.as_deref() == record["replayId"].as_str();
    }
    replay.id.is_none() || sequence > replay.committed
}
pub(crate) fn content(record: &Value, native: bool) -> (String, String) {
    let update = &record["params"]["update"];
    if let Some(kind) = update["sessionUpdate"].as_str() {
        if kind=="protocol_item" {
            let item=&update["item"];
            let kind=item["type"].as_str().unwrap_or("protocol_item");
            let text=if kind=="userMessage" {
                item["content"].as_array().map(|content|content.iter().filter_map(|part|part["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
            }else if kind=="agentMessage" {item["text"].as_str().unwrap_or("").to_owned()}
            else if native {item.to_string()}else{json!({"kind":kind,"id":item["id"],"status":item["status"]}).to_string()};
            return (kind.into(),text);
        }
        if matches!(
            kind,
            "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk"
        ) {
            return (
                kind.into(),
                update["content"]["text"]
                    .as_str()
                    .unwrap_or("非文本协议内容")
                    .into(),
            );
        }
        return (
            kind.into(),
            if native {
                update.to_string()
            } else {
                json!({"kind":kind,"toolCallId":update["toolCallId"],"status":update["status"]})
                    .to_string()
            },
        );
    }
    let kind = record["kind"].as_str().unwrap_or("runtime_event");
    let text=match kind {
        "permission_requested" if !native=>"Agent 请求工具权限".into(),
        "execution" if !native=>json!({"deliveryId":record["deliveryId"],"taskId":record["taskId"],"groupId":record["payload"]["groupContext"]["groupId"],"groupRevision":record["payload"]["groupContext"]["groupRevision"],"state":record["execution"]["state"],"stopReason":record["execution"]["response"]["stopReason"]}).to_string(),
        _ if !native=>json!({"kind":kind,"id":record["id"],"agentId":record["agentId"],"sessionId":record["sessionId"],"sequence":record["sequence"],"timestamp":record["timestamp"],"taskId":record["taskId"],"deliveryId":record["deliveryId"],"groupTaskId":record["groupTaskId"]}).to_string(),
        _=>record.to_string(),
    };
    (kind.into(), text)
}
fn row(
    member: &AgentIdentity,
    bound: &Bound,
    sequence: u64,
    record: &Value,
    replay: &Replay,
    native: bool,
    offset: usize,
) -> Result<Value, String> {
    let (kind, text) = content(record, native);
    let count = text.chars().count();
    if offset > count {
        return Err("invalid history text offset".into());
    }
    let fragment = text
        .chars()
        .skip(offset)
        .take(TEXT_CHARS)
        .collect::<String>();
    let position = TextPage {
        version: 1,
        workspace: member.workspace_id.clone(),
        native,
        bound: bound.clone(),
        sequence,
        offset: offset + TEXT_CHARS,
    };
    let next = if offset + TEXT_CHARS < count {
        Some(encode("text1:", &position)?)
    } else {
        None
    };
    let previous = if offset > 0 {
        Some(encode(
            "text1:",
            &TextPage {
                offset: offset.saturating_sub(TEXT_CHARS),
                ..position
            },
        )?)
    } else {
        None
    };
    Ok(
        json!({"id":format!("{}:{sequence}",member.agent_id),"memberId":member.agent_id,"workspaceId":member.workspace_id,
        "sessionId":member.session_id,"sequence":sequence,"agent":replay.agent.as_deref().unwrap_or(&member.agent_id),"cwd":if native {Some(&member.cwd)}else{None},
        "title":kind,"kind":kind,"text":fragment,"timestamp":record["timestamp"].as_u64().unwrap_or(0),"timestampSource":"observed",
        "source":if record["params"]["protocol"]=="app-server" {if record["phase"]=="replay" {"app-server-replay"}else{"app-server-live"}}else if record["phase"]=="replay" {"acp-replay"}else if !record["params"]["update"].is_null() {"acp-live"}else{"ridge-ledger"},
        "coverage":if replay.id.is_some() {"Agent 回放与接入后记录；接入前完整度未独立验证"}else{"自接入起记录；接入前覆盖未知"},
        "textOffset":offset,"textLength":count,"textCursor":next,"previousTextCursor":previous,
        "groupId":record["payload"]["groupContext"]["groupId"],"groupRevision":record["payload"]["groupContext"]["groupRevision"],"groupTaskId":record["groupTaskId"].as_str().or(record["payload"]["groupContext"]["groupTaskId"].as_str()),
        "deliveryId":record["deliveryId"],"taskId":record["taskId"],"correlationId":record["correlationId"],"causationId":record["causationId"]}),
    )
}
pub fn read(state: &AppState, args: &Value, native: bool) -> Result<Value, String> {
    let workspace = args["workspaceId"]
        .as_str()
        .ok_or("history requires workspaceId, not a CWD/provider guess")?;
    let workspace_uuid = Uuid::parse_str(workspace).map_err(|_| "invalid workspaceId")?;
    if state
        .workspaces
        .lock()
        .map_err(|_| "workspace state unavailable")?
        .workspace(workspace_uuid)
        .is_none()
    {
        return Err("workspace not found".into());
    }
    let members = state
        .roster
        .lock()
        .map_err(|_| "roster unavailable")?
        .agent_identities()
        .into_iter()
        .filter(|member| member.workspace_id == workspace && member.protocol_binding.is_some())
        .cloned()
        .collect::<Vec<_>>();
    let member_for = |id: &str| {
        members
            .iter()
            .find(|member| member.agent_id == id)
            .ok_or("history member does not belong to workspace")
    };
    if let Some(cursor) = args["textCursor"].as_str() {
        let cursor: TextPage = decode("text1:", cursor)?;
        if cursor.version != 1 || cursor.workspace != workspace || cursor.native != native {
            return Err("history cursor scope mismatch".into());
        }
        let member = member_for(&cursor.bound.member_id)?;
        let path = path(&member.agent_id)?;
        let replay = replay(&path, cursor.bound.length)?;
        let mut result = None;
        records(&path, cursor.bound.length, |sequence, record| {
            if sequence == cursor.sequence && visible(sequence, &record, &replay) {
                result = Some(row(
                    member,
                    &cursor.bound,
                    sequence,
                    &record,
                    &replay,
                    native,
                    cursor.offset,
                )?);
            }
            Ok(())
        })?;
        return Ok(json!(
            [result.ok_or("history event not found in snapshot")?]
        ));
    }
    if args["offset"].as_u64().is_some_and(|offset| offset > 0) {
        return Err("history paging requires the opaque Runtime cursor".into());
    }
    let query = args["query"].as_str().unwrap_or("").to_lowercase();
    if query.len() > 1024 {
        return Err("history query too large".into());
    }
    let page = if let Some(cursor) = args["cursor"].as_str() {
        let page: Page = decode("page1:", cursor)?;
        if page.version != 1
            || page.workspace != workspace
            || page.native != native
            || page.query != query
        {
            return Err("history cursor scope mismatch".into());
        }
        page
    } else {
        let mut bounds = Vec::new();
        for member in &members {
            if let Some(length) = state
                .permissions
                .history_length(&member.agent_id, &path(&member.agent_id)?)?
            {
                bounds.push(Bound {
                    member_id: member.agent_id.clone(),
                    length,
                });
            }
        }
        Page {
            version: 1,
            workspace: workspace.into(),
            native,
            query: query.clone(),
            bounds,
            after: None,
        }
    };
    if page.bounds.len() > 1024 {
        return Err("history snapshot has too many members".into());
    }
    let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 200) as usize;
    let mut selected = BTreeMap::new();
    for bound in &page.bounds {
        let member = member_for(&bound.member_id)?;
        let path = path(&member.agent_id)?;
        let replay = replay(&path, bound.length)?;
        records(&path, bound.length, |sequence, record| {
            if !visible(sequence, &record, &replay) {
                return Ok(());
            }
            let key = Key(
                record["timestamp"].as_u64().unwrap_or(0),
                member.agent_id.clone(),
                sequence,
            );
            if page.after.as_ref().is_some_and(|after| key >= *after) {
                return Ok(());
            }
            let (kind, text) = content(&record, native);
            if !query.is_empty()
                && !format!(
                    "{} {} {} {} {} {}",
                    member.agent_id,
                    member.session_id,
                    replay.agent.as_deref().unwrap_or(""),
                    kind,
                    text,
                    record["taskId"]
                )
                .to_lowercase()
                .contains(&query)
            {
                return Ok(());
            }
            selected.insert(
                key,
                row(member, bound, sequence, &record, &replay, native, 0)?,
            );
            if selected.len() > limit + 1 {
                selected.pop_first();
            }
            Ok(())
        })?;
    }
    let more = selected.len() > limit;
    if more {
        selected.pop_first();
    }
    let mut rows = selected.into_iter().rev().collect::<Vec<_>>();
    if more {
        if let Some((key, last)) = rows.last_mut() {
            last["cursor"] = json!(encode(
                "page1:",
                &Page {
                    after: Some(key.clone()),
                    ..page
                }
            )?);
        }
    }
    Ok(Value::Array(rows.into_iter().map(|(_, row)| row).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commune_permissions::HistoryLog;
    fn ledger() -> (std::path::PathBuf, HistoryLog) {
        let path = std::env::temp_dir().join(format!("ridge-history-{}.jsonl", Uuid::new_v4()));
        let log = HistoryLog::open(&path).unwrap();
        (path, log)
    }
    #[test]
    fn a_committed_replay_replaces_protocol_rows_but_not_runtime_ledger() {
        let (path, log) = ledger();
        log.append(json!({"kind":"session_update","phase":"replay","replayId":"old","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"old replay"}}}})).unwrap();
        log.append(json!({"kind":"replay_committed","replayId":"old"}))
            .unwrap();
        log.append(json!({"kind":"permission_decided","id":"approval","optionId":"once"}))
            .unwrap();
        log.append(json!({"kind":"session_update","phase":"replay","replayId":"new","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"latest complete replay"}}}})).unwrap();
        log.append(json!({"kind":"replay_committed","replayId":"new"}))
            .unwrap();
        log.append(json!({"kind":"session_update","phase":"replay","replayId":"incomplete","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"must not replace committed projection"}}}})).unwrap();
        let length = std::fs::metadata(&path).unwrap().len();
        let snapshot = replay(&path, length).unwrap();
        let mut rows = Vec::new();
        records(&path, length, |sequence, event| {
            if visible(sequence, &event, &snapshot) {
                rows.push(event);
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["kind"], "permission_decided");
        assert_eq!(
            rows[1]["params"]["update"]["content"]["text"],
            "latest complete replay"
        );
    }
    #[test]
    fn a_read_snapshot_does_not_include_later_appends_or_change_disk() {
        let (path, log) = ledger();
        log.append(json!({"kind":"session_update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"first"}}}})).unwrap();
        let length = std::fs::metadata(&path).unwrap().len();
        log.append(
            json!({"kind":"execution","deliveryId":"newer","execution":{"state":"running"}}),
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut rows = Vec::new();
        records(&path, length, |_, row| {
            rows.push(row);
            Ok(())
        })
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn remote_history_does_not_search_or_show_private_tool_inputs() {
        let requested = json!({"kind":"permission_requested","params":{"toolCall":{"rawInput":{"secret":"private-value"}}}});
        assert!(!content(&requested, false).1.contains("private-value"));
        let tool = json!({"kind":"session_update","params":{"update":{"sessionUpdate":"tool_call","toolCallId":"tool","rawInput":{"secret":"private-value"}}}});
        assert!(!content(&tool, false).1.contains("private-value"));
        assert!(content(&tool, true).1.contains("private-value"));
    }
    #[test]
    fn long_unicode_events_remain_available_as_opaque_text_pages() {
        let (path, log) = ledger();
        let text = "汉🙂".repeat(5000);
        let record=log.append(json!({"kind":"session_update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":text}}}})).unwrap();
        let member = AgentIdentity {
            protocol_binding: None,
            agent_id: Uuid::new_v4().to_string(),
            workspace_id: Uuid::new_v4().to_string(),
            session_id: "same-session".into(),
            pane_id: String::new(),
            cwd: String::new(),
            executable: String::new(),
            argv: vec![],
            generation: 1,
            lease: "lease".into(),
            lifecycle: ridge_core::teammate::communication::AgentLifecycle::Online,
            online: true,
            last_seen_unix_ms: 0,
            capabilities: vec![],
        };
        let bound = Bound {
            member_id: member.agent_id.clone(),
            length: std::fs::metadata(path).unwrap().len(),
        };
        let mut value = row(&member, &bound, 1, &record, &Replay::default(), true, 0).unwrap();
        let mut joined = value["text"].as_str().unwrap().to_owned();
        while let Some(cursor) = value["textCursor"].as_str() {
            let page: TextPage = decode("text1:", cursor).unwrap();
            value = row(
                &member,
                &bound,
                1,
                &record,
                &Replay::default(),
                true,
                page.offset,
            )
            .unwrap();
            joined.push_str(value["text"].as_str().unwrap());
        }
        assert_eq!(joined, text);
        assert!(value["previousTextCursor"].is_string());
    }
}

/// Redacted facts from the durable permission ledger, never inferred execution/risk.
pub(crate) fn audit(state:&AppState,args:&Value)->Result<Value,String> {
    if args.get("limit").is_some_and(|value|!value.is_null() && !value.is_u64()) {return Err("invalid audit limit".into());}
    let limit=args["limit"].as_u64().unwrap_or(20).clamp(1,50) as usize;
    if args.get("workspaceId").is_some_and(|value|!value.is_null() && !value.is_string()) {return Err("invalid workspaceId".into());}
    let workspace=args["workspaceId"].as_str();
    if let Some(workspace)=workspace {
        let id=Uuid::parse_str(workspace).map_err(|_|"invalid workspaceId")?;
        if state.workspaces.lock().map_err(|_|"workspace unavailable")?.workspace(id).is_none() {return Err("workspace not found".into());}
    }
    let members=state.roster.lock().map_err(|_|"roster unavailable")?.agent_identities().into_iter()
        .filter(|member|member.protocol_binding.is_some() && workspace.is_none_or(|workspace|member.workspace_id==workspace)).cloned().collect::<Vec<_>>();
    let mut rows=BTreeMap::new();
    for entry in state.gateway_permissions.audit(workspace)? {
        rows.insert(Key(entry["ts"].as_u64().ok_or("invalid gateway audit timestamp")?,entry["id"].as_str().ok_or("invalid gateway audit ID")?.to_owned(),0),entry);
        if rows.len()>limit {rows.pop_first();}
    }

    if workspace.is_none() {
        for entry in state.execution_rejections.audit()? {
            rows.insert(Key(entry["ts"].as_u64().ok_or("invalid rejection audit timestamp")?,
                entry["id"].as_str().ok_or("invalid rejection audit ID")?.to_owned(), 0), entry);
            if rows.len() > limit { rows.pop_first(); }
        }
    }

    for member in members {
        let file=path(&member.agent_id)?;
        let Some(length)=state.permissions.history_length(&member.agent_id,&file)? else {continue;};
        records(&file,length,|sequence,record| {
            let cancelled=record["kind"]=="permission_cancelled";
            if !cancelled && record["kind"]!="permission_decided" {return Ok(());}
            let verdict=if cancelled {"cancelled"} else {match record["optionKind"].as_str() {
                Some("allow_once"|"allow_always")=>"approve",
                Some("reject_once"|"reject_always")=>"reject",
                _=>"unknown",
            }};
            let timestamp=record["timestamp"].as_u64().ok_or("invalid permission audit timestamp")?;
            let id=record["id"].as_str().ok_or("invalid permission audit ID")?;
            let source=match record["source"].as_str() {Some("local")=>"local",Some("remote")=>"remote",_=>"unknown"};
            rows.insert(Key(timestamp,member.agent_id.clone(),sequence),json!({"id":id,"ts":timestamp,"source":source,
                "initiator":member.agent_id,"verdict":verdict,"riskLevel":"unknown",
                "reasonSummary":"ACP permission request","outcome":if cancelled {"cancelled"} else {"decided"}}));
            if rows.len()>limit {rows.pop_first();}
            Ok(())
        })?;
    }
    Ok(json!({"items":rows.into_values().rev().collect::<Vec<_>>(),"cap":50}))
}
