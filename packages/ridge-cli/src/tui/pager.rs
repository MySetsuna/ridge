//! Pager presentation over the shared Runtime Host. No local PTY/parser/business graph.
use std::{io::Write,sync::{Arc,atomic::{AtomicBool,Ordering}}};
use anyhow::{anyhow,Context,Result};
use crossterm::{execute,terminal,event::{Event,EventStream,KeyCode,KeyEventKind,KeyModifiers}};
use futures_util::StreamExt;
use ridge_term::terminal_protocol::PaneRef;
use tokio::sync::Notify;
use super::{keymap,lan_session,Session};

const STATUS_LINES:u16=2;

fn leaves(layout:&serde_json::Value,workspace:&str,panes:&mut Vec<PaneRef>)->Result<()>{
    if layout["type"]=="leaf"{
        let id=layout["id"].as_str().ok_or_else(||anyhow!("Runtime layout omitted pane identity"))?;
        panes.push(PaneRef::new(workspace,id));
    }else{
        let children=layout["children"].as_array().ok_or_else(||anyhow!("Runtime layout omitted children"))?;
        for child in children{leaves(child,workspace,panes)?;}
    }
    Ok(())
}

fn workspace_label(entry:&serde_json::Value,index:usize)->String{
    entry["name"].as_str().filter(|name|!name.trim().is_empty()).map(str::to_owned).unwrap_or_else(||format!("WS:{}",index+1))
}

async fn select_workspace(session:&lan_session::LanControllerSession,number:u8)->Result<Option<(Vec<PaneRef>,String)>>{
    let catalog=session.call("list_workspaces",serde_json::json!({})).await?;
    let Some(entry)=catalog.as_array().and_then(|items|items.get(usize::from(number).saturating_sub(1)))else{return Ok(None);};
    let workspace=entry["id"].as_str().ok_or_else(||anyhow!("Runtime workspace identity missing"))?;
    let layout=session.call("get_pane_layout_for",serde_json::json!({"workspaceId":workspace})).await?;
    let mut panes=Vec::new();leaves(&layout,workspace,&mut panes)?;
    if panes.is_empty(){return Err(anyhow!("Runtime workspace has no panes"));}
    Ok(Some((panes,workspace_label(entry,usize::from(number).saturating_sub(1)))))
}

fn chrome(stdout:&mut impl Write,panes:&[PaneRef],active:usize,cols:u16,workspace:&str)->Result<()>{
    write!(stdout,"\x1b[?6l\x1b[1;1H\x1b[2K")?;
    let bar=format!("{workspace} ")+&panes.iter().enumerate().map(|(index,_)|if index==active{format!(" [{}]*shell* ",index+1)}else{format!(" [{}]shell ",index+1)}).collect::<String>();
    let hint="Ctrl+Shift+方向 切换 pane | Ctrl+F 工作区 | Ctrl+Shift+PgUp/PgDn 历史";
    write!(stdout,"{}\x1b[2;1H\x1b[2K{}\x1b[?6h",bar.chars().filter(|character|!character.is_control()).take(cols as usize).collect::<String>(),hint.chars().take(cols as usize).collect::<String>())?;
    Ok(())
}

