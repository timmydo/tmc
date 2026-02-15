use crate::backend::{BackendCommand, BackendResponse};
use crate::compose;
use crate::jmap::types::Mailbox;
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::email_list::EmailListView;
use crate::tui::views::help::HelpView;
use crate::tui::views::{View, ViewAction};
use std::io;
use std::sync::mpsc;

pub struct MailboxListView {
    cmd_tx: mpsc::Sender<BackendCommand>,
    from_address: String,
    page_size: u32,
    mailboxes: Vec<Mailbox>,
    cursor: usize,
    loading: bool,
    error: Option<String>,
    account_names: Vec<String>,
    current_account: String,
    pending_click: bool,
}

impl MailboxListView {
    pub fn new(
        cmd_tx: mpsc::Sender<BackendCommand>,
        from_address: String,
        page_size: u32,
        account_names: Vec<String>,
        current_account: String,
    ) -> Self {
        MailboxListView {
            cmd_tx,
            from_address,
            page_size,
            mailboxes: Vec::new(),
            cursor: 0,
            loading: true,
            error: None,
            account_names,
            current_account,
            pending_click: false,
        }
    }

    fn request_refresh(&mut self) {
        self.loading = true;
        let _ = self.cmd_tx.send(BackendCommand::FetchMailboxes);
    }

    fn next_account_name(&self) -> Option<String> {
        if self.account_names.len() <= 1 {
            return None;
        }
        let current_idx = self
            .account_names
            .iter()
            .position(|n| n == &self.current_account)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.account_names.len();
        Some(self.account_names[next_idx].clone())
    }

