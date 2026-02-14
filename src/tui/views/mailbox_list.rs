use crate::tui::input::Key;
use crate::tui::screen::Terminal;
use crate::tui::views::{View, ViewAction};
use std::io;

pub struct MailboxListView {
    items: Vec<String>,
    cursor: usize,
}

impl MailboxListView {
    pub fn new() -> Self {
        MailboxListView {
            items: vec![
                "INBOX (5 unread)".to_string(),
                "Drafts".to_string(),
                "Sent".to_string(),
                "Trash".to_string(),
                "Archive".to_string(),
            ],
            cursor: 0,
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

        // Mailbox list
        let max_items = (term.rows as usize).saturating_sub(4); // header + sep + bottom status
        for (i, item) in self.items.iter().enumerate().take(max_items) {
            term.move_to(3 + i as u16, 1)?;
            if i == self.cursor {
                term.set_reverse()?;
            }
            term.write_truncated(item, term.cols)?;
            if i == self.cursor {
                term.reset_attr()?;
            }
        }

        // Status bar at bottom
        term.move_to(term.rows, 1)?;
        term.set_reverse()?;
        let status = format!(
            " {}/{} | q:quit n/p:navigate RET:open g:refresh",
            self.cursor + 1,
            self.items.len()
        );
        term.write_truncated(&status, term.cols)?;
        // Fill rest of status bar
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
                if self.cursor + 1 < self.items.len() {
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
                // TODO: Push email list view for selected mailbox
                ViewAction::Continue
            }
            Key::Char('g') => {
                // TODO: Refresh
                ViewAction::Continue
            }
            _ => ViewAction::Continue,
        }
    }
}
