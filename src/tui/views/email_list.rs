use crate::backend::{BackendCommand, BackendResponse};
use crate::compose;
use crate::jmap::types::{Email, Mailbox};
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::email_view::EmailView;
use crate::tui::views::help::HelpView;
use crate::tui::views::{View, ViewAction};
use std::io;
use std::sync::mpsc;

pub struct EmailListView {
    cmd_tx: mpsc::Sender<BackendCommand>,
    from_address: String,
    mailbox_id: String,
    mailbox_name: String,
    page_size: u32,
    emails: Vec<Email>,
    cursor: usize,
    total: Option<u32>,
    loading: bool,
    error: Option<String>,
    pending_click: bool,
    mailboxes: Vec<Mailbox>,
    move_mode: bool,
    move_cursor: usize,
    search_mode: bool,
    search_input: String,
    active_search: Option<String>,
}

impl EmailListView {
    pub fn new(
        cmd_tx: mpsc::Sender<BackendCommand>,
        from_address: String,
        mailbox_id: String,
        mailbox_name: String,
        page_size: u32,
        mailboxes: Vec<Mailbox>,
    ) -> Self {
        EmailListView {
            cmd_tx,
            from_address,
            mailbox_id,
            mailbox_name,
            page_size,
            emails: Vec::new(),
            cursor: 0,
            total: None,
            loading: true,
            error: None,
            pending_click: false,
            mailboxes,
            move_mode: false,
            move_cursor: 0,
            search_mode: false,
            search_input: String::new(),
            active_search: None,
        }
    }

    fn request_refresh(&mut self) {
        self.loading = true;
        let _ = self.cmd_tx.send(BackendCommand::QueryEmails {
            mailbox_id: self.mailbox_id.clone(),
            page_size: self.page_size,
            search_query: self.active_search.clone(),
        });
    }

    fn is_unread(email: &Email) -> bool {
        !email.keywords.contains_key("$seen")
    }

    fn is_flagged(email: &Email) -> bool {
        email.keywords.contains_key("$flagged")
    }

