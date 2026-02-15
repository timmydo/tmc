use crate::backend::{BackendCommand, BackendResponse, EmailMutationAction};
use crate::compose;
use crate::jmap::types::Email;
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::help::HelpView;
use crate::tui::views::{View, ViewAction};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc;

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

enum PendingWriteOp {
    Flag { old_flagged: bool },
    Seen { old_seen: bool },
}

pub struct EmailView {
    cmd_tx: mpsc::Sender<BackendCommand>,
    from_address: String,
    email_id: String,
    email: Option<Email>,
    lines: Vec<String>,
    scroll: usize,
    loading: bool,
    error: Option<String>,
    pending_reply_all: Option<bool>,
    pending_compose: Option<String>,
    status_message: Option<String>,
    next_write_op_id: u64,
    pending_write_ops: HashMap<u64, PendingWriteOp>,
    attachment_picking: bool,
    thread_id: Option<String>,
    thread_emails: Vec<Email>,
}

impl EmailView {
    pub fn new(
        cmd_tx: mpsc::Sender<BackendCommand>,
        from_address: String,
        email_id: String,
    ) -> Self {
        EmailView {
            cmd_tx,
            from_address,
            email_id,
            email: None,
            lines: Vec::new(),
            scroll: 0,
            loading: true,
            error: None,
            pending_reply_all: None,
            pending_compose: None,
            status_message: None,
            next_write_op_id: 1,
            pending_write_ops: HashMap::new(),
            attachment_picking: false,
            thread_id: None,
            thread_emails: Vec::new(),
        }
    }

    pub fn new_thread(
        cmd_tx: mpsc::Sender<BackendCommand>,
        from_address: String,
        thread_id: String,
        _subject: String,
    ) -> Self {
        let _ = cmd_tx.send(BackendCommand::QueryThreadEmails {
            thread_id: thread_id.clone(),
        });
        EmailView {
            cmd_tx,
            from_address,
            email_id: String::new(),
            email: None,
            lines: Vec::new(),
            scroll: 0,
            loading: true,
            error: None,
            pending_reply_all: None,
            pending_compose: None,
            status_message: None,
            next_write_op_id: 1,
            pending_write_ops: HashMap::new(),
            attachment_picking: false,
            thread_id: Some(thread_id),
            thread_emails: Vec::new(),
        }
    }

    fn render_email(email: &Email) -> Vec<String> {
        let mut lines = Vec::new();

        // Headers
        if let Some(ref from) = email.from {
            let addrs: Vec<String> = from.iter().map(|a| a.to_string()).collect();
            lines.push(format!("From: {}", addrs.join(", ")));
        }
        if let Some(ref to) = email.to {
            let addrs: Vec<String> = to.iter().map(|a| a.to_string()).collect();
            lines.push(format!("To: {}", addrs.join(", ")));
        }
        if let Some(ref cc) = email.cc {
            if !cc.is_empty() {
                let addrs: Vec<String> = cc.iter().map(|a| a.to_string()).collect();
                lines.push(format!("Cc: {}", addrs.join(", ")));
            }
        }
        if let Some(ref date) = email.received_at {
            lines.push(format!("Date: {}", date));
        }
        lines.push(format!(
            "Subject: {}",
            email.subject.as_deref().unwrap_or("(no subject)")
        ));

        // Attachments
        if let Some(ref attachments) = email.attachments {
            if !attachments.is_empty() {
                lines.push(String::new());
                lines.push(format!("Attachments ({})", attachments.len()));
                for (i, att) in attachments.iter().enumerate() {
                    let name = att.name.as_deref().unwrap_or("unnamed");
                    let size = att.size.map(format_size).unwrap_or_default();
                    let type_str = att.r#type.as_deref().unwrap_or("application/octet-stream");
                    lines.push(format!("  [{}] {} ({}, {})", i + 1, name, type_str, size));
                }
                lines.push("  Press 'a' then 1-9 to download/open".to_string());
            }
        }

        // Separator
        lines.push(String::new());

        // Body
        let body_text = Self::extract_body(email);
        for line in body_text.lines() {
            lines.push(line.to_string());
        }

        lines
    }

