//! Two owned existing ACP processes, shared session text, real permissions/cancellation.
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;
#[path = "common/commune_fixture.rs"]
mod commune_fixture;
#[test]
#[ignore = "owned existing protocol endpoint"]
fn existing_protocol_endpoint_fixture() {
    commune_fixture::run();
}
struct Kernel {
    endpoint: KernelEndpoint,
    process: Child,
}
impl Kernel {
    fn start(root: &Path) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
            .env("RIDGE_KERNEL_DATA_DIR", root)
            .env("RIDGE_AUTH_FILE", root.join("auth.json"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(process.try_wait().unwrap().is_none());
            if let Ok(raw) = fs::read(root.join("kernel.json")) {
                if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&raw) {
                    if endpoint.pid == process.id() && client::health_ok(&endpoint) {
                        return Self { endpoint, process };
                    }
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Kernel {
    fn drop(&mut self) {
        if client::health_ok(&self.endpoint) {
            client::shutdown_endpoint(&self.endpoint, Duration::from_secs(3)).unwrap();
        }
        let _ = self.process.wait();
    }
}
fn reply(endpoint: &KernelEndpoint, method: &str, args: Value) -> Value {
    client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":method,"args":args,"native":true,"roots":[],"connectionId":Uuid::new_v4()}))).unwrap()
}
fn command(endpoint: &KernelEndpoint, method: &str, args: Value) -> Value {
    let reply = reply(endpoint, method, args);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}
fn attach(endpoint: &KernelEndpoint, workspace: &Value, root: &Path) -> (Value, PathBuf) {
    fs::create_dir_all(root).unwrap();
    let descriptor = root.join("endpoint.json");
    let pane = Uuid::new_v4();
    let created=client::request_json(endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":workspace,
        "program":std::env::current_exe().unwrap(),"args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,
        "env":{"RIDGE_COMMUNE_FIXTURE":descriptor},"role":"shell","cols":80,"rows":24}))).unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let deadline = Instant::now() + Duration::from_secs(8);
    let peer = loop {
        if let Ok(raw) = fs::read(&descriptor) {
            if let Ok(peer) = serde_json::from_slice::<Value>(&raw) {
                break peer;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    let registered = command(
        endpoint,
        "register_commune_member",
        json!({"workspaceId":workspace,"paneId":pane,"process":peer["process"],
        "endpointOwner":peer["process"],"endpoint":peer["endpoint"],"sessionId":peer["sessionId"],"cwd":root}),
    );
    let roster = command(endpoint,"get_teammate_topology",json!({"workspaceId":workspace}));
    let member = roster["roster"].as_array().unwrap().iter().find(|member|member["agentId"]==registered["agentId"]).unwrap().clone();
    assert_eq!(member["paneId"],pane.to_string());
    (member, root.join("prompt-log"))
}
fn target(member: &Value, objective: &str) -> Value {
    json!({"agentId":member["agentId"],"generation":member["generation"],"lease":member["lease"],"objective":objective})
}
fn wait(endpoint: &KernelEndpoint, scope: &Value, predicate: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let status = command(endpoint, "get_agent_message_delivery_status", scope.clone());
        if predicate(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "group state did not settle: {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn frozen_multi_member_dispatch_is_durable_exact_and_never_replayed() {
    let root = std::env::temp_dir().join(format!("ridge-group-task-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let kernel = Kernel::start(&root);
    let endpoint = &kernel.endpoint;
    let workspace = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
        ["workspace_id"]
        .clone();
    let other = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
        ["workspace_id"]
        .clone();
    let (first, first_log) = attach(endpoint, &workspace, &root.join("first"));
    let (second, second_log) = attach(endpoint, &workspace, &root.join("second"));
    assert_ne!(first["agentId"], second["agentId"]);
    let group_id = Uuid::new_v4().to_string();
    let group = json!({"id":group_id,"name":"pair","color":"#60a5fa","memberAgentIds":[first["agentId"],second["agentId"]]});
    let groups = command(
        endpoint,
        "set_teammate_groups",
        json!({"workspaceId":workspace,"expectedRevision":0,"groups":[group]}),
    );
    assert_eq!(groups["groups"][0]["revision"], 1);
    let assigned = json!({"workspaceId":workspace,"groupId":group_id,"groupRevision":1,"groupAction":"assign","idempotency_key":"approved-plan-1",
        "targets":[target(&first,"cancel-permission"),target(&second,"wait-for-cancel")]});
    let accepted = command(endpoint, "send_agent_message", assigned.clone());
    assert_eq!(accepted["children"].as_array().unwrap().len(), 2);
    let scope = json!({"workspaceId":workspace,"groupTaskId":accepted["groupTaskId"]});
    let waiting = wait(endpoint, &scope, |status| {
        status["children"][0]["state"] == "waiting_permission"
            && status["children"][1]["state"] == "running"
    });
    assert_ne!(
        waiting["children"][0]["taskId"],
        waiting["children"][1]["taskId"]
    );
    assert_ne!(waiting["groupTaskId"], waiting["children"][0]["taskId"]);
    assert_eq!(
        waiting["children"][0]["member"]["sessionId"],
        waiting["children"][1]["member"]["sessionId"],
        "same session label must not merge process identities"
    );
    assert_eq!(
        reply(
            endpoint,
            "get_agent_message_delivery_status",
            json!({"workspaceId":other,"groupTaskId":accepted["groupTaskId"]})
        )["ok"],
        false
    );
    let cancel = command(endpoint, "cancel_agent_message", scope.clone());
    assert_eq!(cancel["cancellationRequested"], true);
    wait(endpoint, &scope, |status| {
        status["children"]
            .as_array()
            .unwrap()
            .iter()
            .all(|child| child["state"] == "cancelled")
    });
    let mut broadcast = assigned.clone();
    broadcast["groupAction"] = json!("broadcast");
    broadcast["message"] = json!("broadcast-once");
    broadcast["idempotency_key"] = json!("broadcast-1");
    for member in broadcast["targets"].as_array_mut().unwrap() {
        member.as_object_mut().unwrap().remove("objective");
    }
    let sent = command(endpoint, "send_agent_message", broadcast.clone());
    let broadcast_scope = json!({"workspaceId":workspace,"groupTaskId":sent["groupTaskId"]});
    let ended = wait(endpoint, &broadcast_scope, |status| {
        status["children"]
            .as_array()
            .unwrap()
            .iter()
            .all(|child| child["state"] == "reply_ended")
    });
    assert_eq!(ended["objectiveVerified"], false);
    let mut edited = group.clone();
    edited["memberAgentIds"] = json!([first["agentId"]]);
    command(
        endpoint,
        "set_teammate_groups",
        json!({"workspaceId":workspace,"expectedRevision":1,"groups":[edited]}),
    );
    let replay = command(endpoint, "send_agent_message", broadcast.clone());
    assert_eq!(replay["groupTaskId"], sent["groupTaskId"]);
    assert_eq!(replay["groupRevision"], 1);
    assert_eq!(
        replay["children"], ended["children"],
        "group edit must not change accepted children"
    );
    assert_eq!(fs::read_to_string(&first_log).unwrap(), "broadcast-once\n");
    assert_eq!(
        fs::read_to_string(&second_log).unwrap(),
        "wait-for-cancel\nbroadcast-once\n"
    );
    // A selected stopped member stays in its own failed row; no redirect to a live member.
    let second_pane = client::list_domain_ptys(endpoint)
        .unwrap()
        .into_iter()
        .find(|pty| pty.pty_id.to_string() == second["paneId"].as_str().unwrap())
        .unwrap()
        .pty_id;
    client::destroy_domain_pty(endpoint, second_pane).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let topology = command(
            endpoint,
            "get_teammate_topology",
            json!({"workspaceId":workspace}),
        );
        if topology["roster"]
            .as_array()
            .unwrap()
            .iter()
            .any(|member| member["agentId"] == second["agentId"] && member["online"] == false)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut both = group.clone();
    both["name"] = json!("pair after process exit");
    command(
        endpoint,
        "set_teammate_groups",
        json!({"workspaceId":workspace,"expectedRevision":2,"groups":[both]}),
    );
    let long_objective = "汉🙂".repeat(2500);
    let partial_request = json!({"workspaceId":workspace,"groupId":group_id,"groupRevision":3,"groupAction":"assign","idempotency_key":"partial-1",
        "targets":[target(&first,&long_objective),target(&second,"must-not-redirect")]});
    let partial = command(endpoint, "send_agent_message", partial_request);
    let partial_scope = json!({"workspaceId":workspace,"groupTaskId":partial["groupTaskId"]});
    let partial = wait(endpoint, &partial_scope, |status| {
        status["children"][0]["state"] == "reply_ended"
            && status["children"][1]["state"] == "rejected"
    });
    assert_eq!(
        partial["children"][1]["member"]["agentId"],
        second["agentId"]
    );
    assert_eq!(partial["children"][0]["objectiveTruncated"], true);
    let detail = command(
        endpoint,
        "get_agent_message_delivery_status",
        json!({"workspaceId":workspace,"groupTaskId":partial["groupTaskId"],"objectiveMemberId":first["agentId"]}),
    );
    let cursor = detail["objectiveDetail"]["next"].as_str().unwrap();
    let tail = command(
        endpoint,
        "get_agent_message_delivery_status",
        json!({"workspaceId":workspace,"groupTaskId":partial["groupTaskId"],"objectiveCursor":cursor}),
    );
    assert_eq!(
        format!(
            "{}{}",
            detail["objectiveDetail"]["text"].as_str().unwrap(),
            tail["objectiveDetail"]["text"].as_str().unwrap()
        ),
        long_objective
    );
    assert_eq!(
        reply(
            endpoint,
            "get_agent_message_delivery_status",
            json!({"workspaceId":workspace,"groupTaskId":sent["groupTaskId"],"objectiveCursor":cursor})
        )["ok"],
        false
    );
    let mut first_expected = format!("broadcast-once\n{long_objective}\n");
    assert_eq!(fs::read_to_string(&first_log).unwrap(), first_expected);
    assert_eq!(
        fs::read_to_string(&second_log).unwrap(),
        "wait-for-cancel\nbroadcast-once\n"
    );
    let mut conflict = broadcast.clone();
    conflict["message"] = json!("different-approved-target");
    assert_eq!(reply(endpoint, "send_agent_message", conflict)["ok"], false);
    broadcast["idempotency_key"] = json!("stale-new-parent");
    assert_eq!(
        reply(endpoint, "send_agent_message", broadcast)["ok"],
        false
    );
    let produced=command(endpoint,"send_agent_message",json!({"workspaceId":workspace,"groupId":group_id,"groupRevision":3,"groupAction":"assign","idempotency_key":"outputs-1","targets":[target(&first,"produce-outputs")]}));
    let produced_scope=json!({"workspaceId":workspace,"groupTaskId":produced["groupTaskId"]});
    let output_task=wait(endpoint,&produced_scope,|status|status["children"][0]["state"]=="reply_ended");
    let outputs=output_task["children"][0]["artifacts"].as_array().unwrap();
    assert_eq!(outputs.len(),3,"ordinary text, superseded content and post-response notifications are not current task outputs");
    assert!(outputs.iter().any(|output|output["kind"]=="diff" && output["status"]=="completed"));
    assert!(outputs.iter().all(|output|output["verified"]==false));
    assert!(!outputs.iter().any(|output|output["uri"]=="urn:fixture:obsolete" || output["uri"]=="urn:fixture:late"));
    assert_eq!(fs::read_to_string(root.join("first/result.txt")).unwrap(),"new");
    let source_cursor=outputs.iter().find(|output|output["kind"]=="diff").unwrap()["textCursor"].as_str().unwrap();
    let raw=command(endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"textCursor":source_cursor}));
    assert!(raw[0]["text"].as_str().unwrap().contains("tool_call_update"));
    assert_eq!(raw[0]["taskId"],output_task["children"][0]["taskId"]);
    assert_eq!(raw[0]["groupTaskId"],output_task["groupTaskId"]);
    let remote_reply=client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"get_teammate_topology","args":{"workspaceId":workspace},"native":false,"roots":[],"connectionId":Uuid::new_v4()}))).unwrap();
    assert!(!remote_reply["value"]["groupTasks"].to_string().contains("urn:fixture:"));
    assert!(!remote_reply["value"]["groupTasks"].to_string().contains("private-tool-input"));
    first_expected.push_str("produce-outputs\n");
    let before = fs::read(root.join("commune-groups.json")).unwrap();
    let projection = command(
        endpoint,
        "get_teammate_topology",
        json!({"workspaceId":workspace}),
    );
    assert_eq!(projection["groupTasks"].as_array().unwrap().len(), 4);
    assert_eq!(
        fs::read(root.join("commune-groups.json")).unwrap(),
        before,
        "task queries must not rewrite group definitions"
    );
    drop(kernel);
    let restored = Kernel::start(&root);
    assert!(
        client::list_domain_ptys(&restored.endpoint)
            .unwrap()
            .is_empty(),
        "group records must not spawn replacement Agents on restart"
    );
    let restored_task = command(
        &restored.endpoint,
        "get_agent_message_delivery_status",
        broadcast_scope,
    );
    assert_eq!(restored_task["children"], ended["children"]);
    let restored_outputs=command(&restored.endpoint,"get_agent_message_delivery_status",produced_scope);
    assert_eq!(restored_outputs["children"][0]["artifacts"],output_task["children"][0]["artifacts"]);
    assert_eq!(fs::read_to_string(first_log).unwrap(), first_expected);
    assert_eq!(
        fs::read_to_string(second_log).unwrap(),
        "wait-for-cancel\nbroadcast-once\n"
    );
}