    fn format_email(email: &Email, width: u16) -> String {
        let unread = if Self::is_unread(email) { "N" } else { " " };
        let flagged = if Self::is_flagged(email) { "F" } else { " " };

        let from = email
            .from
            .as_ref()
            .and_then(|addrs| addrs.first())
            .map(|a| {
                a.name
                    .as_deref()
                    .unwrap_or_else(|| a.email.as_deref().unwrap_or("(unknown)"))
            })
            .unwrap_or("(unknown)");

        let subject = email.subject.as_deref().unwrap_or("(no subject)");

        let date = email
            .received_at
            .as_deref()
            .map(|d| {
                if d.len() >= 10 {
                    &d[..10]
                } else {
                    d
                }
            })
            .unwrap_or("");

        let w = width as usize;
        let from_width = 20.min(w.saturating_sub(19));
        let subj_width = w.saturating_sub(19 + from_width);

        let from_display = truncate(from, from_width);
        let subj_display = truncate(subject, subj_width);

        format!(
            " {}{} {} {:from_w$} {}",
            unread,
            flagged,
            date,
            from_display,
            subj_display,
            from_w = from_width
        )
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s.chars().take(max).collect()
    } else {
        let mut end = max - 3;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

impl View for EmailListView {
    fn render(&self, term: &mut Terminal) -> io::Result<()> {
        term.clear()?;

        // Header
        term.move_to(1, 1)?;
        term.set_bold()?;
        let header = if let Some(ref query) = self.active_search {
            match self.total {
                Some(total) => format!("{} [search: {}] ({} results)", self.mailbox_name, query, total),
                None => format!("{} [search: {}]", self.mailbox_name, query),
            }
        } else {
            match self.total {
                Some(total) => format!("{} ({} messages)", self.mailbox_name, total),
                None => self.mailbox_name.clone(),
            }
        };
        term.write_truncated(&header, term.cols)?;
        term.reset_attr()?;

        // Separator
        term.move_to(2, 1)?;
        let sep = "-".repeat(term.cols as usize);
        term.write_str(&sep)?;

        if self.move_mode {
            // Render mailbox picker
            term.move_to(3, 1)?;
            term.set_bold()?;
            term.write_truncated("Move to mailbox:", term.cols)?;
            term.reset_attr()?;

            let max_items = (term.rows as usize).saturating_sub(5);
            let scroll_offset = if self.move_cursor >= max_items {
                self.move_cursor - max_items + 1
            } else {
                0
            };

            for (i, mailbox) in self
                .mailboxes
                .iter()
                .skip(scroll_offset)
                .enumerate()
                .take(max_items)
            {
                let row = 4 + i as u16;
                term.move_to(row, 1)?;

                let display_idx = scroll_offset + i;
                let line = format!("  {}", mailbox.name);

                if display_idx == self.move_cursor {
                    term.set_reverse()?;
                }

                term.write_truncated(&line, term.cols)?;
                term.reset_attr()?;
            }
        } else if self.loading && self.emails.is_empty() {
            term.move_to(3, 1)?;
            term.write_truncated("Loading emails...", term.cols)?;
        } else if let Some(ref err) = self.error {
            term.move_to(3, 1)?;
            term.write_truncated(err, term.cols)?;
        } else if self.emails.is_empty() {
            term.move_to(3, 1)?;
            term.write_truncated("No messages.", term.cols)?;
        } else {
            let max_items = (term.rows as usize).saturating_sub(4);
            let scroll_offset = if self.cursor >= max_items {
                self.cursor - max_items + 1
            } else {
                0
            };

            for (i, email) in self
                .emails
                .iter()
                .skip(scroll_offset)
                .enumerate()
                .take(max_items)
            {
                let row = 3 + i as u16;
                term.move_to(row, 1)?;

                let display_idx = scroll_offset + i;
                let line = Self::format_email(email, term.cols);

                if display_idx == self.cursor {
                    term.set_reverse()?;
                    if Self::is_unread(email) {
                        term.set_bold()?;
                    }
                } else if Self::is_unread(email) {
                    term.set_bold()?;
                }

                term.write_truncated(&line, term.cols)?;
                term.reset_attr()?;
            }
        }

        // Status bar
        term.move_to(term.rows, 1)?;
        term.set_reverse()?;
        let status = if self.search_mode {
            format!(" Search: {}_", self.search_input)
        } else if self.move_mode {
            format!(
                " {}/{} | n/p:navigate RET:move Esc:cancel",
                self.move_cursor + 1,
                self.mailboxes.len()
            )
        } else if self.loading {
            " Loading... | q:back".to_string()
        } else if self.emails.is_empty() {
            " q:back g:refresh s:search".to_string()
        } else {
            let search_hint = if self.active_search.is_some() {
                " Esc:clear-search"
            } else {
                ""
            };
            format!(
                " {}/{} | q:back n/p:nav RET:read g:refresh f:flag u:unread m:move s:search{}",
                self.cursor + 1,
                self.emails.len(),
                search_hint
            )
        };
        term.write_truncated(&status, term.cols)?;
        let remaining = (term.cols as usize).saturating_sub(status.len());
        for _ in 0..remaining {
            term.write_str(" ")?;
        }
        term.reset_attr()?;

        term.flush()
    }

    fn handle_key(&mut self, key: Key, term_rows: u16) -> ViewAction {
        // Search mode: capture text input
        if self.search_mode {
            match key {
                Key::Enter => {
                    self.search_mode = false;
                    if self.search_input.is_empty() {
                        // Empty search clears active search
                        self.active_search = None;
                    } else {
                        self.active_search = Some(self.search_input.clone());
                    }
                    self.search_input.clear();
                    self.request_refresh();
                }
                Key::Escape => {
                    self.search_mode = false;
                    self.search_input.clear();
                }
                Key::Backspace => {
                    self.search_input.pop();
                }
                Key::Char(c) => {
                    self.search_input.push(c);
                }
                _ => {}
            }
            return ViewAction::Continue;
        }

        // Move mode: mailbox picker
        if self.move_mode {
            match key {
                Key::Escape | Key::Char('q') => {
                    self.move_mode = false;
                }
                Key::Char('n') | Key::Char('j') | Key::Down => {
                    if !self.mailboxes.is_empty() && self.move_cursor + 1 < self.mailboxes.len() {
                        self.move_cursor += 1;
                    }
                }
                Key::Char('p') | Key::Char('k') | Key::Up => {
                    if self.move_cursor > 0 {
                        self.move_cursor -= 1;
                    }
                }
                Key::Enter => {
                    if let Some(target) = self.mailboxes.get(self.move_cursor) {
                        if let Some(email) = self.emails.get(self.cursor) {
                            let _ = self.cmd_tx.send(BackendCommand::MoveEmail {
                                id: email.id.clone(),
                                to_mailbox_id: target.id.clone(),
                            });
                            self.emails.remove(self.cursor);
                            if self.cursor >= self.emails.len() && self.cursor > 0 {
                                self.cursor -= 1;
                            }
                            if let Some(ref mut total) = self.total {
                                *total = total.saturating_sub(1);
                            }
                        }
                        self.move_mode = false;
                    }
                }
                Key::ScrollUp => {
                    if self.move_cursor > 0 {
                        self.move_cursor -= 1;
                    }
                }
                Key::ScrollDown => {
                    if !self.mailboxes.is_empty() && self.move_cursor + 1 < self.mailboxes.len() {
                        self.move_cursor += 1;
                    }
                }
                _ => {}
            }
            return ViewAction::Continue;
        }

        // Normal mode
        let page = (term_rows as usize).saturating_sub(4);
        match key {
            Key::Char('q') => ViewAction::Pop,
            Key::Char('n') | Key::Char('j') | Key::Down => {
                if !self.emails.is_empty() && self.cursor + 1 < self.emails.len() {
                    self.cursor += 1;
                }
                ViewAction::Continue
            }
            Key::Char('p') | Key::Char('k') | Key::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                ViewAction::Continue
            }
            Key::PageDown => {
                if !self.emails.is_empty() {
                    self.cursor = (self.cursor + page).min(self.emails.len() - 1);
                }
                ViewAction::Continue
            }
            Key::PageUp => {
                self.cursor = self.cursor.saturating_sub(page);
                ViewAction::Continue
            }
            Key::Home => {
                self.cursor = 0;
                ViewAction::Continue
            }
            Key::End => {
                if !self.emails.is_empty() {
                    self.cursor = self.emails.len() - 1;
                }
                ViewAction::Continue
            }
            Key::Enter => {
                if let Some(email) = self.emails.get_mut(self.cursor) {
                    let view = EmailView::new(
                        self.cmd_tx.clone(),
                        self.from_address.clone(),
                        email.id.clone(),
                    );
                    let _ = self.cmd_tx.send(BackendCommand::GetEmail {
                        id: email.id.clone(),
                    });
                    if !email.keywords.contains_key("$seen") {
                        email.keywords.insert("$seen".to_string(), true);
                        let _ = self.cmd_tx.send(BackendCommand::MarkEmailRead {
                            id: email.id.clone(),
                        });
                    }
                    ViewAction::Push(Box::new(view))
                } else {
                    ViewAction::Continue
                }
            }
            Key::Char('g') => {
                self.request_refresh();
                ViewAction::Continue
            }
            Key::Char('f') => {
                if let Some(email) = self.emails.get_mut(self.cursor) {
                    let is_flagged = email.keywords.contains_key("$flagged");
                    if is_flagged {
                        email.keywords.remove("$flagged");
                    } else {
                        email.keywords.insert("$flagged".to_string(), true);
                    }
                    let _ = self.cmd_tx.send(BackendCommand::SetEmailFlagged {
                        id: email.id.clone(),
                        flagged: !is_flagged,
                    });
                }
                ViewAction::Continue
            }
            Key::Char('u') => {
                if let Some(email) = self.emails.get_mut(self.cursor) {
                    let is_read = email.keywords.contains_key("$seen");
                    if is_read {
                        email.keywords.remove("$seen");
                        let _ = self.cmd_tx.send(BackendCommand::MarkEmailUnread {
                            id: email.id.clone(),
                        });
                    } else {
                        email.keywords.insert("$seen".to_string(), true);
                        let _ = self.cmd_tx.send(BackendCommand::MarkEmailRead {
                            id: email.id.clone(),
                        });
                    }
                }
                ViewAction::Continue
            }
            Key::Char('m') => {
                if !self.emails.is_empty() && !self.mailboxes.is_empty() {
                    self.move_mode = true;
                    self.move_cursor = 0;
                }
                ViewAction::Continue
            }
            Key::Char('s') => {
                self.search_mode = true;
                self.search_input.clear();
                ViewAction::Continue
            }
            Key::Escape => {
                if self.active_search.is_some() {
                    self.active_search = None;
                    self.request_refresh();
                }
                ViewAction::Continue
            }
            Key::Char('c') => {
                let draft = compose::build_compose_draft(&self.from_address);
                ViewAction::Compose(draft)
            }
            Key::Char('?') => ViewAction::Push(Box::new(HelpView::new())),
            Key::ScrollUp => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                ViewAction::Continue
            }
            Key::ScrollDown => {
                if !self.emails.is_empty() && self.cursor + 1 < self.emails.len() {
                    self.cursor += 1;
                }
                ViewAction::Continue
            }
            Key::MouseClick { row, col: _ } => {
                if row >= 3 && !self.emails.is_empty() {
                    let max_items = (term_rows as usize).saturating_sub(4);
                    let scroll_offset = if self.cursor >= max_items {
                        self.cursor - max_items + 1
                    } else {
                        0
                    };
                    let clicked = scroll_offset + (row - 3) as usize;
                    if clicked < self.emails.len() {
                        self.cursor = clicked;
                        self.pending_click = true;
                        return ViewAction::Continue;
                    }
                }
                ViewAction::Continue
            }
            _ => ViewAction::Continue,
        }
    }

