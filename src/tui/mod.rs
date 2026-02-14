pub mod input;
pub mod screen;
pub mod views;

use input::read_key;
use screen::Terminal;
use std::io;
use views::mailbox_list::MailboxListView;
use views::{ViewAction, ViewStack};

pub fn run() -> io::Result<()> {
    let mut term = Terminal::new()?;
    let mut stack = ViewStack::new(Box::new(MailboxListView::new()));

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
