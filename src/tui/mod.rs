pub mod input;
pub mod screen;
pub mod views;

use crate::backend::{self, BackendCommand};
use crate::compose;
use crate::jmap::client::JmapClient;
use input::read_key;
use screen::Terminal;
use std::io;
use views::mailbox_list::MailboxListView;
use views::{ViewAction, ViewStack};

pub fn run(
    client: JmapClient,
    page_size: u32,
    editor: Option<String>,
    username: String,
) -> io::Result<()> {
    let (cmd_tx, resp_rx) = backend::spawn(client);
    let mut term = Terminal::new()?;

    let mailbox_view = MailboxListView::new(cmd_tx.clone(), username.clone(), page_size);
    // Request initial mailbox fetch
    let _ = cmd_tx.send(BackendCommand::FetchMailboxes);

    let mut stack = ViewStack::new(Box::new(mailbox_view));

    // Determine editor command
    let editor_cmd = editor
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_string());

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

            // Check for pending actions from views (e.g., reply compose)
            if let Some(view) = stack.current_mut() {
                if let Some(ViewAction::Compose(draft_text)) = view.take_pending_action() {
                    spawn_editor(&draft_text, &editor_cmd);
                    needs_render = true;
                }
            }
        }
        if needs_render {
            stack.render_current(&mut term)?;
        }

        // Read input
        if let Some(key) = read_key() {
            let action = match stack.handle_key(key, term.rows) {
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
                ViewAction::Compose(draft_text) => {
                    spawn_editor(&draft_text, &editor_cmd);
                    stack.render_current(&mut term)?;
                }
            }
        }
    }

    // Signal backend to shut down
    let _ = cmd_tx.send(BackendCommand::Shutdown);

    Ok(())
}

fn spawn_editor(draft_text: &str, editor_cmd: &str) {
    // Write draft to temp file
    let temp_path = match compose::write_temp_file(draft_text) {
        Ok(path) => path,
        Err(e) => {
            crate::log_error!("Failed to create temp file: {}", e);
            return;
        }
    };

    // Spawn editor as a separate process
    let path_str = temp_path.display().to_string();
    let child = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} {}", editor_cmd, path_str))
        .spawn();

    match child {
        Ok(mut child) => {
            // Background thread waits for editor exit then cleans up temp file
            std::thread::spawn(move || {
                let _ = child.wait();
                let _ = std::fs::remove_file(&temp_path);
            });
        }
        Err(e) => {
            crate::log_error!("Failed to spawn editor: {}", e);
            let _ = std::fs::remove_file(&temp_path);
        }
    }
}