    fn take_pending_action(&mut self) -> Option<ViewAction> {
        if self.pending_click {
            self.pending_click = false;
            if let Some(email) = self.emails.get_mut(self.cursor) {
                let view = EmailView::new(
                    self.cmd_tx.clone(),
                    self.from_address.clone(),
                    email.id.clone(),
                );
                let _ = self.cmd_tx.send(BackendCommand::GetEmail {
                    id: email.id.clone(),
                });
                if !email.keywords.contains_key("$seen") {
                    email.keywords.insert("$seen".to_string(), true);
                    let _ = self.cmd_tx.send(BackendCommand::MarkEmailRead {
                        id: email.id.clone(),
                    });
                }
                return Some(ViewAction::Push(Box::new(view)));
            }
        }
        None
    }

    fn on_response(&mut self, response: &BackendResponse) -> bool {
        match response {
            BackendResponse::Emails {
                mailbox_id,
                emails,
                total,
            } if *mailbox_id == self.mailbox_id => {
                self.loading = false;
                self.total = *total;
                match emails {
                    Ok(emails) => {
                        self.emails = emails.clone();
                        self.error = None;
                        if self.cursor >= self.emails.len() && !self.emails.is_empty() {
                            self.cursor = self.emails.len() - 1;
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to fetch emails: {}", e));
                    }
                }
                true
            }
            _ => false,
        }
    }
}
