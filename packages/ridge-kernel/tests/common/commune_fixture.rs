use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
};
pub fn run() {
    let path = std::env::var_os("RIDGE_COMMUNE_FIXTURE").expect("fixture path");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    fs::write(path,serde_json::to_vec(&json!({"process":ridge_core::process_identity::instance(std::process::id()).unwrap(),"endpoint":listener.local_addr().unwrap().to_string(),"sessionId":"fixture-existing"})).unwrap()).unwrap();
    for accepted in listener.incoming() {
        let mut socket = accepted.unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut pending = Value::Null;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let value: Value = serde_json::from_str(&line).unwrap();
            let result = match value["method"].as_str().unwrap() {
                "initialize" => {
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true},"agentInfo":{"name":std::env::var("RIDGE_COMMUNE_LABEL").unwrap_or_else(|_|"unknown-protocol-process".into()),"version":"fixture"}})
                }
                "session/load" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    writeln!(socket,"{}",json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fixture-existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"existing history"}}}})).unwrap();
                    json!({})
                }
                "session/prompt" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    assert!(pending.is_null(), "one writer per existing conversation");
                    let text = value["params"]["prompt"][0]["text"].as_str().unwrap();
                    if text == "permission-needed" || text == "cancel-permission" {
                        writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":"external-permission","method":"session/request_permission",
                            "params":{"sessionId":"fixture-existing","toolCall":{"toolCallId":"tool","title":"private-operation","rawInput":{"secret":"private-argument"}},
                            "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"always","name":"Allow always","kind":"allow_always"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]}})).unwrap();
                        let mut cancelled = false;
                        let outcome = loop {
                            let mut line = String::new();
                            assert!(reader.read_line(&mut line).unwrap() > 0);
                            let reply: Value = serde_json::from_str(&line).unwrap();
                            if reply["method"] == "session/cancel" {
                                cancelled = true;
                                continue;
                            }
                            assert_eq!(reply["id"], "external-permission");
                            assert!(reply["error"].is_null(), "permission response: {reply}");
                            break reply["result"]["outcome"].clone();
                        };
                        if text == "cancel-permission" {
                            assert_eq!(outcome["outcome"], "cancelled");
                            if !cancelled {
                                let mut line = String::new();
                                reader.read_line(&mut line).unwrap();
                                assert_eq!(
                                    serde_json::from_str::<Value>(&line).unwrap()["method"],
                                    "session/cancel"
                                );
                            }
                        } else {
                            assert_eq!(outcome["optionId"], "once");
                        }
                        let mut log = fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open("permission-decisions")
                            .unwrap();
                        writeln!(log, "{outcome}").unwrap();
                        writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":value["id"],"result":{"stopReason":if text=="cancel-permission" {"cancelled"}else{"end_turn"}}})).unwrap();
                        continue;
                    }
                    let mut log = fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open("prompt-log")
                        .unwrap();
                    writeln!(log, "{text}").unwrap();
                    if text == "disconnect-during-prompt" {
                        break;
                    }
                    if text == "wait-for-cancel" {
                        pending = value["id"].clone();
                        continue;
                    }
                    json!({"stopReason":"end_turn"})
                }
                "session/cancel" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    assert!(!pending.is_null());
                    writeln!(
                        socket,
                        "{}",
                        json!({"jsonrpc":"2.0","id":pending,"result":{"stopReason":"cancelled"}})
                    )
                    .unwrap();
                    pending = Value::Null;
                    continue;
                }
                other => panic!("registration must not call {other}"),
            };
            writeln!(
                socket,
                "{}",
                json!({"jsonrpc":"2.0","id":value["id"],"result":result})
            )
            .unwrap();
        }
    }
}
