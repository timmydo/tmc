use crate::backend::BackendResponse;
use crate::compose;
use crate::jmap::types::Email;
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::help::HelpView;
use crate::tui::views::{View, ViewAction};
use std::io;
use std::sync::mpsc;

use crate::backend::BackendCommand;

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

        // Separator
        lines.push(String::new());

        // Body
        let body_text = Self::extract_body(email);
        for line in body_text.lines() {
            lines.push(line.to_string());
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

        email
            .preview
            .as_deref()
            .unwrap_or("(no body)")
            .to_string()
    }

    fn request_reply(&mut self, reply_all: bool) {
        self.pending_reply_all = Some(reply_all);
        // Fetch the email with reply headers (messageId, references, replyTo, sentAt)
        let _ = self.cmd_tx.send(BackendCommand::GetEmailForReply {
            id: self.email_id.clone(),
        });
    }
}

impl View for EmailView {
    fn render(&self, term: &mut Terminal) -> io::Result<()> {
        term.clear()?;

        if self.loading {
            term.move_to(1, 1)?;
            term.write_truncated("Loading email...", term.cols)?;
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

        for (i, line) in self.lines.iter().skip(self.scroll).enumerate().take(visible_rows) {
            let row = 1 + i as u16;
            term.move_to(row, 1)?;

            // Bold headers (lines before the first empty line)
            let abs_idx = self.scroll + i;
            let is_header = abs_idx < self.lines.len()
                && self.lines[..abs_idx].iter().all(|l| !l.is_empty());

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
        let status = if self.pending_reply_all.is_some() {
            format!(
                " line {}/{} | Loading reply data... | q:back",
                self.scroll + 1,
                total_lines
            )
        } else {
            format!(
                " line {}/{} | q:back n/j:down p/k:up r:reply R:reply-all c:compose ?:help",
                self.scroll + 1,
                total_lines
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
            Key::Char('c') => {
                let draft = compose::build_compose_draft(&self.from_address);
                ViewAction::Compose(draft)
            }
            Key::Char('?') => ViewAction::Push(Box::new(HelpView::new())),
            _ => ViewAction::Continue,
        }
    }

    fn on_response(&mut self, response: &BackendResponse) -> bool {
        match response {
            BackendResponse::EmailBody { id, result } if *id == self.email_id => {
                self.loading = false;
                match result.as_ref() {
                    Ok(email) => {
                        self.lines = Self::render_email(email);
                        self.email = Some(email.clone());
                        self.error = None;
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
                            let draft = compose::build_reply_draft(
                                email,
                                reply_all,
                                &self.from_address,
                            );
                            self.pending_compose = Some(draft);
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to load reply data: {}", e));
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn take_pending_action(&mut self) -> Option<ViewAction> {
        self.pending_compose
            .take()
            .map(ViewAction::Compose)
    }
}