    fn sort_mailboxes(mailboxes: &mut [Mailbox]) {
        mailboxes.sort_by(|a, b| {
            let rank = |m: &Mailbox| -> u32 {
                match m.role.as_deref() {
                    Some("inbox") => 0,
                    Some("drafts") => 1,
                    Some("sent") => 2,
                    Some("junk") => 3,
                    Some("trash") => 4,
                    Some("archive") => 5,
                    Some(_) => 6,
                    None => 7,
                }
            };
            let ra = rank(a);
            let rb = rank(b);
            if ra != rb {
                ra.cmp(&rb)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });
    }

    fn format_mailbox(m: &Mailbox) -> String {
        if m.unread_emails > 0 {
            format!("{} ({}/{})", m.name, m.unread_emails, m.total_emails)
        } else if m.total_emails > 0 {
            format!("{} ({})", m.name, m.total_emails)
        } else {
            m.name.clone()
        }
    }
}

impl View for MailboxListView {
    fn render(&self, term: &mut Terminal) -> io::Result<()> {
        term.clear()?;

        // Header
        term.move_to(1, 1)?;
        term.set_bold()?;
        let header = if self.account_names.len() > 1 {
            format!("tmc - {}", self.current_account)
        } else {
            "tmc - Timmy's Mail Console".to_string()
        };
        term.write_truncated(&header, term.cols)?;
        term.reset_attr()?;

        // Separator
        term.move_to(2, 1)?;
        let sep = "-".repeat(term.cols as usize);
        term.write_str(&sep)?;

        if self.loading && self.mailboxes.is_empty() {
            term.move_to(3, 1)?;
            term.write_truncated("Loading mailboxes...", term.cols)?;
        } else if let Some(ref err) = self.error {
            term.move_to(3, 1)?;
            term.write_truncated(err, term.cols)?;
        } else if self.mailboxes.is_empty() {
            term.move_to(3, 1)?;
            term.write_truncated("No mailboxes found.", term.cols)?;
        } else {
            let max_items = (term.rows as usize).saturating_sub(4);
            let scroll_offset = if self.cursor >= max_items {
                self.cursor - max_items + 1
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
                let row = 3 + i as u16;
                term.move_to(row, 1)?;

                let display_idx = scroll_offset + i;
                let line = Self::format_mailbox(mailbox);

                if display_idx == self.cursor {
                    term.set_reverse()?;
                    if mailbox.unread_emails > 0 {
                        term.set_bold()?;
                    }
                } else if mailbox.unread_emails > 0 {
                    term.set_bold()?;
                }

                term.write_truncated(&line, term.cols)?;
                term.reset_attr()?;
            }
        }

        // Status bar
        term.move_to(term.rows, 1)?;
        term.set_reverse()?;
        let account_hint = if self.account_names.len() > 1 {
            " a:account"
        } else {
            ""
        };
        let status = if self.loading {
            format!(" Loading... | q:quit{}", account_hint)
        } else if self.mailboxes.is_empty() {
            format!(" q:quit g:refresh{}", account_hint)
        } else {
            format!(
                " {}/{} | q:quit n/p:navigate RET:open g:refresh c:compose ?:help{}",
                self.cursor + 1,
                self.mailboxes.len(),
                account_hint,
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
        let page = (term_rows as usize).saturating_sub(4);
        match key {
            Key::Char('q') => ViewAction::Quit,
            Key::Char('n') | Key::Char('j') | Key::Down => {
                if !self.mailboxes.is_empty() && self.cursor + 1 < self.mailboxes.len() {
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
                if !self.mailboxes.is_empty() {
                    self.cursor = (self.cursor + page).min(self.mailboxes.len() - 1);
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
                if !self.mailboxes.is_empty() {
                    self.cursor = self.mailboxes.len() - 1;
                }
                ViewAction::Continue
            }
            Key::Enter => {
                if let Some(mailbox) = self.mailboxes.get(self.cursor) {
                    let view = EmailListView::new(
                        self.cmd_tx.clone(),
                        self.from_address.clone(),
                        mailbox.id.clone(),
                        mailbox.name.clone(),
                        self.page_size,
                    );
                    // Send the query command
                    let _ = self.cmd_tx.send(BackendCommand::QueryEmails {
                        mailbox_id: mailbox.id.clone(),
                        page_size: self.page_size,
                    });
                    ViewAction::Push(Box::new(view))
                } else {
                    ViewAction::Continue
                }
            }
            Key::Char('g') => {
                self.request_refresh();
                ViewAction::Continue
            }
            Key::Char('c') => {
                let draft = compose::build_compose_draft(&self.from_address);
                ViewAction::Compose(draft)
            }
            Key::Char('a') => {
                if let Some(next) = self.next_account_name() {
                    ViewAction::SwitchAccount(next)
                } else {
                    ViewAction::Continue
                }
            }
            Key::Char('?') => ViewAction::Push(Box::new(HelpView::new())),
            Key::ScrollUp => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                ViewAction::Continue
            }
            Key::ScrollDown => {
                if !self.mailboxes.is_empty() && self.cursor + 1 < self.mailboxes.len() {
                    self.cursor += 1;
                }
                ViewAction::Continue
            }
            Key::MouseClick { row, col: _ } => {
                if row >= 3 && !self.mailboxes.is_empty() {
                    let max_items = (term_rows as usize).saturating_sub(4);
                    let scroll_offset = if self.cursor >= max_items {
                        self.cursor - max_items + 1
                    } else {
                        0
                    };
                    let clicked = scroll_offset + (row - 3) as usize;
                    if clicked < self.mailboxes.len() {
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
            if let Some(mailbox) = self.mailboxes.get(self.cursor) {
                let view = EmailListView::new(
                    self.cmd_tx.clone(),
                    self.from_address.clone(),
                    mailbox.id.clone(),
                    mailbox.name.clone(),
                    self.page_size,
                );
                let _ = self.cmd_tx.send(BackendCommand::QueryEmails {
                    mailbox_id: mailbox.id.clone(),
                    page_size: self.page_size,
                });
                return Some(ViewAction::Push(Box::new(view)));
            }
        }
        None
    }

    fn on_response(&mut self, response: &BackendResponse) -> bool {
        match response {
            BackendResponse::Mailboxes(result) => {
                self.loading = false;
                match result {
                    Ok(mailboxes) => {
                        let mut mailboxes = mailboxes.clone();
                        Self::sort_mailboxes(&mut mailboxes);
                        self.mailboxes = mailboxes;
                        self.error = None;
                        if self.cursor >= self.mailboxes.len() && !self.mailboxes.is_empty() {
                            self.cursor = self.mailboxes.len() - 1;
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to fetch mailboxes: {}", e));
                    }
                }
                true
            }
            _ => false,
        }
    }
}
