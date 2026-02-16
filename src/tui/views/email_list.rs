use crate::backend::{BackendCommand, BackendResponse, EmailMutationAction, RulesDryRunResult};
use crate::compose;
use crate::jmap::types::{Email, Mailbox};
use crate::rules;
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::email_view::EmailView;
use crate::tui::views::help::HelpView;
use crate::tui::views::rules_preview::RulesPreviewView;
use crate::tui::views::thread_view::ThreadView;
use crate::tui::views::{View, ViewAction};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::mpsc;

enum PendingWriteOp {
    Flag {
        email_id: String,
        old_flagged: bool,
    },
    Seen {
        email_id: String,
        old_seen: bool,
    },
    Move {
        email: Box<Email>,
        from_index: usize,
    },
}

pub struct EmailListView {
    cmd_tx: mpsc::Sender<BackendCommand>,
    from_address: String,
    mailbox_id: String,
    mailbox_name: String,
    page_size: u32,
    emails: Vec<Email>,
    cursor: usize,
    total: Option<u32>,
    next_query_position: u32,
    last_loaded_count: u32,
    loading: bool,
    loading_more: bool,
    error: Option<String>,
    pending_click: bool,
    pending_reply_all_email_id: Option<String>,
    pending_compose: Option<String>,
    pending_rules_preview: Option<(String, RulesDryRunResult)>,
    mailboxes: Vec<Mailbox>,
    move_mode: bool,
    move_cursor: usize,
    search_mode: bool,
    search_input: String,
    active_search: Option<String>,
    status_message: Option<String>,
    next_write_op_id: u64,
    pending_write_ops: HashMap<u64, PendingWriteOp>,
    thread_counts: HashMap<String, (usize, usize)>,
    scroll_offset: usize,
    archive_folder: String,
    deleted_folder: String,
}

