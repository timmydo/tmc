use crate::jmap::client::JmapClient;
use crate::jmap::types::Mailbox;
use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::email_list::EmailListView;
use crate::tui::views::{View, ViewAction};
use std::cell::RefCell;
use std::io;
use std::rc::Rc;

pub struct MailboxListView {
    client: Rc<RefCell<JmapClient>>,
    page_size: u32,
    mailboxes: Vec<Mailbox>,
    cursor: usize,
    error: Option<String>,
}

impl MailboxListView {
    pub fn new(client: Rc<RefCell<JmapClient>>, page_size: u32) -> Self {
        MailboxListView {
            client,
            page_size,
            mailboxes: Vec::new(),
            cursor: 0,
            error: None,
        }
    }

    pub fn refresh(&mut self) {
        match self.client.borrow().get_mailboxes() {
            Ok(mut mailboxes) => {
                // Sort: role-based mailboxes first (inbox, drafts, sent, trash, archive),
                // then alphabetically
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
        term.write_truncated("tmc - Timmy's Mail Console", term.cols)?;
        term.reset_attr()?;

        // Separator
        term.move_to(2, 1)?;
        let sep = "-".repeat(term.cols as usize);
        term.write_str(&sep)?;

        if let Some(ref err) = self.error {
            term.move_to(3, 1)?;
            term.write_truncated(err, term.cols)?;
        } else if self.mailboxes.is_empty() {
            term.move_to(3, 1)?;
            term.write_truncated("No mailboxes found.", term.cols)?;
        } else {
            let max_items = (term.rows as usize).saturating_sub(4);
            // Scrolling: keep cursor visible
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
        let status = if self.mailboxes.is_empty() {
            " q:quit g:refresh".to_string()
        } else {
            format!(
                " {}/{} | q:quit n/p:navigate RET:open g:refresh",
                self.cursor + 1,
                self.mailboxes.len()
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

    fn handle_key(&mut self, key: Key) -> ViewAction {
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
            Key::Enter => {
                if let Some(mailbox) = self.mailboxes.get(self.cursor) {
                    let mut view = EmailListView::new(
                        Rc::clone(&self.client),
                        mailbox.id.clone(),
                        mailbox.name.clone(),
                        self.page_size,
                    );
                    view.refresh();
                    ViewAction::Push(Box::new(view))
                } else {
                    ViewAction::Continue
                }
            }
            Key::Char('g') => {
                self.refresh();
                ViewAction::Continue
            }
            _ => ViewAction::Continue,
        }
    }
}