pub async fn run_pager(shell:Option<String>,cwd:Option<String>,count:usize)->Result<()>{
    let cwd=Some(lan_session::local_cwd(cwd.as_deref())?);
    let (session,mut output)=lan_session::connect_local(shell.clone(),cwd.clone()).await?;
    let first=session.pane_ref().ok_or_else(||anyhow!("Runtime terminal identity missing"))?;
    let notify=Arc::new(Notify::new());let closed=Arc::new(AtomicBool::new(false));
    let (signal,ended)=(notify.clone(),closed.clone());
    let pump=tokio::spawn(async move{
        while output.recv().await.is_some(){signal.notify_one();}
        ended.store(true,Ordering::Release);signal.notify_one();
    });
    for _ in 1..count{
        session.call("split_pane",serde_json::json!({"workspaceId":first.workspace_id,"paneId":first.pane_id,"direction":"horizontal","shell":shell,"cwd":cwd})).await?;
    }
    let layout=session.call("get_pane_layout_for",serde_json::json!({"workspaceId":first.workspace_id})).await?;
    let mut panes=Vec::new();leaves(&layout,&first.workspace_id,&mut panes)?;
    if panes.is_empty(){return Err(anyhow!("Runtime workspace has no panes"));}
    let mut active=panes.iter().position(|pane|pane==&first).unwrap_or(0);
    let catalog=session.call("list_workspaces",serde_json::json!({})).await?;
    let (index,entry)=catalog.as_array().and_then(|entries|entries.iter().enumerate().find(|(_,entry)|entry["id"]==first.workspace_id)).ok_or_else(||anyhow!("Runtime workspace disappeared"))?;
    let mut workspace=workspace_label(entry,index);
    let mut stdout=std::io::stdout();let (mut cols,rows)=terminal::size()?;
    session.resize(cols,rows.saturating_sub(STATUS_LINES).max(1))?;
    terminal::enable_raw_mode().context("进入 raw mode 失败")?;
    execute!(stdout,terminal::EnterAlternateScreen)?;
    let result=async {
        write!(stdout,"\x1b[{};{}r\x1b[?6h",STATUS_LINES+1,rows)?;
        chrome(&mut stdout,&panes,active,cols,&workspace)?;
        if let Some(bytes)=session.presentation(){super::write_presentation(&mut stdout,&bytes)?;}stdout.flush()?;
        let mut events=EventStream::new();
        'ui: loop{tokio::select!{
            event=events.next()=>match event{
                Some(Ok(Event::Key(key)))=>{
                    if matches!(key.kind,KeyEventKind::Release){continue;}
                    if key.modifiers.contains(KeyModifiers::CONTROL)&&key.code==KeyCode::Char(']'){break Ok(());}
                    if let Some(back)=super::history_direction(&key){
                        let result=super::wait_control(session.scroll_history(back),&mut events,"正在加载历史",|width,height|{
                            cols=width;session.resize(width,height.saturating_sub(STATUS_LINES).max(1))?;
                            write!(stdout,"\x1b[{};{}r",STATUS_LINES+1,height)?;chrome(&mut stdout,&panes,active,cols,&workspace)?;stdout.flush()?;Ok(())
                        }).await;
                        match result{
                            Ok(Some(Some(bytes)))=>super::write_presentation(&mut stdout,&bytes)?,
                            Ok(None)=>break Ok(()),
                            Ok(Some(None))=>{},
                            Err(error)=>{write!(stdout,"\r\n历史不可用：{error}\r\n")?;stdout.flush()?;},
                        }
                        continue;
                    }
                    if keymap::is_control_shortcut(&key){
                        if let KeyCode::F(number @ 1..=12)=key.code{
                            let selected=super::wait_control(select_workspace(&session,number),&mut events,"正在切换工作区",|width,height|{
                                cols=width;session.resize(width,height.saturating_sub(STATUS_LINES).max(1))?;
                                write!(stdout,"\x1b[{};{}r",STATUS_LINES+1,height)?;chrome(&mut stdout,&panes,active,cols,&workspace)?;stdout.flush()?;Ok(())
                            }).await;
                            match selected{
                                Ok(Some(Some((next,label))))=>{
                                    session.activate(next[0].clone())?;panes=next;active=0;workspace=label;
                                    write!(stdout,"\x1b[1;1H\x1b[J")?;
                                    chrome(&mut stdout,&panes,active,cols,&workspace)?;stdout.flush()?;
                                }
                                Ok(None)=>break 'ui Ok(()),
                                Ok(Some(None))=>{},
                                Err(error)=>{write!(stdout,"\r\n工作区不可用：{error}\r\n")?;stdout.flush()?;},
                            }
                            continue;
                        }
                        let next=match key.code{
                            KeyCode::Right=>(active+1).min(panes.len()-1),
                            KeyCode::Left=>active.saturating_sub(1),
                            _=>active,
                        };
                        if next!=active{
                            session.activate(panes[next].clone())?;active=next;
                            write!(stdout,"\x1b[1;1H\x1b[J")?;
                            chrome(&mut stdout,&panes,active,cols,&workspace)?;stdout.flush()?;
                        }
                        continue;
                    }
                    if let Some(bytes)=keymap::encode_key(&key){
                        if session.input_ready(){if let Some(bytes)=session.follow_tail(){super::write_presentation(&mut stdout,&bytes)?;}session.send_input(&bytes)?;}
                        else{write!(stdout,"\r\n会话尚未接通，按键未发送。\r\n")?;stdout.flush()?;}
                    }
                }
                Some(Ok(Event::Resize(width,height)))=>{
                    cols=width;session.resize(width,height.saturating_sub(STATUS_LINES).max(1))?;
                    write!(stdout,"\x1b[{};{}r",STATUS_LINES+1,height)?;
                    chrome(&mut stdout,&panes,active,cols,&workspace)?;stdout.flush()?;
                }
                Some(Err(error))=>break Err(error.into()),
                None=>break Ok(()),
                _=>{},
            },
            _=notify.notified()=>{
                if let Some(error)=session.failure(){break Err(anyhow!(error));}
                if closed.load(Ordering::Acquire){break Ok(());}
                // Always repaint the current canonical view, never a queued frame from the old activation.
                if let Some(bytes)=session.presentation(){super::write_presentation(&mut stdout,&bytes)?;}
            }
        }}
    }.await;
    pump.abort();
    let _=write!(stdout,"\x1b[r\x1b[?6l");
    let _=execute!(stdout,terminal::LeaveAlternateScreen);
    let _=terminal::disable_raw_mode();
    result
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn workspace_chrome_uses_runtime_names_without_interpreting_control_sequences(){
        assert_eq!(workspace_label(&serde_json::json!({"name":null}),1),"WS:2");
        let label=workspace_label(&serde_json::json!({"name":"\u{1b}]52;c;untrusted\u{7}\nWorkspace"}),0);
        let mut output=Vec::new();
        chrome(&mut output,&[PaneRef::new("workspace","pane")],0,120,&label).unwrap();
        let output=String::from_utf8(output).unwrap();
        assert!(!output.contains("\u{1b}]52"));assert!(!output.contains('\u{7}'));assert!(!output.contains('\n'));
        assert!(output.contains("Workspace"));
    }
}