impl EmailListView {
    pub fn new(
        cmd_tx: mpsc::Sender<BackendCommand>,
        from_address: String,
        mailbox_id: String,
        mailbox_name: String,
        page_size: u32,
        mailboxes: Vec<Mailbox>,
        archive_folder: String,
        deleted_folder: String,
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
            next_query_position: 0,
            last_loaded_count: 0,
            loading: true,
            loading_more: false,
            error: None,
            pending_click: false,
            pending_reply_all_email_id: None,
            pending_compose: None,
            pending_rules_preview: None,
            mailboxes,
            move_mode: false,
            move_cursor: 0,
            search_mode: false,
            search_input: String::new(),
            active_search: None,
            status_message: None,
            next_write_op_id: 1,
            pending_write_ops: HashMap::new(),
            thread_counts: HashMap::new(),
            scroll_offset: 0,
            archive_folder,
            deleted_folder,
        }
    }

    fn request_refresh(&mut self, origin: &str) {
        self.next_query_position = 0;
        self.last_loaded_count = 0;
        self.loading = true;
        self.loading_more = false;
        self.scroll_offset = 0;
        let _ = self.cmd_tx.send(BackendCommand::QueryEmails {
            origin: origin.to_string(),
            mailbox_id: self.mailbox_id.clone(),
            page_size: self.page_size,
            position: 0,
            search_query: self.active_search.clone(),
        });
    }

    fn can_load_more(&self) -> bool {
        if self.loading || self.emails.is_empty() {
            return false;
        }

        if let Some(total) = self.total {
            (self.emails.len() as u32) < total
        } else {
            self.last_loaded_count >= self.page_size
        }
    }

    fn request_load_more(&mut self) -> bool {
        if !self.can_load_more() {
            return false;
        }

        self.loading = true;
        self.loading_more = true;
        match self.cmd_tx.send(BackendCommand::QueryEmails {
            origin: "email_list.load_more".to_string(),
            mailbox_id: self.mailbox_id.clone(),
            page_size: self.page_size,
            position: self.next_query_position,
            search_query: self.active_search.clone(),
        }) {
            Ok(()) => true,
            Err(e) => {
                self.loading = false;
                self.loading_more = false;
                self.status_message = Some(format!("Load more failed: {}", e));
                false
            }
        }
    }

    fn is_unread(email: &Email) -> bool {
        !email.keywords.contains_key("$seen")
    }

    fn is_flagged(email: &Email) -> bool {
        email.keywords.contains_key("$flagged")
    }

    fn format_email(email: &Email, width: u16, thread_counts: Option<(usize, usize)>) -> String {
        let unread = if Self::is_unread(email) { "N" } else { " " };
        let flagged = if Self::is_flagged(email) { "F" } else { " " };

        // Fixed 8-char column for thread indicator [read/total]
        let thread_col = match thread_counts {
            Some((unread_count, total)) if total > 1 => {
                let read_count = total - unread_count;
                format!("[{}/{}]", read_count, total)
            }
            _ => String::new(),
        };
        let thread_display = format!("{:<8}", thread_col);

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
            .map(|d| if d.len() >= 10 { &d[..10] } else { d })
            .unwrap_or("");

        let w = width as usize;
        // " NF" (4) + thread_display (8) + date (10) + " " (1) + from + " " (1) + subject
        let prefix_len = 4 + 8 + 10 + 1;
        let from_width = 20.min(w.saturating_sub(prefix_len + 1));
        let subj_width = w.saturating_sub(prefix_len + from_width + 1);

        let from_display = truncate(from, from_width);
        let subj_display = truncate(subject, subj_width);

        format!(
            " {}{}{}{} {:from_w$} {}",
            unread,
            flagged,
            thread_display,
            date,
            from_display,
            subj_display,
            from_w = from_width
        )
    }

    fn get_thread_counts(&self, email: &Email) -> Option<(usize, usize)> {
        email
            .thread_id
            .as_ref()
            .and_then(|tid| self.thread_counts.get(tid))
            .copied()
    }

    fn next_op_id(&mut self) -> u64 {
        let id = self.next_write_op_id;
        self.next_write_op_id = self.next_write_op_id.wrapping_add(1);
        id
    }

    fn set_email_flag_state(&mut self, email_id: &str, flagged: bool) {
        if let Some(email) = self.emails.iter_mut().find(|e| e.id == email_id) {
            if flagged {
                email.keywords.insert("$flagged".to_string(), true);
            } else {
                email.keywords.remove("$flagged");
            }
        }
    }

    fn set_email_seen_state(&mut self, email_id: &str, seen: bool) {
        if let Some(email) = self.emails.iter_mut().find(|e| e.id == email_id) {
            if seen {
                email.keywords.insert("$seen".to_string(), true);
            } else {
                email.keywords.remove("$seen");
            }
        }
    }

    fn rollback_pending_write(&mut self, op: PendingWriteOp) {
        match op {
            PendingWriteOp::Flag {
                email_id,
                old_flagged,
            } => self.set_email_flag_state(&email_id, old_flagged),
            PendingWriteOp::Seen { email_id, old_seen } => {
                self.set_email_seen_state(&email_id, old_seen)
            }
            PendingWriteOp::Move { email, from_index } => {
                let insert_at = from_index.min(self.emails.len());
                self.emails.insert(insert_at, *email);
                self.cursor = insert_at;
                if let Some(ref mut total) = self.total {
                    *total = total.saturating_add(1);
                }
            }
        }
    }

    fn open_selected(&mut self) -> Option<ViewAction> {
        let email = self.emails.get(self.cursor)?;
        let thread_total = self
            .get_thread_counts(email)
            .map(|(_, total)| total)
            .unwrap_or(1);
        let can_expire_now = self.is_in_deleted_folder();

        if thread_total > 1 {
            // Open concatenated thread reading view
            let thread_id = email.thread_id.clone().unwrap_or_default();
            let subject = email
                .subject
                .clone()
                .unwrap_or_else(|| "(no subject)".to_string());
            let view = EmailView::new_thread(
                self.cmd_tx.clone(),
                self.from_address.clone(),
                thread_id,
                subject,
                can_expire_now,
            );
            Some(ViewAction::Push(Box::new(view)))
        } else {
            self.open_single_email()
        }
    }

    fn open_thread_list(&mut self) -> Option<ViewAction> {
        let email = self.emails.get(self.cursor)?;
        let thread_total = self
            .get_thread_counts(email)
            .map(|(_, total)| total)
            .unwrap_or(1);
        let can_expire_now = self.is_in_deleted_folder();

        if thread_total > 1 {
            let thread_id = email.thread_id.clone().unwrap_or_default();
            let subject = email
                .subject
                .clone()
                .unwrap_or_else(|| "(no subject)".to_string());
            let view = ThreadView::new(
                self.cmd_tx.clone(),
                self.from_address.clone(),
                thread_id,
                subject,
                self.mailboxes.clone(),
                self.archive_folder.clone(),
                self.deleted_folder.clone(),
                can_expire_now,
            );
            Some(ViewAction::Push(Box::new(view)))
        } else {
            self.open_single_email()
        }
    }

    fn open_single_email(&mut self) -> Option<ViewAction> {
        let email = self.emails.get(self.cursor)?;
        let email_id = email.id.clone();
        let was_seen = email.keywords.contains_key("$seen");
        let view = EmailView::new(
            self.cmd_tx.clone(),
            self.from_address.clone(),
            email_id.clone(),
            self.is_in_deleted_folder(),
        );
        let _ = self.cmd_tx.send(BackendCommand::GetEmail {
            id: email_id.clone(),
        });
        if !was_seen {
            let op_id = self.next_op_id();
            self.pending_write_ops.insert(
                op_id,
                PendingWriteOp::Seen {
                    email_id: email_id.clone(),
                    old_seen: false,
                },
            );
            self.set_email_seen_state(&email_id, true);
            if let Err(e) = self.cmd_tx.send(BackendCommand::MarkEmailRead {
                op_id,
                id: email_id.clone(),
            }) {
                self.record_send_failure(
                    op_id,
                    PendingWriteOp::Seen {
                        email_id,
                        old_seen: false,
                    },
                    "Mark read",
                    e.to_string(),
                );
            }
        }
        Some(ViewAction::Push(Box::new(view)))
    }

    fn record_send_failure(&mut self, op_id: u64, op: PendingWriteOp, action: &str, err: String) {
        self.pending_write_ops.remove(&op_id);
        self.rollback_pending_write(op);
        self.status_message = Some(format!("{} failed: {}", action, err));
    }

    fn adjust_scroll(&mut self, max_items: usize) {
        if max_items == 0 {
            return;
        }
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + max_items {
            self.scroll_offset = self.cursor - max_items + 1;
        }
    }

    fn is_in_deleted_folder(&self) -> bool {
        if self.mailbox_name.eq_ignore_ascii_case(&self.deleted_folder) {
            return true;
        }
        rules::resolve_mailbox_id(&self.deleted_folder, &self.mailboxes)
            .is_some_and(|id| id == self.mailbox_id)
    }

    fn move_selected_to_folder(&mut self, folder: &str, action_label: &str) {
        let Some(target_id) = rules::resolve_mailbox_id(folder, &self.mailboxes) else {
            self.status_message = Some(format!(
                "{} failed: could not resolve folder '{}'",
                action_label, folder
            ));
            return;
        };

        let Some(email) = self.emails.get(self.cursor).cloned() else {
            return;
        };
        let thread_total = self
            .get_thread_counts(&email)
            .map(|(_, total)| total)
            .unwrap_or(1);
        let op_id = self.next_op_id();
        let from_index = self.cursor;
        self.pending_write_ops.insert(
            op_id,
            PendingWriteOp::Move {
                email: Box::new(email.clone()),
                from_index,
            },
        );

        let send_result = if thread_total > 1 {
            if let Some(thread_id) = email.thread_id.clone() {
                self.cmd_tx.send(BackendCommand::MoveThread {
                    op_id,
                    thread_id,
                    to_mailbox_id: target_id,
                })
            } else {
                self.cmd_tx.send(BackendCommand::MoveEmail {
                    op_id,
                    id: email.id.clone(),
                    to_mailbox_id: target_id,
                })
            }
        } else {
            self.cmd_tx.send(BackendCommand::MoveEmail {
                op_id,
                id: email.id.clone(),
                to_mailbox_id: target_id,
            })
        };

        self.emails.remove(from_index);
        if self.cursor >= self.emails.len() && self.cursor > 0 {
            self.cursor -= 1;
        }
        if let Some(ref mut total) = self.total {
            *total = total.saturating_sub(1);
        }
        if let Err(e) = send_result {
            self.record_send_failure(
                op_id,
                PendingWriteOp::Move {
                    email: Box::new(email),
                    from_index,
                },
                action_label,
                e.to_string(),
            );
        }
    }

    fn expire_selected_now(&mut self) {
        let Some(email) = self.emails.get(self.cursor).cloned() else {
            return;
        };
        let thread_total = self
            .get_thread_counts(&email)
            .map(|(_, total)| total)
            .unwrap_or(1);
        let op_id = self.next_op_id();
        let from_index = self.cursor;
        self.pending_write_ops.insert(
            op_id,
            PendingWriteOp::Move {
                email: Box::new(email.clone()),
                from_index,
            },
        );
        let send_result = if thread_total > 1 {
            if let Some(thread_id) = email.thread_id.clone() {
                self.cmd_tx
                    .send(BackendCommand::DestroyThread { op_id, thread_id })
            } else {
                self.cmd_tx.send(BackendCommand::DestroyEmail {
                    op_id,
                    id: email.id.clone(),
                })
            }
        } else {
            self.cmd_tx.send(BackendCommand::DestroyEmail {
                op_id,
                id: email.id.clone(),
            })
        };
        self.emails.remove(from_index);
        if self.cursor >= self.emails.len() && self.cursor > 0 {
            self.cursor -= 1;
        }
        if let Some(ref mut total) = self.total {
            *total = total.saturating_sub(1);
        }
        if let Err(e) = send_result {
            self.record_send_failure(
                op_id,
                PendingWriteOp::Move {
                    email: Box::new(email),
                    from_index,
                },
                "Expire",
                e.to_string(),
            );
        }
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
                Some(total) => format!(
                    "{} [search: {}] ({} results)",
                    self.mailbox_name, query, total
                ),
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

            for (i, email) in self
                .emails
                .iter()
                .skip(self.scroll_offset)
                .enumerate()
                .take(max_items)
            {
                let row = 3 + i as u16;
                term.move_to(row, 1)?;

                let display_idx = self.scroll_offset + i;
                let thread_counts = self.get_thread_counts(email);
                let line = Self::format_email(email, term.cols, thread_counts);

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
        let base_status = if self.search_mode {
            format!(" Search: {}_", self.search_input)
        } else if self.move_mode {
            format!(
                " {}/{} | n/p:navigate RET:move Esc:cancel",
                self.move_cursor + 1,
                self.mailboxes.len()
            )
        } else if self.loading {
            if self.loading_more {
                " Loading more... | q:back".to_string()
            } else {
                " Loading... | q:back".to_string()
            }
        } else if self.emails.is_empty() {
            " q:back g:refresh s:search".to_string()
        } else {
            let search_hint = if self.active_search.is_some() {
                " Esc:clear-search"
            } else {
                ""
            };
            let load_more_hint = if self.can_load_more() { " l:more" } else { "" };
            let expire_hint = if self.is_in_deleted_folder() {
                " D:expire"
            } else {
                ""
            };
            format!(
                " {}/{} | q:back n/p:nav RET:read g:refresh R:reply-all e:dry-run E:run-rules a:archive d:delete{} f:flag u:unread m:move s:search{}{}",
                self.cursor + 1,
                self.emails.len(),
                expire_hint,
                search_hint,
                load_more_hint
            )
        };
        let status = if let Some(ref msg) = self.status_message {
            format!("{} | {}", msg, base_status)
        } else {
            base_status
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
                    self.request_refresh("email_list.search_submit");
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
                    if let Some(target_id) =
                        self.mailboxes.get(self.move_cursor).map(|m| m.id.clone())
                    {
                        if let Some(email) = self.emails.get(self.cursor).cloned() {
                            let op_id = self.next_op_id();
                            let from_index = self.cursor;
                            self.pending_write_ops.insert(
                                op_id,
                                PendingWriteOp::Move {
                                    email: Box::new(email.clone()),
                                    from_index,
                                },
                            );
                            let send_result = self.cmd_tx.send(BackendCommand::MoveEmail {
                                op_id,
                                id: email.id.clone(),
                                to_mailbox_id: target_id,
                            });
                            self.emails.remove(from_index);
                            if self.cursor >= self.emails.len() && self.cursor > 0 {
                                self.cursor -= 1;
                            }
                            if let Some(ref mut total) = self.total {
                                *total = total.saturating_sub(1);
                            }
                            if let Err(e) = send_result {
                                self.record_send_failure(
                                    op_id,
                                    PendingWriteOp::Move {
                                        email: Box::new(email),
                                        from_index,
                                    },
                                    "Move",
                                    e.to_string(),
                                );
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
        let max_items = (term_rows as usize).saturating_sub(4);
        let page = max_items;
        match key {
            Key::Char('q') => ViewAction::Pop,
            Key::Char('n') | Key::Char('j') | Key::Down => {
                if !self.emails.is_empty() && self.cursor + 1 < self.emails.len() {
                    self.cursor += 1;
                    self.adjust_scroll(max_items);
                } else {
                    self.request_load_more();
                }
                ViewAction::Continue
            }
            Key::Char('p') | Key::Char('k') | Key::Up => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.adjust_scroll(max_items);
                }
                ViewAction::Continue
            }
            Key::PageDown => {
                if !self.emails.is_empty() {
                    self.cursor = (self.cursor + page).min(self.emails.len() - 1);
                    self.adjust_scroll(max_items);
                    if self.cursor + 1 >= self.emails.len() {
                        self.request_load_more();
                    }
                }
                ViewAction::Continue
            }
            Key::PageUp => {
                self.cursor = self.cursor.saturating_sub(page);
                self.adjust_scroll(max_items);
                ViewAction::Continue
            }
            Key::Home => {
                self.cursor = 0;
                self.adjust_scroll(max_items);
                ViewAction::Continue
            }
            Key::End => {
                if !self.emails.is_empty() {
                    self.cursor = self.emails.len() - 1;
                    self.adjust_scroll(max_items);
                    self.request_load_more();
                }
                ViewAction::Continue
            }
            Key::Enter => self.open_selected().unwrap_or(ViewAction::Continue),
            Key::AltEnter => self.open_thread_list().unwrap_or(ViewAction::Continue),
            Key::Char('g') => {
                self.request_refresh("email_list.key_g");
                ViewAction::Continue
            }
            Key::Char('R') => {
                if let Some(email) = self.emails.get(self.cursor) {
                    self.pending_reply_all_email_id = Some(email.id.clone());
                    if let Err(e) = self.cmd_tx.send(BackendCommand::GetEmailForReply {
                        id: email.id.clone(),
                    }) {
                        self.pending_reply_all_email_id = None;
                        self.status_message = Some(format!("Reply all failed to send: {}", e));
                    } else {
                        self.status_message = Some("Preparing reply-all draft...".to_string());
                    }
                }
                ViewAction::Continue
            }
            Key::Char('e') => {
                let mailbox_id = self.mailbox_id.clone();
                let mailbox_name = self.mailbox_name.clone();
                let loaded_email_ids = self.emails.iter().map(|e| e.id.clone()).collect();
                if let Err(e) = self.cmd_tx.send(BackendCommand::PreviewRulesForMailbox {
                    origin: "email_list.key_e_dry_run".to_string(),
                    mailbox_id,
                    mailbox_name: mailbox_name.clone(),
                    loaded_email_ids,
                }) {
                    self.status_message = Some(format!("Rules dry-run failed to send: {}", e));
                } else {
                    self.status_message =
                        Some(format!("Running rules dry-run in '{}'", mailbox_name));
                }
                ViewAction::Continue
            }
            Key::Char('E') => {
                let mailbox_id = self.mailbox_id.clone();
                let mailbox_name = self.mailbox_name.clone();
                let loaded_email_ids = self.emails.iter().map(|e| e.id.clone()).collect();
                if let Err(e) = self.cmd_tx.send(BackendCommand::RunRulesForMailbox {
                    origin: "email_list.key_E_run_rules".to_string(),
                    mailbox_id,
                    mailbox_name: mailbox_name.clone(),
                    loaded_email_ids,
                }) {
                    self.status_message = Some(format!("Run rules failed to send: {}", e));
                } else {
                    self.status_message = Some(format!("Running rules in '{}'", mailbox_name));
                }
                ViewAction::Continue
            }
            Key::Char('f') => {
                if let Some(email) = self.emails.get(self.cursor) {
                    let email_id = email.id.clone();
                    let old_flagged = email.keywords.contains_key("$flagged");
                    let new_flagged = !old_flagged;
                    let op_id = self.next_op_id();
                    self.pending_write_ops.insert(
                        op_id,
                        PendingWriteOp::Flag {
                            email_id: email_id.clone(),
                            old_flagged,
                        },
                    );
                    self.set_email_flag_state(&email_id, new_flagged);
                    if let Err(e) = self.cmd_tx.send(BackendCommand::SetEmailFlagged {
                        op_id,
                        id: email_id.clone(),
                        flagged: new_flagged,
                    }) {
                        self.record_send_failure(
                            op_id,
                            PendingWriteOp::Flag {
                                email_id,
                                old_flagged,
                            },
                            "Flag update",
                            e.to_string(),
                        );
                    }
                }
                ViewAction::Continue
            }
            Key::Char('u') => {
                if let Some(email) = self.emails.get(self.cursor) {
                    let email_id = email.id.clone();
                    let old_seen = email.keywords.contains_key("$seen");
                    let new_seen = !old_seen;
                    let op_id = self.next_op_id();
                    self.pending_write_ops.insert(
                        op_id,
                        PendingWriteOp::Seen {
                            email_id: email_id.clone(),
                            old_seen,
                        },
                    );
                    self.set_email_seen_state(&email_id, new_seen);
                    let send_result = if new_seen {
                        self.cmd_tx.send(BackendCommand::MarkEmailRead {
                            op_id,
                            id: email_id.clone(),
                        })
                    } else {
                        self.cmd_tx.send(BackendCommand::MarkEmailUnread {
                            op_id,
                            id: email_id.clone(),
                        })
                    };
                    if let Err(e) = send_result {
                        self.record_send_failure(
                            op_id,
                            PendingWriteOp::Seen { email_id, old_seen },
                            "Read state update",
                            e.to_string(),
                        );
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
            Key::Char('a') => {
                let target = self.archive_folder.clone();
                self.move_selected_to_folder(&target, "Archive");
                ViewAction::Continue
            }
            Key::Char('d') => {
                let target = self.deleted_folder.clone();
                self.move_selected_to_folder(&target, "Delete");
                ViewAction::Continue
            }
            Key::Char('D') => {
                if self.is_in_deleted_folder() {
                    self.expire_selected_now();
                } else {
                    self.status_message =
                        Some("Expire is only available in the deleted folder".to_string());
                }
                ViewAction::Continue
            }
            Key::Char('s') => {
                self.search_mode = true;
                self.search_input.clear();
                ViewAction::Continue
            }
            Key::Char('l') => {
                self.request_load_more();
                ViewAction::Continue
            }
            Key::Escape => {
                if self.active_search.is_some() {
                    self.active_search = None;
                    self.request_refresh("email_list.clear_search_escape");
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
                    self.adjust_scroll(max_items);
                }
                ViewAction::Continue
            }
            Key::ScrollDown => {
                if !self.emails.is_empty() && self.cursor + 1 < self.emails.len() {
                    self.cursor += 1;
                    self.adjust_scroll(max_items);
                } else {
                    self.request_load_more();
                }
                ViewAction::Continue
            }
            Key::MouseClick { row, col: _ } => {
                if row >= 3 && !self.emails.is_empty() {
                    let clicked = self.scroll_offset + (row - 3) as usize;
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
        if let Some(draft) = self.pending_compose.take() {
            return Some(ViewAction::Compose(draft));
        }
        if let Some((mailbox_name, preview)) = self.pending_rules_preview.take() {
            return Some(ViewAction::Push(Box::new(RulesPreviewView::new(
                mailbox_name,
                preview,
            ))));
        }
        if self.pending_click {
            self.pending_click = false;
            return self.open_selected();
        }
        None
    }

    fn on_response(&mut self, response: &BackendResponse) -> bool {
        match response {
            BackendResponse::Emails {
                mailbox_id,
                emails,
                total,
                position,
                loaded,
                thread_counts,
            } if *mailbox_id == self.mailbox_id => {
                self.loading = false;
                self.loading_more = false;
                self.total = *total;
                self.last_loaded_count = *loaded;
                self.next_query_position = position.saturating_add(*loaded);
                match emails {
                    Ok(emails) => {
                        if *position == 0 {
                            self.emails = emails.clone();
                            self.pending_write_ops.clear();
                            self.thread_counts = thread_counts.clone();
                        } else {
                            self.thread_counts
                                .extend(thread_counts.iter().map(|(k, v)| (k.clone(), *v)));
                            let mut existing_ids: HashSet<String> =
                                self.emails.iter().map(|e| e.id.clone()).collect();
                            for email in emails {
                                if existing_ids.insert(email.id.clone()) {
                                    self.emails.push(email.clone());
                                }
                            }
                        }
                        self.error = None;
                        if self.cursor >= self.emails.len() && !self.emails.is_empty() {
                            self.cursor = self.emails.len() - 1;
                        }
                    }
                    Err(e) => {
                        if *position == 0 {
                            self.error = Some(format!("Failed to fetch emails: {}", e));
                        } else {
                            self.status_message = Some(format!("Load more failed: {}", e));
                        }
                    }
                }
                true
            }
            BackendResponse::EmailMutation {
                op_id,
                id: _,
                action,
                result,
            } => {
                if let Some(pending) = self.pending_write_ops.remove(op_id) {
                    match result {
                        Ok(()) => {
                            if let PendingWriteOp::Seen { .. } = pending {
                                self.request_refresh("email_list.seen_mutation_followup");
                            }
                        }
                        Err(e) => {
                            self.rollback_pending_write(pending);
                            let action_label = match action {
                                EmailMutationAction::MarkRead => "Mark read",
                                EmailMutationAction::MarkUnread => "Mark unread",
                                EmailMutationAction::SetFlagged(_) => "Flag update",
                                EmailMutationAction::Move => "Move",
                                EmailMutationAction::Destroy => "Expire",
                            };
                            self.status_message = Some(format!("{} failed: {}", action_label, e));
                        }
                    }
                    true
                } else {
                    false
                }
            }
            BackendResponse::EmailForReply { id, result } => {
                if self.pending_reply_all_email_id.as_deref() == Some(id.as_str()) {
                    self.pending_reply_all_email_id = None;
                    match result.as_ref() {
                        Ok(email) => {
                            let draft = compose::build_reply_draft(email, true, &self.from_address);
                            self.pending_compose = Some(draft);
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Reply all failed: {}", e));
                        }
                    }
                    true
                } else {
                    false
                }
            }
            BackendResponse::ThreadMarkedRead { result, .. } => {
                if result.is_ok() {
                    self.request_refresh("email_list.thread_marked_read");
                    true
                } else {
                    false
                }
            }
            BackendResponse::RulesDryRun {
                mailbox_id,
                mailbox_name,
                result,
            } if *mailbox_id == self.mailbox_id => {
                match result {
                    Ok(preview) => {
                        self.status_message = Some(format!(
                            "Rules dry-run in '{}': scanned {}, matched {}, actions {}",
                            mailbox_name, preview.scanned, preview.matched_rules, preview.actions
                        ));
                        self.pending_rules_preview = Some((mailbox_name.clone(), preview.clone()));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Rules dry-run failed: {}", e));
                    }
                }
                true
            }
            BackendResponse::RulesRun {
                mailbox_id,
                mailbox_name,
                result,
            } if *mailbox_id == self.mailbox_id => {
                match result {
                    Ok(summary) => {
                        self.status_message = Some(format!(
                            "Rules run in '{}': scanned {}, matched {}, actions {}",
                            mailbox_name, summary.scanned, summary.matched_rules, summary.actions
                        ));
                        self.request_refresh("email_list.rules_run_followup");
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Rules run failed: {}", e));
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn trigger_periodic_sync(&mut self) -> bool {
        if self.loading || self.move_mode || self.search_mode {
            return false;
        }
        self.request_refresh("email_list.periodic_sync");
        true
    }
}
