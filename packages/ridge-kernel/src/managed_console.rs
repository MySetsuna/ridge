use crate::managed_agent::CollaborationTerminal;
use anyhow::{anyhow, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot};

#[derive(Args, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentArgs {
    /// Catalog profile, for example codex, claude, grok, or opencode.
    #[arg(required_unless_present_any = ["command", "connect"], conflicts_with_all = ["command", "connect"])]
    pub profile: Option<String>,
    /// Explicit executable for any compatible ACP Agent or app-server.
    #[arg(long, conflicts_with = "connect")]
    pub command: Option<PathBuf>,
    /// Existing local shared daemon. Creates this terminal's own protocol session.
    #[arg(long, requires = "protocol")]
    pub connect: Option<String>,
    #[arg(long, value_parser=["acp","acp-ws","app-server"])]
    pub protocol: Option<String>,
    /// Absolute local file containing the shared WebSocket Bearer credential.
    #[arg(long, requires = "connect")]
    pub bearer_token_file: Option<PathBuf>,
    /// Explicit ACP authentication method advertised by this Agent.
    #[arg(long)]
    pub auth_method: Option<String>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Additional exact arguments. No permission-bypass flags are injected.
    #[arg(last = true)]
    #[serde(default)]
    pub arguments: Vec<String>,
}

struct Permission {
    params: Value,
    answer: oneshot::Sender<Option<String>>,
}

impl AgentArgs {
    pub fn validate(&self) -> Result<()> {
        if [self.profile.is_some(), self.command.is_some(), self.connect.is_some()].into_iter().filter(|present| *present).count() != 1 {
            return Err(anyhow!("choose exactly one Agent profile, executable, or shared endpoint"));
        }
        if self
            .protocol
            .as_deref()
            .is_some_and(|protocol| !matches!(protocol, "acp" | "acp-ws" | "app-server"))
            || (self.protocol.is_some() && self.command.is_none() && self.connect.is_none())
            || (self.protocol.as_deref() == Some("acp-ws") && self.connect.is_none())
        {
            return Err(anyhow!(
                "explicit protocol requires a compatible executable or shared endpoint"
            ));
        }
        if let Some(endpoint) = &self.connect {
            crate::commune_endpoint::LocalEndpoint::try_from(endpoint.clone()).map_err(|error| anyhow!(error))?;
            if self.protocol.is_none() || !self.arguments.is_empty() {
                return Err(anyhow!("shared endpoints require an explicit protocol and do not accept process arguments"));
            }
        }
        if let Some(path) = &self.bearer_token_file {
            if self.connect.is_none() || self.protocol.as_deref() == Some("acp") || !crate::commune_endpoint::local_path(path) {
                return Err(anyhow!("Bearer credentials require a shared WebSocket and an absolute local file"));
            }
        }
        if self.auth_method.is_some() && self.protocol.as_deref() == Some("app-server") {
            return Err(anyhow!("authentication method selection requires ACP"));
        }
        if serde_json::to_vec(self)?.len() > 16 * 1024 {
            return Err(anyhow!("collaboration launch configuration exceeds 16 KiB"));
        }
        Ok(())
    }
}

