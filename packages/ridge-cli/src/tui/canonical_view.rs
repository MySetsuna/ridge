//! Bounded presentation mirror. Semantic mutation uses ridge-term; no VT feed or PTY owner.
use ridge_term::{term::{Terminal,attrs::{Attrs,ColorKind,Flags}},terminal_protocol::{PaneRef,TerminalBody,TerminalFrame,TerminalInput}};
use std::fmt::Write;

struct HistoryView {terminal:Terminal,start:u64,end:u64,epoch:u64}

pub(crate) struct View {
    pub pane:Option<PaneRef>,
    pub epoch:Option<String>,
    previous_epoch:Option<String>,
    revision:Option<u64>,
    history:Option<ridge_term::terminal_protocol::HistoryRange>,
    terminal:Terminal,
    pub source:String,
    pub next_input:u64,
    pub activation:u64,
    seen_epochs:std::collections::HashMap<PaneRef,String>,
    history_view:Option<HistoryView>,
}
impl Default for View {
    fn default()->Self{Self{pane:None,epoch:None,previous_epoch:None,revision:None,history:None,terminal:Terminal::new(24,80,256),source:uuid::Uuid::new_v4().to_string(),next_input:1,activation:1,seen_epochs:Default::default(),history_view:None}}
}
impl View {
    pub fn select(&mut self,pane:PaneRef){if self.pane.as_ref()!=Some(&pane){let activation=self.activation;let seen_epochs=std::mem::take(&mut self.seen_epochs);*self=Self{pane:Some(pane),activation,seen_epochs,..Self::default()};}}
    pub fn reset_connection(&mut self){let pane=self.pane.clone();let activation=self.activation;let previous_epoch=self.epoch.clone().or_else(||self.previous_epoch.clone());let seen_epochs=std::mem::take(&mut self.seen_epochs);let history_view=self.history_view.take();*self=Self{pane,previous_epoch,activation,seen_epochs,history_view,..Self::default()};}
    pub fn follow_tail(&mut self)->bool {self.history_view.take().is_some()}
    pub fn dimensions(&self)->(usize,usize){(self.terminal.rows(),self.terminal.cols())}
    pub fn history_request(&mut self,back:bool)->Option<ridge_term::terminal_protocol::HistoryRequest>{
        let history=self.history?;
        let rows=self.terminal.rows().min(ridge_term::terminal_protocol::MAX_HISTORY_PAGE_ROWS as usize) as u16;
        let before=if back{self.history_view.as_ref().map_or(history.end,|view|view.start)}else{
            let view=self.history_view.as_ref()?;
            if view.end>=history.end{self.follow_tail();return None;}
            view.end.saturating_add(u64::from(rows)).min(history.end)
        };
        if before<=history.start{return None;}
        Some(ridge_term::terminal_protocol::HistoryRequest{pane:self.pane.clone()?,terminal_epoch:self.epoch.clone()?,activation_id:self.activation,history_epoch:history.epoch,before,max_rows:rows})
    }
    pub fn apply_history(&mut self,request:&ridge_term::terminal_protocol::HistoryRequest,frame:TerminalFrame)->Result<Vec<u8>,&'static str>{
        if self.pane.as_ref()!=Some(&request.pane)||self.epoch.as_ref()!=Some(&request.terminal_epoch)||self.activation!=request.activation_id
            ||frame.pane!=request.pane||frame.terminal_epoch!=request.terminal_epoch||frame.activation_id!=request.activation_id{return Err("TERMINAL_HISTORY_BINDING_CHANGED");}
        let TerminalBody::HistoryPage(page)=frame.body else{return Err("TERMINAL_HISTORY_PAGE_REQUIRED");};
        if page.lines.len()>usize::from(request.max_rows){return Err("HISTORY_REPLY_EXCEEDS_REQUEST");}
        if page.history.epoch!=request.history_epoch{return Err("HISTORY_GENERATION_CHANGED");}
        let mut terminal=Terminal::new(self.terminal.rows(),self.terminal.cols(),ridge_term::terminal_protocol::MAX_HISTORY_PAGE_ROWS as usize);
        if !ridge_term::semantic::prepend_history_page(&mut terminal,self.history,Some(request.before),&page)?{return Err("HISTORY_GENERATION_CHANGED");}
        terminal.scroll_up_view(page.lines.len());
        self.history_view=Some(HistoryView{terminal,start:page.start,end:request.before,epoch:page.history.epoch});
        Ok(self.render().into_bytes())
    }
    pub fn input(&self,bytes:&[u8])->Result<TerminalInput,&'static str>{
        Ok(TerminalInput{pane:self.pane.clone().ok_or("LAN pane not attached")?,terminal_epoch:self.epoch.clone().ok_or("LAN snapshot not received")?,activation_id:self.activation,source_id:self.source.clone(),sequence:self.next_input,bytes:bytes.to_vec()})
    }
    pub fn apply(&mut self,frame:TerminalFrame)->Result<Option<Vec<u8>>,&'static str>{
        if self.pane.as_ref()!=Some(&frame.pane)||frame.activation_id!=self.activation{return Ok(None);}
        let had_history=self.history_view.is_some();
        let recovering=self.epoch.is_none();
        let applied=super::performance::begin();
        match frame.body {
            TerminalBody::Snapshot(snapshot)=>{
                if self.seen_epochs.get(&frame.pane).is_some_and(|epoch|epoch!=&frame.terminal_epoch){return Err("TERMINAL_LIFETIME_CHANGED");}
                if !self.seen_epochs.contains_key(&frame.pane)&&self.seen_epochs.len()>=64{return Err("TERMINAL_VIEW_IDENTITY_LIMIT");}
                if self.epoch.as_ref().or(self.previous_epoch.as_ref()).is_some_and(|epoch|epoch!=&frame.terminal_epoch){return Err("TERMINAL_LIFETIME_CHANGED");}
                if self.revision.is_some_and(|revision|snapshot.revision<=revision){return Ok(None);}
                self.terminal.apply_semantic_snapshot(&snapshot);
                self.seen_epochs.insert(frame.pane,frame.terminal_epoch.clone());
                self.revision=Some(snapshot.revision);self.history=Some(snapshot.history);self.epoch=Some(frame.terminal_epoch);
            }
            TerminalBody::Delta(delta)=>{
                if self.epoch.is_none(){return Err("TERMINAL_SNAPSHOT_REQUIRED");}
                if self.epoch.as_ref()!=Some(&frame.terminal_epoch){return Ok(None);}
                if self.revision.is_some_and(|revision|delta.revision<=revision){return Ok(None);}
                if self.revision!=Some(delta.base_revision){return Err("TERMINAL_REVISION_GAP");}
                let history=self.history.ok_or("TERMINAL_SNAPSHOT_REQUIRED")?;
                if history.epoch!=delta.history.epoch{return Err("HISTORY_GENERATION_CHANGED");}
                ridge_term::semantic::validate_delta(&self.terminal,&delta,history.end)?;
                self.terminal.apply_semantic_delta(&delta);self.revision=Some(delta.revision);self.history=Some(delta.history);
            }
            TerminalBody::Error(_)=>return Err("TERMINAL_PROTOCOL_ERROR"),
            TerminalBody::HistoryPage(_)=>return Ok(None),
        }
        super::performance::finish("semantic_apply",applied,0,0);
        if self.history_view.as_ref().is_some_and(|view|self.history.is_some_and(|history|view.epoch!=history.epoch)||view.terminal.rows()!=self.terminal.rows()||view.terminal.cols()!=self.terminal.cols()){self.history_view=None;}
        if self.history_view.is_some(){return Ok(recovering.then(||self.render().into_bytes()));}
        if had_history{return Ok(Some(format!("\r\n历史代际或尺寸已变化，返回实时输出。\r\n{}",self.render()).into_bytes()));}
        Ok(Some(self.render().into_bytes()))
    }
    pub(crate) fn render(&self)->String{
        let started=super::performance::begin();
        let terminal=self.history_view.as_ref().map_or(&self.terminal,|view|&view.terminal);
        let mut out=String::from("\x1b[?25l\x1b[?7l");let mut attrs=None;
        for row_id in 0..terminal.rows(){
            let Some(row)=terminal.viewport_row(row_id) else{continue;};
            let _=write!(out,"\x1b[{};1H",row_id+1);
            for (col,cell) in row.cells.iter().enumerate(){
                if cell.width==0{continue;}
                let current=terminal.grid().attrs.get(cell.attr);
                if attrs!=Some(current){sgr(&mut out,current);attrs=Some(current);}
                let link=row.hyperlinks.iter().find(|link|link.col_start<=col&&col<link.col_end);
                let link=link.filter(|link|!link.uri.chars().any(char::is_control));
                if let Some(link)=link{let _=write!(out,"\x1b]8;;{}\x1b\\",link.uri);}
                if let Some(cluster)=row.cluster_at(col){out.extend(cluster.text.chars().filter(|ch|!ch.is_control()));}
                else{out.push(if cell.ch.is_control(){' '}else{cell.ch});}
                if link.is_some(){out.push_str("\x1b]8;;\x1b\\");}
            }
        }
        let cursor=terminal.grid().cursor();
        let _=write!(out,"\x1b[0m\x1b[?7h\x1b[{};{}H",cursor.row+1,cursor.col+1);
        let shape=match terminal.modes().cursor_shape{ridge_term::term::modes::CursorShape::Block=>1,ridge_term::term::modes::CursorShape::Underline=>3,ridge_term::term::modes::CursorShape::Bar=>5};
        let _=write!(out,"\x1b[{} q",shape+usize::from(!terminal.modes().cursor_blink));
        if self.history_view.is_none()&&terminal.modes().cursor_visible{out.push_str("\x1b[?25h");}
        super::performance::finish("render",started,out.len(),0);out
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use ridge_term::{producer::TerminalProducer,terminal_protocol::TerminalDeltaFrame};
    #[test]
    fn history_pages_beyond_live_cache_and_keeps_live_output_until_follow_tail(){
        let pane=PaneRef::new("work","pane");
        let mut producer=TerminalProducer::with_epoch(3,16,1000,"lifetime".into());
        for row in 0..600{producer.feed_semantic(format!("ROW_{row:04}\r\n").as_bytes());}
        let mut view=View::default();view.select(pane.clone());view.apply(producer.semantic_snapshot().into_frame(pane.clone(),1)).unwrap();
        let mut pages=0;
        while let Some(request)=view.history_request(true){
            let frame=producer.semantic_history(&request).unwrap().into_frame(pane.clone(),1);
            view.apply_history(&request,frame).unwrap();pages+=1;
            assert!(view.history_view.as_ref().unwrap().terminal.scrollback_len()<=usize::from(ridge_term::terminal_protocol::MAX_HISTORY_PAGE_ROWS));
            assert!(view.terminal.scrollback_len()<=256);assert!(pages<=200);
        }
        assert!(pages>85);assert!(view.render().contains("ROW_0000"));
        let request=view.history_request(false).unwrap();
        view.apply_history(&request,producer.semantic_history(&request).unwrap().into_frame(pane.clone(),1)).unwrap();
        assert!(view.render().contains("ROW_0003"));
        let update=producer.feed_semantic(b"RECENT\r\n");
        assert!(view.apply(update.into_frame(pane,1)).unwrap().is_none());
        assert!(!view.render().contains("RECENT"));
        assert!(view.follow_tail());assert!(view.render().contains("RECENT"));
        assert_eq!(view.input(b"typed").unwrap().terminal_epoch,"lifetime");
    }
    #[test]
    fn history_rejects_stale_binding_generation_and_oversized_reply(){
        let pane=PaneRef::new("work","pane");let mut producer=TerminalProducer::with_epoch(3,16,256,"lifetime".into());
        for row in 0..20{producer.feed_semantic(format!("{row}\r\n").as_bytes());}
        let mut view=View::default();view.select(pane.clone());view.apply(producer.semantic_snapshot().into_frame(pane.clone(),1)).unwrap();
        let request=view.history_request(true).unwrap();let frame=producer.semantic_history(&request).unwrap().into_frame(pane,1);
        let mut stale=frame.clone();stale.activation_id+=1;
        assert_eq!(view.apply_history(&request,stale).unwrap_err(),"TERMINAL_HISTORY_BINDING_CHANGED");
        let mut stale=frame.clone();if let TerminalBody::HistoryPage(page)=&mut stale.body{page.history.epoch+=1;}
        assert_eq!(view.apply_history(&request,stale).unwrap_err(),"HISTORY_GENERATION_CHANGED");
        let mut oversized=frame.clone();if let TerminalBody::HistoryPage(page)=&mut oversized.body{page.lines.push(page.lines[0].clone());}
        assert_eq!(view.apply_history(&request,oversized).unwrap_err(),"HISTORY_REPLY_EXCEEDS_REQUEST");
        assert!(view.history_view.is_none());view.apply_history(&request,frame).unwrap();
        view.reset_connection();assert!(view.input(b"offline").is_err());
        assert!(view.apply(producer.semantic_snapshot().into_frame(request.pane,1)).unwrap().is_some());
        assert!(view.history_view.is_some());
    }
    #[test]
    fn canonical_projection_preserves_cells_and_fences_identity_revision_and_input(){
        let pane=PaneRef::new("work","pane");let mut producer=TerminalProducer::with_epoch(2,8,256,"lifetime".into());
        producer.feed_semantic(b"\x1b[1;31mhi\x1b[0m");
        let frame=producer.semantic_snapshot().into_frame(pane.clone(),1);
        let mut view=View::default();view.select(pane.clone());
        let output=String::from_utf8(view.apply(frame.clone()).unwrap().unwrap()).unwrap();
        assert!(output.contains("hi"));assert!(output.contains(";1"));
        assert!(view.apply(frame.clone()).unwrap().is_none());
        let mut wrong=frame.clone();wrong.pane.workspace_id="other".into();assert!(view.apply(wrong).unwrap().is_none());
        let input=view.input(b"input").unwrap();assert_eq!(input.pane,pane);assert_eq!(input.terminal_epoch,"lifetime");assert_eq!(input.activation_id,1);assert_eq!(input.sequence,1);
        let revision=view.revision.unwrap();let history=view.history.unwrap();
        let gap=TerminalFrame::new(pane,"lifetime",1,TerminalBody::Delta(TerminalDeltaFrame{base_revision:revision+1,revision:revision+2,history,deltas:vec![],hyperlinks:vec![]}));
        assert_eq!(view.apply(gap).unwrap_err(),"TERMINAL_REVISION_GAP");
        let mut replaced=frame;replaced.terminal_epoch="new-lifetime".into();assert_eq!(view.apply(replaced).unwrap_err(),"TERMINAL_LIFETIME_CHANGED");
    }
    #[test]
    fn switching_ignores_old_activation_and_remembers_each_terminal_lifetime(){
        let first=PaneRef::new("workspace","first");let second=PaneRef::new("workspace","second");
        let a=TerminalProducer::with_epoch(2,8,256,"first-lifetime".into());
        let b=TerminalProducer::with_epoch(2,8,256,"second-lifetime".into());
        let mut view=View::default();view.select(first.clone());view.apply(a.semantic_snapshot().into_frame(first.clone(),1)).unwrap();
        view.activation=2;view.reset_connection();view.select(second.clone());
        assert!(view.apply(a.semantic_snapshot().into_frame(first.clone(),1)).unwrap().is_none());
        view.apply(b.semantic_snapshot().into_frame(second,2)).unwrap();
        view.activation=3;view.reset_connection();view.select(first.clone());
        let replaced=TerminalProducer::with_epoch(2,8,256,"replacement".into());
        assert_eq!(view.apply(replaced.semantic_snapshot().into_frame(first.clone(),3)).unwrap_err(),"TERMINAL_LIFETIME_CHANGED");
        view.apply(a.semantic_snapshot().into_frame(first,3)).unwrap();
        assert_eq!(view.input(b"input").unwrap().activation_id,3);
    }
    #[test]
    fn reconnect_requires_snapshot_of_original_terminal_lifetime(){
        let pane=PaneRef::new("work","pane");let producer=TerminalProducer::with_epoch(2,8,256,"original".into());
        let frame=producer.semantic_snapshot().into_frame(pane.clone(),1);
        let mut view=View::default();view.select(pane.clone());view.apply(frame.clone()).unwrap();
        let source=view.source.clone();view.reset_connection();view.select(pane);
        assert!(view.input(b"offline").is_err());assert_ne!(view.source,source);
        let mut replaced=frame.clone();replaced.terminal_epoch="replacement".into();
        assert_eq!(view.apply(replaced).unwrap_err(),"TERMINAL_LIFETIME_CHANGED");
        assert!(view.apply(frame).unwrap().is_some());assert_eq!(view.input(b"online").unwrap().sequence,1);
    }
}
fn sgr(out:&mut String,attrs:Attrs){
    out.push_str("\x1b[0");
    for (flag,code) in [(Flags::BOLD,1),(Flags::DIM,2),(Flags::ITALIC,3),(Flags::UNDERLINE,4),(Flags::BLINK,5),(Flags::INVERSE,7),(Flags::HIDDEN,8),(Flags::STRIKETHROUGH,9),(Flags::DBL_UNDERLINE,21)]{
        if attrs.flags.contains(flag){let _=write!(out,";{code}");}
    }
    for (color,prefix) in [(attrs.fg,38),(attrs.bg,48)]{
        match color.kind(){ColorKind::Default=>{},ColorKind::Indexed(index)=>{let _=write!(out,";{prefix};5;{index}");},ColorKind::Rgb(r,g,b)=>{let _=write!(out,";{prefix};2;{r};{g};{b}");}}
    }
    out.push('m');
}
