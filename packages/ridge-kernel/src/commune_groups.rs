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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    workspaces: BTreeMap<String, Snapshot>,
}
pub struct Store {
    path: PathBuf,
    document: Document,
}
impl Store {
    pub fn member_count(&self) -> usize {
        self.document
            .workspaces
            .values()
            .flat_map(|snapshot| &snapshot.groups)
            .map(|group| group.member_agent_ids.len())
            .sum()
    }
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let document = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Document>(&bytes).map_err(|e| {
                format!(
                    "invalid group authority; preserve {} for recovery: {e}",
                    path.display()
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Document {
                version: 1,
                workspaces: BTreeMap::new(),
            },
            Err(e) => return Err(e.to_string()),
        };
        if document.version != 1 {
            return Err("unsupported group authority version".into());
        }
        Ok(Self { path, document })
    }
    pub fn snapshot(&self, workspace: &str) -> Snapshot {
        self.document
            .workspaces
            .get(workspace)
            .cloned()
            .unwrap_or_default()
    }
    fn replace(
        &mut self,
        workspace: &str,
        expected: u64,
        mut groups: Vec<Group>,
        members: &HashSet<String>,
        legacy: Option<Value>,
    ) -> Result<Snapshot, String> {
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
        let mut next = self.document.clone();
        next.workspaces.insert(workspace.into(), snapshot.clone());
        write_atomic(
            &self.path,
            &serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("group commit failed; previous projection retained: {e}"))?;
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
                    "group member is not an exact registered identity in this workspace".into(),
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
pub fn replace(state: &AppState, args: &Value, native: bool) -> Result<Value, String> {
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
