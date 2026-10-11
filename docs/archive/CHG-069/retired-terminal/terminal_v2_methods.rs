    pub fn remote_v2_snapshot(
        &self,
        revision: u64,
        title: String,
        cwd: String,
    ) -> crate::terminal_v2::TerminalSnapshot {
        crate::remote_v2::snapshot(&self.grid, &self.modes, revision, title, cwd)
    }

    pub fn apply_remote_v2_snapshot(&mut self, snapshot: &crate::terminal_v2::TerminalSnapshot) {
        self.remote_hyperlinks = snapshot.hyperlinks.iter()
            .map(|link| (link.id, (link.uri.clone(), link.params.clone())))
            .collect();
        crate::remote_v2::install_snapshot(&mut self.grid, &mut self.modes, snapshot);
        self.current_attrs = Attrs::DEFAULT;
        self.scroll_offset = 0;
        self.user_scroll_locked = false;
        self.pending_response.clear();
        self.pending_events.clear();
        if !snapshot.title.is_empty() {
            self.pending_events
                .push(KernelEvent::TitleChanged(snapshot.title.clone()));
        }
        if !snapshot.cwd.is_empty() {
            self.pending_events
                .push(KernelEvent::CwdChanged(snapshot.cwd.clone()));
        }
        self.last_printed = None;
        self.current_link = None;
        self.grapheme_buf.clear();
        self.pending_reset = false;
        self.last_applied_frame_seq = None;
    }

    pub fn apply_remote_v2_delta(&mut self, frame: &crate::terminal_v2::TerminalDeltaFrame) {
        use crate::terminal_v2::{ScreenKind, TerminalDelta};
        for delta in &frame.deltas {
            match delta {
                TerminalDelta::HyperlinkUpsert(link) => {
                    self.update_remote_hyperlink(link.id, Some((link.uri.clone(), link.params.clone())));
                    continue;
                }
                TerminalDelta::HyperlinkRemove { id } => {
                    self.update_remote_hyperlink(*id, None);
                    continue;
                }
                TerminalDelta::Cells { screen, row, col, cells, .. } => {
                    let active = self.grid.is_alt;
                    self.grid.is_alt = *screen == ScreenKind::Alternate;
                    if let Some(local) = crate::remote_v2::delta_to_local(delta) {
                        self.apply_delta(&local);
                    }
                    let target = if self.grid.is_alt { &mut self.grid.alt } else { &mut self.grid.primary };
                    if let Some(target) = target.rows.get_mut(usize::from(*row)) {
                        crate::remote_v2::restore_cell_links(target, usize::from(*col), cells, &self.remote_hyperlinks);
                    }
                    self.grid.is_alt = active;
                    continue;
                }
                TerminalDelta::ScrollbackAppend { lines } => {
                    let cols = self.grid.cols();
                    for line in lines {
                        let row = crate::remote_v2::row_from_wire(line, cols, &mut self.grid.attrs, &self.remote_hyperlinks);
                        let _ = self.grid.scrollback.push(row);
                    }
                    if self.user_scroll_locked && !self.grid.is_alt {
                        self.scroll_offset = self.scroll_offset.saturating_add(lines.len()).min(self.grid.scrollback.len());
                    }
                    continue;
                }
                TerminalDelta::Cursor { screen, .. } | TerminalDelta::Scroll { screen, .. } => {
                    let active = self.grid.is_alt;
                    self.grid.is_alt = *screen == ScreenKind::Alternate;
                    if let Some(local) = crate::remote_v2::delta_to_local(delta) {
                        self.apply_delta(&local);
                    }
                    self.grid.is_alt = active;
                    continue;
                }
                TerminalDelta::SavedCursor { screen, cursor } => {
                    let target = if *screen == ScreenKind::Alternate { &mut self.grid.alt } else { &mut self.grid.primary };
                    target.saved_cursor = cursor.map(|cursor| super::cursor::SavedCursor {
                        row: usize::from(cursor.row), col: usize::from(cursor.col),
                        ..Default::default()
                    });
                    continue;
                }
                TerminalDelta::ScrollRegion { screen, top, bottom } => {
                    let target = if *screen == ScreenKind::Alternate { &mut self.grid.alt } else { &mut self.grid.primary };
                    target.scroll_top = usize::from(*top);
                    target.scroll_bottom = usize::from(*bottom);
                    continue;
                }
                _ => {}
            }
            if let Some(local) = crate::remote_v2::delta_to_local(delta) {
                self.apply_delta(&local);
            }
        }
    }

    pub fn prepend_remote_v2_history(&mut self, lines: &[crate::terminal_v2::WireLine]) {
        crate::remote_v2::prepend_history(&mut self.grid, lines, &self.remote_hyperlinks);
    }

    fn update_remote_hyperlink(&mut self, id: u32, replacement: Option<(String, String)>) {
        let old = match replacement.as_ref() {
            Some(link) => self.remote_hyperlinks.insert(id, link.clone()),
            None => self.remote_hyperlinks.remove(&id),
        };
        let Some(old) = old else { return; };
        let update = |row: &mut super::cell::Row| {
            row.hyperlinks.retain_mut(|span| {
                if span.uri != old.0 || span.id.as_deref().unwrap_or("") != old.1 { return true; }
                let Some((uri, params)) = &replacement else { return false; };
                span.uri.clone_from(uri);
                span.id = (!params.is_empty()).then(|| params.clone());
                true
            });
        };
        for row in self.grid.primary.rows.iter_mut().chain(self.grid.alt.rows.iter_mut()) { update(row); }
        for index in 0..self.grid.scrollback.len() {
            if let Some(row) = self.grid.scrollback.get_mut(index) { update(row); }
        }
    }