    fn render_thread_emails(emails: &[Email]) -> Vec<String> {
        let mut lines = Vec::new();
        for (i, email) in emails.iter().enumerate() {
            if i > 0 {
                lines.push(String::new());
                lines.push("─".repeat(60));
                lines.push(String::new());
            }
            // Headers for each email in thread
            if let Some(ref from) = email.from {
                let addrs: Vec<String> = from.iter().map(|a| a.to_string()).collect();
                lines.push(format!("From: {}", addrs.join(", ")));
            }
            if let Some(ref to) = email.to {
                let addrs: Vec<String> = to.iter().map(|a| a.to_string()).collect();
                lines.push(format!("To: {}", addrs.join(", ")));
            }
            if let Some(ref cc) = email.cc {
                if !cc.is_empty() {
                    let addrs: Vec<String> = cc.iter().map(|a| a.to_string()).collect();
                    lines.push(format!("Cc: {}", addrs.join(", ")));
                }
            }
            if let Some(ref date) = email.received_at {
                lines.push(format!("Date: {}", date));
            }
            lines.push(format!(
                "Subject: {}",
                email.subject.as_deref().unwrap_or("(no subject)")
            ));
            lines.push(String::new());
            let body_text = Self::extract_body(email);
            for line in body_text.lines() {
                lines.push(line.to_string());
            }
        }
        lines
    }

    fn extract_body(email: &Email) -> String {
        if let Some(ref text_body) = email.text_body {
            for part in text_body {
                if let Some(value) = email.body_values.get(&part.part_id) {
                    return value.value.clone();
                }
            }
        }

        email.preview.as_deref().unwrap_or("(no body)").to_string()
    }

    fn request_reply(&mut self, reply_all: bool) {
        self.pending_reply_all = Some(reply_all);
        // Fetch the email with reply headers (messageId, references, replyTo, sentAt)
        let _ = self.cmd_tx.send(BackendCommand::GetEmailForReply {
            id: self.email_id.clone(),
        });
    }

    fn next_op_id(&mut self) -> u64 {
        let id = self.next_write_op_id;
        self.next_write_op_id = self.next_write_op_id.wrapping_add(1);
        id
    }

    fn set_flagged(&mut self, flagged: bool) {
        if let Some(ref mut email) = self.email {
            if flagged {
                email.keywords.insert("$flagged".to_string(), true);
            } else {
                email.keywords.remove("$flagged");
            }
        }
    }

    fn set_seen(&mut self, seen: bool) {
        if let Some(ref mut email) = self.email {
            if seen {
                email.keywords.insert("$seen".to_string(), true);
            } else {
                email.keywords.remove("$seen");
            }
        }
    }

    fn download_attachment(&mut self, index: usize) {
        let attachment = self
            .email
            .as_ref()
            .and_then(|e| e.attachments.as_ref())
            .and_then(|a| a.get(index));

        if let Some(att) = attachment {
            if let Some(ref blob_id) = att.blob_id {
                let name = att.name.as_deref().unwrap_or("attachment").to_string();
                let content_type = att
                    .r#type
                    .as_deref()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                self.status_message = Some(format!("Downloading {}...", name));
                let _ = self.cmd_tx.send(BackendCommand::DownloadAttachment {
                    blob_id: blob_id.clone(),
                    name,
                    content_type,
                });
            } else {
                self.status_message = Some("Attachment has no blob ID".to_string());
            }
        } else {
            self.status_message = Some("Invalid attachment number".to_string());
        }
    }

    fn attachment_count(&self) -> usize {
        self.email
            .as_ref()
            .and_then(|e| e.attachments.as_ref())
            .map(|a| a.len())
            .unwrap_or(0)
    }

    fn rollback_pending_write(&mut self, op: PendingWriteOp) {
        match op {
            PendingWriteOp::Flag { old_flagged } => self.set_flagged(old_flagged),
            PendingWriteOp::Seen { old_seen } => self.set_seen(old_seen),
        }
    }
}

impl View for EmailView {
    fn wants_mouse(&self) -> bool {
        false
    }

