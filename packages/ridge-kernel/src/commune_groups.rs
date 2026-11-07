//! Workspace-scoped, compare-and-swap group authority. Clients cache projections only.
use crate::server::AppState;
use ridge_core::{workspace::file::write_atomic, EventSink};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use uuid::Uuid;
use sha2::{Digest, Sha256};
pub use ridge_mcp::server::HOST_GROUP_SCOPE;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub color: String,
    pub member_agent_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_agent_id: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub revision: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub groups: Vec<Group>,
    #[serde(default)]
    archived: Vec<Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy: Option<Value>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemberSource {
    pub host_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub terminal_epoch: String,
    pub process: ridge_core::process_identity::ProcessInstance,
}
impl MemberSource {
    fn terminal_available(&self, state: &AppState) -> bool {
        if self.host_id != state.host_id { return false; }
        let Ok(pane) = Uuid::parse_str(&self.pane_id) else { return false; };
        let Ok(workspace) = Uuid::parse_str(&self.workspace_id) else { return false; };
        let Ok(info) = state.ptys.info(pane) else { return false; };
        if info.workspace_id != Some(workspace) { return false; }
        let (Some(pid), Some(start_instance)) = (info.child_pid, info.child_start_instance) else { return false; };
        let root = ridge_core::process_identity::ProcessInstance { pid, start_instance };
        ridge_core::process_identity::descendant(&root, &self.process).unwrap_or(false)
            && state.ptys.agent_terminal_view(pane, workspace, 0).is_ok_and(|view| view["terminalEpoch"] == self.terminal_epoch)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Member {
    pub name: String,
    pub source: Option<MemberSource>,
    #[serde(default)]
    pub archived: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    #[serde(default, deserialize_with = "unique_members")]
    members: BTreeMap<String, Member>,
    workspaces: BTreeMap<String, Snapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<Snapshot>,
}
fn unique_members<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Member>, D::Error> {
    struct Members;
    impl<'de> serde::de::Visitor<'de> for Members {
        type Value = BTreeMap<String, Member>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("unique member IDs")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut members = BTreeMap::new();
            while let Some((id, member)) = map.next_entry::<String, Member>()? {
                if id.is_empty() || members.insert(id, member).is_some() {
                    return Err(serde::de::Error::custom(
                        "empty or duplicate organization member ID",
                    ));
                }
            }
            Ok(members)
        }
    }
    deserializer.deserialize_map(Members)
}

pub struct Store {
    path: PathBuf,
    document: Document,
    bytes: Option<Vec<u8>>,
}
impl Store {
    pub(crate) fn members(&self) -> BTreeMap<String, Member> { self.document.members.clone() }
    pub(crate) fn member_for_source(&self, source: &MemberSource) -> Result<Option<String>, String> {
        let matches = self.document.members.iter().filter(|(_, member)| member.source.as_ref() == Some(source))
            .map(|(id, _)| id.clone()).collect::<Vec<_>>();
        if matches.len() > 1 { return Err("ambiguous organization source; explicit recovery required".into()); }
        Ok(matches.into_iter().next())
    }
    pub fn host_authoritative(&self) -> bool { self.document.host.is_some() }
    pub fn member_count(&self) -> usize {
        if let Some(snapshot)=&self.document.host {return snapshot.groups.iter().map(|group|group.member_agent_ids.len()).sum();}
        self.document
            .workspaces
            .values()
            .flat_map(|snapshot| &snapshot.groups)
            .map(|group| group.member_agent_ids.len())
            .sum()
    }
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let document = match &bytes {
            Some(bytes) => serde_json::from_slice::<Document>(bytes).map_err(|error| format!("invalid group authority; preserve {} for recovery: {error}", path.display()))?,
            None => Document { version: 1, members: BTreeMap::new(), workspaces: BTreeMap::new(), host: None },
        };
        if !matches!(document.version, 1 | 2 | 3) {
            return Err("unsupported group authority version".into());
        }
        if document.version < 3 && !document.members.is_empty() { return Err("legacy group authority contains unexpected organization members; preserve for recovery".into()); }
        let sources = document.members.values().filter_map(|member| member.source.as_ref()).collect::<Vec<_>>();
        if sources.iter().enumerate().any(|(index, source)| sources[..index].contains(source)) {
            return Err("duplicate organization source; preserve group authority for recovery".into());
        }
        if document.version == 3 {
            for id in document.workspaces.values().chain(document.host.iter())
                .flat_map(|scope| scope.groups.iter().chain(&scope.archived)).flat_map(|group| &group.member_agent_ids) {
                if !document.members.contains_key(id) { return Err("v3 group references missing organization member; preserve authority for recovery".into()); }
            }
            if let Some(host) = &document.host {
                let active = document.members.iter().filter(|(_, member)| !member.archived).map(|(id, _)| id.clone()).collect();
                validate(&mut host.groups.clone(), &active).map_err(|error| format!("invalid v3 Host groups; preserve authority for recovery: {error}"))?;
            }
        }
        Ok(Self { path, document, bytes })
    }
    pub fn snapshot(&self, workspace: &str) -> Snapshot {
        if workspace == HOST_GROUP_SCOPE { return self.host_snapshot(); }
        self.document
            .workspaces
            .get(workspace)
            .cloned()
            .unwrap_or_default()
    }
    fn host_snapshot(&self) -> Snapshot {
        if let Some(snapshot)=&self.document.host {return snapshot.clone();}
        let mut snapshot=Snapshot::default();
        for (workspace, legacy) in &self.document.workspaces {
            for (original, archived) in legacy.groups.iter().map(|group|(group,false)).chain(legacy.archived.iter().map(|group|(group,true))) {
                let mut group=original.clone();
                if self.document.workspaces.values().filter(|source|source.groups.iter().chain(&source.archived).any(|other|other.id==original.id)).count()>1 {
                    group.id=format!("legacy-{:x}",Sha256::digest(format!("{workspace}\0{}",original.id).as_bytes()));
                }
                if archived {snapshot.archived.push(group);} else {snapshot.groups.push(group);}
            }
        }
        snapshot
    }
    fn replace(
        &mut self,
        workspace: &str,
        expected: u64,
        groups: Vec<Group>,
        members: &HashSet<String>,
        legacy: Option<Value>,
    ) -> Result<Snapshot, String> {
        self.commit(workspace, expected, groups, members, legacy, None)
    }
    fn commit(
        &mut self, workspace: &str, expected: u64, mut groups: Vec<Group>,
        members: &HashSet<String>, legacy: Option<Value>, organization: Option<BTreeMap<String, Member>>,
    ) -> Result<Snapshot, String> {
        if workspace != HOST_GROUP_SCOPE && self.document.host.is_some() {return Err("legacy group scope is read-only; refresh the Host-global groups".into());}
        let mut snapshot = self.snapshot(workspace);
        if snapshot.revision != expected {
            return Err(format!(
                "group revision conflict: expected {expected}, current {}; refresh before editing",
                snapshot.revision
            ));
        }
        if legacy.is_some() && (snapshot.revision != 0 || snapshot.legacy.is_some()) {
            return Err("legacy groups already reconciled".into());
        }
        validate(&mut groups, members)?;
        for group in &mut groups {
            if let Some(old) = snapshot.groups.iter().find(|old| old.id == group.id) {
                // Revision is server-owned; ignore untrusted projection values.
                group.revision = old.revision;
                if group != old {
                    group.revision = old
                        .revision
                        .checked_add(1)
                        .ok_or("group revision exhausted")?;
                }
            } else {
                if snapshot.archived.iter().any(|old| old.id == group.id) {
                    return Err("archived group ID cannot be reused".into());
                }
                group.revision = 1;
            }
        }
        for old in &snapshot.groups {
            if !groups.iter().any(|group| group.id == old.id) {
                snapshot.archived.push(old.clone());
            }
        }
        snapshot.groups = groups;
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or("group revision exhausted")?;
        if legacy.is_some() {
            snapshot.legacy = legacy;
        }
        let current = match std::fs::read(&self.path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("group authority read failed; previous projection retained: {error}")),
        };
        if current != self.bytes { return Err("group authority changed outside Runtime; preserve file and reopen for recovery".into()); }
        let mut next = self.document.clone();
        if let Some(organization) = organization { next.members = organization; }
        if workspace == HOST_GROUP_SCOPE {
            if self.document.version < 3 {
                let original=match std::fs::read(&self.path) {Ok(bytes)=>Some(bytes),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>None,Err(error)=>return Err(format!("group authority read failed; migration aborted: {error}"))};
                if let Some(original)=original {
                    let archive=self.path.with_file_name(format!("commune-groups-v{}-{:x}.archive.json",self.document.version,Sha256::digest(&original)));
                    if !archive.exists() {write_atomic(&archive,&original).map_err(|e|format!("group archive failed; previous authority retained: {e}"))?;}
                    else if std::fs::read(&archive).map_err(|e|e.to_string())?!=original {return Err("group archive integrity mismatch".into());}
                }
            }
            next.version=3;
            for id in next.workspaces.values().chain(std::iter::once(&snapshot))
                .flat_map(|scope| scope.groups.iter().chain(&scope.archived))
                .flat_map(|group| &group.member_agent_ids) {
                next.members.entry(id.clone()).or_insert_with(|| Member { name: "历史成员".into(), source: None, archived: false });
            }
            next.host=Some(snapshot.clone());
        } else {next.workspaces.insert(workspace.into(), snapshot.clone());}
        let bytes = serde_json::to_vec_pretty(&next).map_err(|error| error.to_string())?;
        write_atomic(&self.path, &bytes).map_err(|error| format!("group commit failed; previous projection retained: {error}"))?;
        self.bytes = Some(bytes);
        self.document = next;
        Ok(snapshot)
    }
}
fn validate(groups: &mut [Group], members: &HashSet<String>) -> Result<(), String> {
    if groups.len() > 128 {
        return Err("too many groups".into());
    }
    let mut ids = HashSet::new();
    let mut assigned = HashSet::new();
    for group in groups {
        if group.id.is_empty() || group.id.len() > 128 || !ids.insert(group.id.clone()) {
            return Err("invalid or duplicate group ID".into());
        }
        group.name = group.name.trim().into();
        if group.name.is_empty() || group.name.chars().count() > 128 {
            return Err("invalid group name".into());
        }
        if group.color.len() != 7
            || !group.color.starts_with('#')
            || !group.color[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("group color must be #RRGGBB".into());
        }
        if group.member_agent_ids.len() > 1024 {
            return Err("too many group members".into());
        }
        for member in &group.member_agent_ids {
            if !members.contains(member) {
                return Err(
                    "group member is not an exact registered identity in this scope".into(),
                );
            }
            if !assigned.insert(member.clone()) {
                return Err("a member may belong to only one group".into());
            }
        }
        if group
            .leader_agent_id
            .as_ref()
            .is_some_and(|leader| !group.member_agent_ids.contains(leader))
        {
            return Err("group leader must be an existing group member".into());
        }
    }
    Ok(())
}
pub fn projection(state: &AppState, workspace: &str) -> Result<Value, String> {
    let snapshot = state
        .groups
        .lock()
        .map_err(|_| "group authority unavailable")?
        .snapshot(workspace);
    Ok(json!({"groups":snapshot.groups,"groupsRevision":snapshot.revision}))
}
/// Organization records are durable. Execution facts come exclusively from live protocol identities.
pub(crate) fn member_projection(
    state: &AppState,
    connections: &std::collections::HashMap<
        String,
        std::sync::Arc<crate::commune_connection::Conversation>,
    >,
    native: bool,
) -> Result<Value, String> {
    let (mut members, snapshot) = {
        let store = state
            .groups
            .lock()
            .map_err(|_| "group authority unavailable")?;
        (store.members(), store.snapshot(HOST_GROUP_SCOPE))
    };
    let identities = state
        .roster
        .lock()
        .map_err(|_| "roster unavailable")?
        .agent_identities()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    for identity in identities
        .iter()
        .filter(|identity| identity.protocol_binding.is_some())
    {
        members
            .entry(identity.agent_id.clone())
            .or_insert_with(|| Member {
                name: if identity.executable.is_empty() {
                    format!("Agent · {}", identity.session_id)
                } else {
                    identity.executable.clone()
                },
                source: None,
                archived: false,
            });
    }
    for id in snapshot
        .groups
        .iter()
        .chain(&snapshot.archived)
        .flat_map(|group| &group.member_agent_ids)
    {
        members.entry(id.clone()).or_insert_with(|| Member {
            name: "历史成员".into(),
            source: None,
            archived: false,
        });
    }
    let rights = if native {
        ridge_core::CapabilitySet::allow_all()
    } else {
        ridge_core::CapabilitySet::remote_default()
    };
    let rows = members.iter().map(|(id, member)| {
        let identity = identities.iter().find(|identity| identity.agent_id == *id && identity.protocol_binding.is_some());
        let connection = connections.get(id);
        let connected = !member.archived && identity.is_some_and(|identity| identity.online)
            && connection.is_some_and(|session| session.is_connected());
        let source = member.source.as_ref();
        let workspace = identity.map(|identity| identity.workspace_id.as_str()).or(source.map(|source| source.workspace_id.as_str()));
        let pane = identity.map(|identity| identity.pane_id.as_str()).or(source.map(|source| source.pane_id.as_str()));
        let terminal = source.is_some_and(|source| source.terminal_available(state)) || identity.is_some_and(|identity| identity.protocol_binding.as_ref().is_some_and(|binding| binding.runtime_epoch == state.runtime_epoch
            && ridge_core::process_identity::validate(&binding.process).is_ok()
            && Uuid::parse_str(&identity.pane_id).ok().and_then(|pane| state.ptys.info(pane).ok()).is_some_and(|pane| pane.workspace_id.map(|id| id.to_string()).as_deref() == Some(identity.workspace_id.as_str())
                        && pane.child_pid.zip(pane.child_start_instance).is_some_and(|(pid, start_instance)|
                            ridge_core::process_identity::descendant(&ridge_core::process_identity::ProcessInstance {pid,start_instance}, &binding.process).unwrap_or(false)))));
        let has = |capability: &str| identity.is_some_and(|identity| identity.capabilities.iter().any(|value| value == capability));
        let name = connection.and_then(|session| session.agent_info()).and_then(|info| info["title"].as_str().or(info["name"].as_str()))
            .filter(|name| !name.is_empty()).unwrap_or(if member.name.is_empty() { "历史成员" } else { &member.name });
        let mut row = json!({"agentId":id,"name":name,"archived":member.archived,"workspaceId":workspace,"paneId":pane,
            "terminalAvailable":terminal,"protocolConnected":connected,
            "canSend":connected && rights.is_allowed("send_agent_message") && has("conversation.prompt"),
            "canCancel":connected && rights.is_allowed("cancel_agent_message") && has("conversation.cancel")});
        if let Some(identity) = identity {
            row["generation"]=json!(identity.generation); row["lease"]=json!(identity.lease);
            row["sessionId"]=json!(identity.session_id); row["capabilities"]=json!(identity.capabilities);
        }
        if let Some(source) = source {
            if let Ok(candidates) = state.commune_candidates.lock() {
                if let Some((reference, _)) = candidates.iter().find(|(_, candidate)| candidate.source == *source) { row["candidateRef"] = json!(reference); }
            }
        }
        if native {
            row["source"]=json!(source);
            if let Some(binding) = identity.and_then(|identity| identity.protocol_binding.as_ref()) { row["process"] = json!(binding.process); }
        }
        row
    }).collect::<Vec<_>>();
    Ok(json!(rows))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Enrollment {
    candidate_ref: String,
    group_id: String,
}
fn member_ids(args: &Value, field: &str) -> Result<HashSet<String>, String> {
    let ids: Vec<String> = serde_json::from_value(args.get(field).cloned().unwrap_or(json!([])))
        .map_err(|error| error.to_string())?;
    let unique = ids.iter().cloned().collect::<HashSet<_>>();
    if unique.len() != ids.len() || ids.len() > 1024 {
        return Err(format!("invalid or duplicate {field}"));
    }
    Ok(unique)
}

pub fn replace(
    state: &AppState,
    args: &Value,
    native: bool,
    connected: &HashSet<String>,
) -> Result<Value, String> {
    if args["groupScope"] == "host" {
        if !args["workspaceId"].is_null() || args.get("legacyGroups").is_some() {
            return Err(
                "Host groups do not accept a workspace owner or browser legacy import".into(),
            );
        }
        let enrollments: Vec<Enrollment> =
            serde_json::from_value(args.get("enrollments").cloned().unwrap_or(json!([])))
                .map_err(|e| e.to_string())?;
        if enrollments.len() > 1024 {
            return Err("too many enrollments".into());
        }
        // Resolve outside the storage lock. Discovery never holds it or performs writes.
        let mut candidates = Vec::new();
        let mut references = HashSet::new();
        for enrollment in &enrollments {
            if !references.insert(&enrollment.candidate_ref) {
                return Err("duplicate enrollment".into());
            }
            candidates.push(crate::agent_discovery::resolve(
                state,
                &enrollment.candidate_ref,
            )?);
        }
        let retired = state
            .retired_workspaces
            .lock()
            .map_err(|_| "workspace lifecycle unavailable")?;
        for candidate in &candidates {
            let workspace = Uuid::parse_str(&candidate.source.workspace_id)
                .map_err(|_| "invalid candidate workspace")?;
            if retired.contains(&workspace)
                || state
                    .workspaces
                    .lock()
                    .map_err(|_| "workspace unavailable")?
                    .workspace(workspace)
                    .is_none()
            {
                return Err("candidate workspace closed".into());
            }
        }
        let identities = state
            .roster
            .lock()
            .map_err(|_| "roster unavailable")?
            .agent_identities()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut store = state
            .groups
            .lock()
            .map_err(|_| "group authority unavailable")?;
        let expected = args["expectedRevision"]
            .as_u64()
            .ok_or("group edits require expectedRevision")?;
        if expected != store.snapshot(HOST_GROUP_SCOPE).revision {
            return Err("group revision conflict; refresh before editing".into());
        }
        let mut groups: Vec<Group> =
            serde_json::from_value(args["groups"].clone()).map_err(|e| e.to_string())?;
        let mut organization = store.members();
        for identity in identities
            .iter()
            .filter(|identity| identity.protocol_binding.is_some())
        {
            organization
                .entry(identity.agent_id.clone())
                .or_insert_with(|| Member {
                    name: if identity.executable.is_empty() {
                        format!("Agent · {}", identity.session_id)
                    } else {
                        identity.executable.clone()
                    },
                    source: None,
                    archived: false,
                });
        }
        for id in store
            .snapshot(HOST_GROUP_SCOPE)
            .groups
            .iter()
            .flat_map(|group| &group.member_agent_ids)
        {
            organization.entry(id.clone()).or_insert_with(|| Member {
                name: "历史成员".into(),
                source: None,
                archived: false,
            });
        }
        let archived = member_ids(args, "archiveMemberIds")?;
        let restored = member_ids(args, "restoreMemberIds")?;
        if !archived.is_disjoint(&restored) {
            return Err("member cannot be archived and restored together".into());
        }
        for id in &restored {
            let member = organization
                .get_mut(id)
                .ok_or("unknown organization member")?;
            if !member.archived {
                return Err("member is already active".into());
            }
            member.archived = false;
        }
        for (enrollment, candidate) in enrollments.iter().zip(candidates) {
            let group = groups
                .iter_mut()
                .find(|group| group.id == enrollment.group_id)
                .ok_or("enrollment group missing from submission")?;
            let protocol = identities
                .iter()
                .filter(|identity| {
                    identity.workspace_id == candidate.source.workspace_id
                        && identity.pane_id == candidate.source.pane_id
                        && identity
                            .protocol_binding
                            .as_ref()
                            .is_some_and(|binding| binding.process == candidate.source.process)
                })
                .map(|identity| identity.agent_id.clone())
                .collect::<HashSet<_>>();
            let mut matching = protocol;
            if let Some(id) = store.member_for_source(&candidate.source)? {
                matching.insert(id);
            }
            if matching.len() > 1 {
                return Err("candidate has conflicting member identities".into());
            }
            let id = matching
                .into_iter()
                .next()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            if organization.get(&id).is_some_and(|member| member.archived) {
                return Err("archived member requires explicit restore".into());
            }
            organization.insert(
                id.clone(),
                Member {
                    name: candidate.name,
                    source: Some(candidate.source),
                    archived: false,
                },
            );
            if !group.member_agent_ids.contains(&id) {
                group.member_agent_ids.push(id);
            }
        }
        for id in &archived {
            let member = organization
                .get_mut(id)
                .ok_or("unknown organization member")?;
            if member.archived {
                return Err("member is already archived".into());
            }
            if groups
                .iter()
                .any(|group| group.member_agent_ids.contains(id))
                || connected.contains(id)
                || state.permissions.has_pending(id)?
            {
                return Err("remove member from groups and finish its connection and approvals before archiving".into());
            }
            member.archived = true;
        }
        let members = organization
            .iter()
            .filter(|(_, member)| !member.archived)
            .map(|(id, _)| id.clone())
            .collect();
        for enrollment in &enrollments {
            let candidate = state
                .commune_candidates
                .lock()
                .map_err(|_| "candidate authority unavailable")?
                .get(&enrollment.candidate_ref)
                .cloned()
                .ok_or("candidate expired")?;
            if !candidate.source.terminal_available(state) {
                return Err("candidate process or terminal changed".into());
            }
        }
        let snapshot = store.commit(
            HOST_GROUP_SCOPE,
            expected,
            groups,
            &members,
            None,
            Some(organization),
        )?;
        drop(store);
        drop(retired);
        state.business_events.broadcast(
            "commune-groups-changed",
            json!({"groupScope":"host","revision":snapshot.revision}),
        );
        return Ok(
            json!({"groupScope":"host","groups":snapshot.groups,"groupsRevision":snapshot.revision}),
        );
    }
    if ["enrollments", "archiveMemberIds", "restoreMemberIds"].iter().any(|field| args.get(*field).is_some()) {
        return Err("organization mutations require Host group scope".into());
    }
    let workspace = args["workspaceId"].as_str().ok_or("missing workspaceId")?;
    let id = Uuid::parse_str(workspace).map_err(|_| "invalid workspaceId")?;
    // Keep retirement serialized until persistence; lock order matches workspace close.
    let retired = state
        .retired_workspaces
        .lock()
        .map_err(|_| "workspace lifecycle unavailable")?;
    if retired.contains(&id)
        || state
            .workspaces
            .lock()
            .map_err(|_| "workspace unavailable")?
            .workspace(id)
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
        .map(|member| member.agent_id.clone())
        .collect::<HashSet<_>>();
    let expected = args["expectedRevision"]
        .as_u64()
        .ok_or("group edits require expectedRevision")?;
    let mut archived = None;
    let groups = if let Some(legacy) = args.get("legacyGroups") {
        if !native {
            return Err("legacy group import requires a native trusted client".into());
        }
        let rows = legacy.as_array().ok_or("legacyGroups must be an array")?;
        if rows.len() > 128 {
            return Err("too many legacy groups".into());
        }
        let sidecar_path = crate::registry::ridge_data_dir()
            .join("workspace-memory")
            .join(format!("{id}.json"));
        let sidecar = match std::fs::read(&sidecar_path) {
            Ok(bytes) => Some(
                serde_json::from_slice::<Value>(&bytes)
                    .map_err(|e| format!("legacy memory invalid; preserved for recovery: {e}"))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("legacy memory read failed: {e}")),
        };
        archived = Some(json!({"browserGroups":legacy,"workspaceMemory":sidecar}));
        // Ambiguous or foreign references remain archived, never filled by brand/CWD.
        let mut candidates: Vec<Group> = Vec::new();
        for raw in rows.iter().chain(
            sidecar
                .as_ref()
                .and_then(|doc| doc["teammateGroups"].as_array())
                .into_iter()
                .flatten(),
        ) {
            if let Ok(mut group) = serde_json::from_value::<Group>(raw.clone()) {
                group.revision = 0;
                if validate(std::slice::from_mut(&mut group), &members).is_ok()
                    && !candidates.contains(&group)
                {
                    candidates.push(group);
                }
            }
        }
        let ambiguous_ids = candidates
            .iter()
            .filter(|group| {
                candidates
                    .iter()
                    .filter(|other| other.id == group.id)
                    .count()
                    > 1
            })
            .map(|group| group.id.clone())
            .collect::<HashSet<_>>();
        let mut assignments = BTreeMap::<String, usize>::new();
        for group in &candidates {
            for member in &group.member_agent_ids {
                *assignments.entry(member.clone()).or_default() += 1;
            }
        }
        candidates
            .into_iter()
            .filter(|group| {
                !ambiguous_ids.contains(&group.id)
                    && group
                        .member_agent_ids
                        .iter()
                        .all(|member| assignments[member] == 1)
            })
            .collect()
    } else {
        serde_json::from_value::<Vec<Group>>(args["groups"].clone()).map_err(|e| e.to_string())?
    };
    let snapshot = state
        .groups
        .lock()
        .map_err(|_| "group authority unavailable")?
        .replace(workspace, expected, groups, &members, archived)?;
    drop(retired);
    state.business_events.broadcast(
        "commune-groups-changed",
        json!({"workspaceId":workspace,"revision":snapshot.revision}),
    );
    Ok(
        json!({"groups":snapshot.groups,"groupsRevision":snapshot.revision,
        "legacyArchived":snapshot.legacy.is_some()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn group(id: &str, member: &str) -> Group {
        Group {
            id: id.into(),
            name: id.into(),
            color: "#60a5fa".into(),
            member_agent_ids: vec![member.into()],
            leader_agent_id: Some(member.into()),
            created_at: 1,
            revision: 999,
        }
    }
    #[test]
    fn v1_v2_migrate_once_preserve_bytes_and_all_references() {
        for version in [1, 2] {
            let root = std::env::temp_dir().join(format!("ridge-v3-migration-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join("commune-groups.json");
            let old = json!({"version":version,"workspaces":{"old":{"revision":7,"groups":[],"archived":[group("retired","historical")],"legacy":{"raw":"retained"}}},
                "host":{"revision":9,"groups":[group("host","current")],"archived":[]}});
            let original = serde_json::to_vec_pretty(&old).unwrap();
            std::fs::write(&path, &original).unwrap();
            let mut store = Store::open(path.clone()).unwrap();
            assert_eq!(store.snapshot(HOST_GROUP_SCOPE).revision, 9);
            assert_eq!(std::fs::read(&path).unwrap(), original, "read must not migrate");
            let allowed = HashSet::from(["current".into()]);
            store.replace(HOST_GROUP_SCOPE,9,vec![group("host","current")],&allowed,None).unwrap();
            let archive = root.join(format!("commune-groups-v{version}-{:x}.archive.json",Sha256::digest(&original)));
            assert_eq!(std::fs::read(&archive).unwrap(), original);
            let migrated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(migrated["version"],3);
            assert_eq!(migrated["workspaces"],old["workspaces"]);
            assert_eq!(migrated["members"]["historical"]["source"],Value::Null);
            assert!(migrated["members"]["current"].is_object());
            let mut restarted = Store::open(path.clone()).unwrap();
            restarted.replace(HOST_GROUP_SCOPE,10,vec![group("host","current")],&allowed,None).unwrap();
            assert_eq!(restarted.members().len(),2);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(),2,"second edit must not repeat migration");
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct OldDocument { #[serde(rename="version")] _version:u8, #[serde(rename="workspaces")] _workspaces:Value, #[serde(rename="host")] _host:Value }
            assert!(serde_json::from_value::<OldDocument>(migrated).is_err(),"old Runtime must refuse v3 unknown members");
        }
    }
    #[test]
    fn organization_and_group_commit_fail_together_and_archive_failure_keeps_original() {
        let root = std::env::temp_dir().join(format!("ridge-v3-atomic-{}",Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("commune-groups.json");
        let original = br#"{"version":2,"workspaces":{},"host":{"revision":0,"groups":[]}}"#;
        std::fs::write(&path,original).unwrap();
        let mut store = Store::open(path.clone()).unwrap();
        let mut organization = BTreeMap::new();
        organization.insert("new".into(),Member{name:"Terminal".into(),source:None,archived:false});
        let allowed = HashSet::from(["new".into()]);
        let archive = root.join(format!("commune-groups-v2-{:x}.archive.json",Sha256::digest(original)));
        std::fs::create_dir(&archive).unwrap();
        assert!(store.commit(HOST_GROUP_SCOPE,0,vec![group("g","new")],&allowed,None,Some(organization.clone())).is_err());
        assert!(store.members().is_empty());assert_eq!(store.snapshot(HOST_GROUP_SCOPE).revision,0);
        assert_eq!(std::fs::read(&path).unwrap(),original);
        std::fs::remove_dir(&archive).unwrap();
        store.commit(HOST_GROUP_SCOPE,0,vec![group("g","new")],&allowed,None,Some(organization)).unwrap();
        assert_eq!(store.members().len(),1);
        let confirmed = std::fs::read(&path).unwrap();
        assert!(store.commit(HOST_GROUP_SCOPE,0,vec![],&allowed,None,Some(BTreeMap::new())).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),confirmed);assert_eq!(store.members().len(),1);
    }
    #[test]
    fn external_corruption_during_runtime_is_never_overwritten() {
        let path = std::env::temp_dir().join(format!("ridge-v3-external-{}.json",Uuid::new_v4()));
        let mut store = Store::open(path.clone()).unwrap();
        let members = HashSet::from(["member".into()]);
        store.replace(HOST_GROUP_SCOPE,0,vec![group("g","member")],&members,None).unwrap();
        std::fs::write(&path,b"{broken").unwrap();
        assert!(store.replace(HOST_GROUP_SCOPE,1,vec![],&members,None).unwrap_err().contains("changed outside Runtime"));
        assert_eq!(std::fs::read(&path).unwrap(),b"{broken");assert_eq!(store.snapshot(HOST_GROUP_SCOPE).revision,1);
    }
    #[test]
    fn duplicate_member_ids_and_corruption_are_preserved() {
        let path = std::env::temp_dir().join(format!("ridge-v3-invalid-{}.json",Uuid::new_v4()));
        for bytes in [br#"{"version":3,"workspaces":{},"members":{"same":{"name":"a","source":null},"same":{"name":"b","source":null}}}"#.as_slice(),b"{broken"] {
            std::fs::write(&path,bytes).unwrap();
            assert!(Store::open(path.clone()).is_err());assert_eq!(std::fs::read(&path).unwrap(),bytes);
        }
    }
    #[test]
    fn persisted_revision_fences_clients_and_archives_removed_groups() {
        let path = std::env::temp_dir().join(format!("ridge-groups-{}.json", Uuid::new_v4()));
        let mut store = Store::open(path.clone()).unwrap();
        let members = HashSet::from(["member".into()]);
        let first = store
            .replace(
                "workspace",
                0,
                vec![group("group", "member")],
                &members,
                None,
            )
            .unwrap();
        assert_eq!(first.groups[0].revision, 1);
        assert!(store
            .replace("workspace", 0, vec![], &members, None)
            .unwrap_err()
            .contains("revision conflict"));
        assert_eq!(store.snapshot("other").revision, 0);
        store
            .replace("workspace", 1, vec![], &members, None)
            .unwrap();
        let mut reopened = Store::open(path).unwrap();
        assert_eq!(reopened.snapshot("workspace").archived.len(), 1);
        assert!(reopened
            .replace(
                "workspace",
                2,
                vec![group("group", "member")],
                &members,
                None
            )
            .is_err());
    }
    #[test]
    fn host_migration_retains_archived_ids_and_failed_commit_keeps_legacy_authority() {
        let root=std::env::temp_dir().join(format!("ridge-host-migration-{}",Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();let path=root.join("groups.json");
        let mut store=Store::open(path.clone()).unwrap();let members=HashSet::from(["member".into()]);
        store.replace("workspace",0,vec![group("retired","member")],&members,None).unwrap();
        store.replace("workspace",1,vec![],&members,None).unwrap();
        let original=std::fs::read(&path).unwrap();
        assert!(store.replace(HOST_GROUP_SCOPE,0,vec![group("retired","member")],&members,None).is_err());
        std::fs::rename(&path,root.join("preserved.json")).unwrap();std::fs::create_dir(&path).unwrap();
        assert!(store.replace(HOST_GROUP_SCOPE,0,vec![],&members,None).is_err());
        assert!(!store.host_authoritative());assert_eq!(store.snapshot("workspace").revision,2);
        assert_eq!(std::fs::read(root.join("preserved.json")).unwrap(),original);
    }
    #[test]
    fn foreign_members_duplicate_membership_and_nonmember_leaders_reject() {
        let members = HashSet::from(["member".into()]);
        assert!(validate(&mut [group("foreign", "other")], &members).is_err());
        assert!(validate(&mut [group("a", "member"), group("b", "member")], &members).is_err());
        let mut bad = group("leader", "member");
        bad.leader_agent_id = Some("other".into());
        assert!(validate(&mut [bad], &members).is_err());
    }
    #[test]
    fn failed_commit_keeps_authority_and_disk_recoverable() {
        let root = std::env::temp_dir().join(format!("ridge-groups-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("groups.json");
        let mut store = Store::open(path.clone()).unwrap();
        let members = HashSet::from(["member".into()]);
        store
            .replace(
                "workspace",
                0,
                vec![group("group", "member")],
                &members,
                None,
            )
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        std::fs::rename(&path, root.join("original.json")).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(store
            .replace("workspace", 1, vec![], &members, None)
            .is_err());
        assert_eq!(store.snapshot("workspace").revision, 1);
        assert_eq!(std::fs::read(root.join("original.json")).unwrap(), original);
    }
}
