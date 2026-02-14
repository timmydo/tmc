pub mod input;
pub mod screen;
pub mod views;

use crate::backend::{self, BackendCommand};
use crate::jmap::client::JmapClient;
use input::read_key;
use screen::Terminal;
use std::io;
use views::mailbox_list::MailboxListView;
use views::{ViewAction, ViewStack};

pub fn run(client: JmapClient, page_size: u32) -> io::Result<()> {
    let (cmd_tx, resp_rx) = backend::spawn(client);
    let mut term = Terminal::new()?;

    let mailbox_view = MailboxListView::new(cmd_tx.clone(), page_size);
    // Request initial mailbox fetch
    let _ = cmd_tx.send(BackendCommand::FetchMailboxes);

    let mut stack = ViewStack::new(Box::new(mailbox_view));

    // Initial render
    stack.render_current(&mut term)?;

    loop {
        // Check for terminal resize
        if term.check_resize() {
            stack.render_current(&mut term)?;
        }

        // Poll backend responses (non-blocking)
        let mut needs_render = false;
        while let Ok(response) = resp_rx.try_recv() {
            if stack.handle_response(&response) {
                needs_render = true;
            }
        }
        if needs_render {
            stack.render_current(&mut term)?;
        }

        // Read input
        if let Some(key) = read_key() {
            let action = match stack.handle_key(key) {
                Some(action) => action,
                None => break,
            };

            match action {
                ViewAction::Continue => {
                    stack.render_current(&mut term)?;
                }
                ViewAction::Push(new_view) => {
                    stack.push(new_view);
                    stack.render_current(&mut term)?;
                }
                ViewAction::Pop => {
                    if !stack.pop() {
                        break;
                    }
                    stack.render_current(&mut term)?;
                }
                ViewAction::Quit => {
                    break;
                }
            }
        }
    }

    // Signal backend to shut down
    let _ = cmd_tx.send(BackendCommand::Shutdown);

    Ok(())
}