    fn render(&self, term: &mut Terminal) -> io::Result<()> {
        term.clear()?;

        if self.loading {
            term.move_to(1, 1)?;
            let load_msg = if self.thread_id.is_some() {
                "Loading thread..."
            } else {
                "Loading email..."
            };
            term.write_truncated(load_msg, term.cols)?;
            term.move_to(term.rows, 1)?;
            term.set_reverse()?;
            term.write_truncated(" Loading... | q:back", term.cols)?;
            let remaining = (term.cols as usize).saturating_sub(20);
            for _ in 0..remaining {
                term.write_str(" ")?;
            }
            term.reset_attr()?;
            return term.flush();
        }

        if let Some(ref err) = self.error {
            term.move_to(1, 1)?;
            term.write_truncated(err, term.cols)?;
            term.move_to(term.rows, 1)?;
            term.set_reverse()?;
            term.write_truncated(" q:back", term.cols)?;
            let remaining = (term.cols as usize).saturating_sub(7);
            for _ in 0..remaining {
                term.write_str(" ")?;
            }
            term.reset_attr()?;
            return term.flush();
        }

        let visible_rows = (term.rows as usize).saturating_sub(1);

        for (i, line) in self
            .lines
            .iter()
            .skip(self.scroll)
            .enumerate()
            .take(visible_rows)
        {
            let row = 1 + i as u16;
            term.move_to(row, 1)?;

            // Bold headers (lines before the first empty line)
            let abs_idx = self.scroll + i;
            let is_header =
                abs_idx < self.lines.len() && self.lines[..abs_idx].iter().all(|l| !l.is_empty());

            if is_header && !line.is_empty() {
                term.set_bold()?;
                term.write_truncated(line, term.cols)?;
                term.reset_attr()?;
            } else {
                term.write_truncated(line, term.cols)?;
            }
        }

        // Status bar
        term.move_to(term.rows, 1)?;
        term.set_reverse()?;
        let total_lines = self.lines.len();
        let base_status = if self.attachment_picking {
            format!(
                " line {}/{} | Pick attachment [1-{}] or any key to cancel",
                self.scroll + 1,
                total_lines,
                self.attachment_count()
            )
        } else if self.pending_reply_all.is_some() {
            format!(
                " line {}/{} | Loading reply data... | q:back",
                self.scroll + 1,
                total_lines
            )
        } else {
            let att_hint = if self.attachment_count() > 0 {
                " a:attach"
            } else {
                ""
            };
            format!(
                " line {}/{} | q:back n/j:down p/k:up r:reply R:reply-all{} ?:help",
                self.scroll + 1,
                total_lines,
                att_hint
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
        // Attachment picking mode: waiting for digit
        if self.attachment_picking {
            self.attachment_picking = false;
            if let Key::Char(c @ '1'..='9') = key {
                let index = (c as usize) - ('1' as usize);
                self.download_attachment(index);
            } else {
                self.status_message = Some("Cancelled".to_string());
            }
            return ViewAction::Continue;
        }

        let page = (term_rows as usize).saturating_sub(1);
        match key {
            Key::Char('q') => ViewAction::Pop,
            Key::Char('n') | Key::Char('j') | Key::Down => {
                if self.scroll + 1 < self.lines.len() {
                    self.scroll += 1;
                }
                ViewAction::Continue
            }
            Key::Char('p') | Key::Char('k') | Key::Up => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                ViewAction::Continue
            }
            Key::PageDown | Key::Char(' ') => {
                self.scroll = (self.scroll + page).min(self.lines.len().saturating_sub(1));
                ViewAction::Continue
            }
            Key::PageUp => {
                self.scroll = self.scroll.saturating_sub(page);
                ViewAction::Continue
            }
            Key::Home => {
                self.scroll = 0;
                ViewAction::Continue
            }
            Key::End => {
                self.scroll = self.lines.len().saturating_sub(1);
                ViewAction::Continue
            }
            Key::Char('r') => {
                self.request_reply(false);
                ViewAction::Continue
            }
            Key::Char('R') => {
                self.request_reply(true);
                ViewAction::Continue
            }
            Key::Char('f') => {
                if let Some(ref email) = self.email {
                    let old_flagged = email.keywords.contains_key("$flagged");
                    let new_flagged = !old_flagged;
                    let op_id = self.next_op_id();
                    self.pending_write_ops
                        .insert(op_id, PendingWriteOp::Flag { old_flagged });
                    self.set_flagged(new_flagged);
                    if let Err(e) = self.cmd_tx.send(BackendCommand::SetEmailFlagged {
                        op_id,
                        id: self.email_id.clone(),
                        flagged: new_flagged,
                    }) {
                        self.pending_write_ops.remove(&op_id);
                        self.set_flagged(old_flagged);
                        self.status_message = Some(format!("Flag update failed: {}", e));
                    }
                }
                ViewAction::Continue
            }
            Key::Char('u') => {
                if let Some(ref email) = self.email {
                    let old_seen = email.keywords.contains_key("$seen");
                    let new_seen = !old_seen;
                    let op_id = self.next_op_id();
                    self.pending_write_ops
                        .insert(op_id, PendingWriteOp::Seen { old_seen });
                    self.set_seen(new_seen);
                    let send_result = if new_seen {
                        self.cmd_tx.send(BackendCommand::MarkEmailRead {
                            op_id,
                            id: self.email_id.clone(),
                        })
                    } else {
                        self.cmd_tx.send(BackendCommand::MarkEmailUnread {
                            op_id,
                            id: self.email_id.clone(),
                        })
                    };
                    if let Err(e) = send_result {
                        self.pending_write_ops.remove(&op_id);
                        self.set_seen(old_seen);
                        self.status_message = Some(format!("Read state update failed: {}", e));
                    }
                }
                ViewAction::Continue
            }
            Key::Char('a') => {
                let count = self.attachment_count();
                if count == 0 {
                    self.status_message = Some("No attachments".to_string());
                } else if count == 1 {
                    self.download_attachment(0);
                } else {
                    self.attachment_picking = true;
                    self.status_message = Some(format!("Download attachment [1-{}]:", count));
                }
                ViewAction::Continue
            }
            Key::Char('c') => {
                let draft = compose::build_compose_draft(&self.from_address);
                ViewAction::Compose(draft)
            }
            Key::Char('?') => ViewAction::Push(Box::new(HelpView::new())),
            Key::ScrollUp => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                ViewAction::Continue
            }
            Key::ScrollDown => {
                if self.scroll + 1 < self.lines.len() {
                    self.scroll += 1;
                }
                ViewAction::Continue
            }
            _ => ViewAction::Continue,
        }
    }