pub async fn run(args: AgentArgs) -> Result<()> {
    args.validate()?;
    let (program, mut arguments, protocol) = if args.connect.is_some() {
        (None, Vec::new(), args.protocol.expect("validated shared protocol"))
    } else if let Some(program) = args.command {
        (
            Some(program),
            Vec::new(),
            args.protocol.unwrap_or_else(|| "acp".into()),
        )
    } else {
        let catalog = crate::agent_profiles::list_profiles().map_err(|error| anyhow!(error))?;
        let profile = catalog
            .as_array()
            .and_then(|profiles| {
                profiles
                    .iter()
                    .find(|profile| profile["id"].as_str() == args.profile.as_deref())
            })
            .ok_or_else(|| anyhow!("unknown Agent profile"))?;
        let recipe = &profile["collaboration"];
        let command=recipe["command"].as_str().ok_or_else(||anyhow!("profile has no collaboration recipe; use --command with a compatible ACP executable"))?;
        let arguments = recipe["args"]
            .as_array()
            .ok_or_else(|| anyhow!("invalid collaboration arguments"))?
            .iter()
            .map(|argument| {
                argument
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("invalid collaboration argument"))
            })
            .collect::<Result<Vec<_>>>()?;
        (
            Some(PathBuf::from(command)),
            arguments,
            recipe["protocol"]
                .as_str()
                .ok_or_else(|| anyhow!("collaboration protocol required"))?
                .to_owned(),
        )
    };
    arguments.extend(args.arguments);
    let cwd = args
        .cwd
        .unwrap_or(std::env::current_dir()?)
        .canonicalize()
        .context("collaboration working directory unavailable")?;
    let (permission_tx, mut permissions) = mpsc::channel::<Permission>(1);
    eprintln!(
        "Starting a new collaboration session with {}.",
        args.connect.as_deref().map(str::to_owned).unwrap_or_else(|| program.as_ref().expect("owned program").display().to_string())
    );
    let updates: ridge_acp::existing::UpdateSink = Arc::new(|event| {
            let update = &event["update"];
            if let Some(text) = update["content"]["text"].as_str() {
                print!("{text}");
                let _ = std::io::stdout().flush();
            } else if let Some(title) = update["title"].as_str() {
                eprintln!("\n{title}");
            }
            Ok(())
        });
    let permission: ridge_acp::existing::PermissionHandler = Arc::new(move |params| {
            let sender = permission_tx.clone();
            Box::pin(async move {
                let (answer, wait) = oneshot::channel();
                sender.send(Permission { params, answer }).await.ok()?;
                wait.await.ok().flatten()
            })
        });
    let startup = async {
        if let Some(endpoint) = &args.connect {
            CollaborationTerminal::connect_shared(&protocol, endpoint, args.bearer_token_file.as_deref(), &cwd, updates, permission, args.auth_method).await
        } else {
            CollaborationTerminal::start(&protocol, program.as_ref().expect("owned program"), &arguments, &cwd, updates, permission, args.auth_method).await
        }
    };
    let terminal = tokio::select! {
        result = startup => result.map_err(|error| anyhow!(error))?,
        _ = tokio::signal::ctrl_c() => {
            eprintln!("Startup cancelled. Check the Agent’s history before creating another session.");
            return Ok(());
        }
    };
    let session = terminal.session();
    if let Some(info) = session.agent_info() {
        eprintln!(
            "Agent {} {}",
            info["name"].as_str().unwrap_or("unknown"),
            info["version"].as_str().unwrap_or("version unavailable")
        );
    }
    eprintln!("Session {}\nOpen Agent's Commune in this terminal's workspace to connect.\n/approve <optionId> · /reject · /cancel · /exit",session.session_id());
    let (input, mut lines) = mpsc::channel::<std::io::Result<String>>(8);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if input.blocking_send(line).is_err() {
                break;
            }
        }
    });
    let mut pending: Option<Permission> = None;
    let mut turn = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            line=lines.recv()=>{
                let Some(line)=line else {break;};
                let line=line?;
                match line.trim() {
                    "/exit"=>break,
                    "/cancel"=>{if let Err(error)=session.cancel().await{eprintln!("{error}");}}
                    "/reject"=>{if let Some(permission)=pending.take(){let _=permission.answer.send(None);}}
                    command if command.starts_with("/approve ")=>{
                        let option=command.trim_start_matches("/approve ").trim();
                        let offered = pending.as_ref().is_some_and(|permission| {
                            permission.params["options"].as_array().is_some_and(|options| {
                                options.iter().any(|offered| offered["optionId"] == option)
                            })
                        });
                        if offered {
                            if let Some(permission) = pending.take() {
                                let _ = permission.answer.send(Some(option.to_owned()));
                            }
                        } else {
                            eprintln!("Choose an optionId from the pending Agent request.");
                        }
                    }
                    ""=>{}
                    _=>match session.prompt(line) {
                        Ok(response)=>{turn.spawn(async move {
                            match response.await {
                                Ok(Ok(result)) => eprintln!("\n{}",result["stopReason"]),
                                Ok(Err(error)) => eprintln!("\n{error}"),
                                Err(_) => eprintln!("\nResult unknown. No turn was resent."),
                            }
                        });}
                        Err(error)=>eprintln!("{error}"),
                    }
                }
            }
            Some(permission)=permissions.recv()=>{
                eprintln!("\nApproval required\n{}",serde_json::to_string_pretty(&permission.params)?);
                pending=Some(permission);
            }
            Some(_)=turn.join_next()=>{}
            _=tokio::signal::ctrl_c()=>break,
            _=tokio::time::sleep(std::time::Duration::from_millis(250))=>{if !session.is_connected(){eprintln!("Agent disconnected. Execution outcome requires confirmation.");break;}}
        }
    }
    drop(pending);
    drop(terminal);
    Ok(())
}
