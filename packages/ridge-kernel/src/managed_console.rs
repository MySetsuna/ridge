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
    #[arg(required_unless_present = "command", conflicts_with = "command")]
    pub profile: Option<String>,
    /// Explicit executable for any compatible ACP Agent or app-server.
    #[arg(long)]
    pub command: Option<PathBuf>,
    #[arg(long, requires="command", value_parser=["acp","app-server"])]
    pub protocol: Option<String>,
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
        if self.profile.is_some() == self.command.is_some() {
            return Err(anyhow!("choose exactly one Agent profile or executable"));
        }
        if self
            .protocol
            .as_deref()
            .is_some_and(|protocol| !matches!(protocol, "acp" | "app-server"))
            || (self.protocol.is_some() && self.command.is_none())
        {
            return Err(anyhow!(
                "explicit protocol requires a compatible executable"
            ));
        }
        if serde_json::to_vec(self)?.len() > 16 * 1024 {
            return Err(anyhow!("collaboration launch configuration exceeds 16 KiB"));
        }
        Ok(())
    }
}

pub async fn run(args: AgentArgs) -> Result<()> {
    args.validate()?;
    let (program, mut arguments, protocol) = if let Some(program) = args.command {
        (
            program,
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
            PathBuf::from(command),
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
        program.display()
    );
    let startup = CollaborationTerminal::start(
        &protocol,
        &program,
        &arguments,
        &cwd,
        Arc::new(|event| {
            let update = &event["update"];
            if let Some(text) = update["content"]["text"].as_str() {
                print!("{text}");
                let _ = std::io::stdout().flush();
            } else if let Some(title) = update["title"].as_str() {
                eprintln!("\n{title}");
            }
            Ok(())
        }),
        Arc::new(move |params| {
            let sender = permission_tx.clone();
            Box::pin(async move {
                let (answer, wait) = oneshot::channel();
                sender.send(Permission { params, answer }).await.ok()?;
                wait.await.ok().flatten()
            })
        }),
        args.auth_method,
    );
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