    fn on_response(&mut self, response: &BackendResponse) -> bool {
        match response {
            BackendResponse::ThreadEmails { thread_id, emails }
                if self.thread_id.as_deref() == Some(thread_id) =>
            {
                self.loading = false;
                match emails {
                    Ok(emails) => {
                        self.thread_emails = emails.clone();
                        if let Some(last) = emails.last() {
                            self.email_id = last.id.clone();
                            self.email = Some(last.clone());
                        }
                        self.lines = Self::render_thread_emails(&self.thread_emails);
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load thread: {}", e));
                    }
                }
                true
            }
            BackendResponse::EmailBody { id, result } if *id == self.email_id => {
                self.loading = false;
                match result.as_ref() {
                    Ok(email) => {
                        self.lines = Self::render_email(email);
                        self.email = Some(email.clone());
                        self.error = None;
                        self.pending_write_ops.clear();
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load email: {}", e));
                    }
                }
                true
            }
            BackendResponse::EmailForReply { id, result } if *id == self.email_id => {
                let reply_all = self.pending_reply_all.take();
                match result.as_ref() {
                    Ok(email) => {
                        self.email = Some(email.clone());
                        if let Some(reply_all) = reply_all {
                            let draft =
                                compose::build_reply_draft(email, reply_all, &self.from_address);
                            self.pending_compose = Some(draft);
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load reply data: {}", e));
                    }
                }
                true
            }
            BackendResponse::EmailMutation {
                op_id,
                id,
                action,
                result,
            } if *id == self.email_id => {
                if let Some(pending) = self.pending_write_ops.remove(op_id) {
                    if let Err(e) = result {
                        self.rollback_pending_write(pending);
                        let action_label = match action {
                            EmailMutationAction::MarkRead => "Mark read",
                            EmailMutationAction::MarkUnread => "Mark unread",
                            EmailMutationAction::SetFlagged(_) => "Flag update",
                            EmailMutationAction::Move => "Move",
                        };
                        self.status_message = Some(format!("{} failed: {}", action_label, e));
                    }
                    true
                } else {
                    false
                }
            }
            BackendResponse::AttachmentDownloaded { name, result } => {
                match result {
                    Ok(path) => {
                        self.status_message = Some(format!("Saved: {}", path.display()));
                        // Try to open with xdg-open / open
                        let opener = std::env::var("OPENER").unwrap_or_else(|_| {
                            if cfg!(target_os = "macos") {
                                "open".to_string()
                            } else {
                                "xdg-open".to_string()
                            }
                        });
                        match std::process::Command::new(&opener)
                            .arg(path)
                            .stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn()
                        {
                            Ok(_) => {}
                            Err(e) => {
                                self.status_message =
                                    Some(format!("Saved {} (could not open: {})", name, e));
                            }
                        }
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Download failed: {}", e));
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn take_pending_action(&mut self) -> Option<ViewAction> {
        self.pending_compose.take().map(ViewAction::Compose)
    }
}
