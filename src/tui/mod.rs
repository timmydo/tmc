pub mod input;
pub mod screen;
pub mod views;

use crate::jmap::client::JmapClient;
use input::read_key;
use screen::Terminal;
use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use views::mailbox_list::MailboxListView;
use views::{ViewAction, ViewStack};

pub fn run(client: JmapClient, page_size: u32) -> io::Result<()> {
    let client = Rc::new(RefCell::new(client));
    let mut term = Terminal::new()?;

    let mut mailbox_view = MailboxListView::new(Rc::clone(&client), page_size);
    mailbox_view.refresh();

    let mut stack = ViewStack::new(Box::new(mailbox_view));

    // Initial render
    stack.render_current(&mut term)?;

    loop {
        // Check for terminal resize
        if term.check_resize() {
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

    Ok(())
}
