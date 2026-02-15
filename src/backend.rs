use crate::jmap::client::JmapClient;
use crate::jmap::types::{Email, Mailbox};
use std::sync::mpsc;
use std::thread;

/// Commands sent from the UI thread to the backend thread.
pub enum BackendCommand {
    FetchMailboxes,
    QueryEmails {
        mailbox_id: String,
        page_size: u32,
        position: u32,
        search_query: Option<String>,
    },
    GetEmail {
        id: String,
    },
    GetEmailForReply {
        id: String,
    },
    MarkEmailRead {
        op_id: u64,
        id: String,
    },
    MarkEmailUnread {
        op_id: u64,
        id: String,
    },
    SetEmailFlagged {
        op_id: u64,
        id: String,
        flagged: bool,
    },
    MoveEmail {
        op_id: u64,
        id: String,
        to_mailbox_id: String,
    },
    Shutdown,
}

/// Responses sent from the backend thread to the UI thread.
pub enum BackendResponse {
    Mailboxes(Result<Vec<Mailbox>, String>),
    Emails {
        mailbox_id: String,
        emails: Result<Vec<Email>, String>,
        total: Option<u32>,
        position: u32,
        loaded: u32,
    },
    EmailBody {
        id: String,
        result: Box<Result<Email, String>>,
    },
    EmailForReply {
        id: String,
        result: Box<Result<Email, String>>,
    },
    EmailMutation {
        op_id: u64,
        id: String,
        action: EmailMutationAction,
        result: Result<(), String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmailMutationAction {
    MarkRead,
    MarkUnread,
    SetFlagged(bool),
    Move,
}

/// Spawn the backend thread. Returns the command sender and response receiver.
pub fn spawn(
    client: JmapClient,
) -> (
    mpsc::Sender<BackendCommand>,
    mpsc::Receiver<BackendResponse>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCommand>();
    let (resp_tx, resp_rx) = mpsc::channel::<BackendResponse>();

    thread::spawn(move || {
        backend_loop(client, cmd_rx, resp_tx);
    });

    (cmd_tx, resp_rx)
}

fn backend_loop(
    client: JmapClient,
    cmd_rx: mpsc::Receiver<BackendCommand>,
    resp_tx: mpsc::Sender<BackendResponse>,
) {
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            BackendCommand::FetchMailboxes => {
                let result = client.get_mailboxes().map_err(|e| e.to_string());
                let _ = resp_tx.send(BackendResponse::Mailboxes(result));
            }
            BackendCommand::QueryEmails {
                mailbox_id,
                page_size,
                position,
                search_query,
            } => {
                let result = (|| {
                    let query = client
                        .query_emails(&mailbox_id, page_size, position, search_query.as_deref())
                        .map_err(|e| e.to_string())?;
                    let total = query.total;
                    let position = query.position;
                    let loaded = query.ids.len() as u32;
                    let emails = if query.ids.is_empty() {
                        Ok(Vec::new())
                    } else {
                        client.get_emails(&query.ids).map_err(|e| e.to_string())
                    }?;
                    Ok((emails, total, position, loaded))
                })();

                let (emails, total, position, loaded) = match result {
                    Ok((emails, total, position, loaded)) => (Ok(emails), total, position, loaded),
                    Err(e) => (Err(e), None, position, 0),
                };

                let _ = resp_tx.send(BackendResponse::Emails {
                    mailbox_id,
                    emails,
                    total,
                    position,
                    loaded,
                });
            }
            BackendCommand::GetEmail { id } => {
                let result = client
                    .get_email(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|opt| opt.ok_or_else(|| "Email not found".to_string()));

                let _ = resp_tx.send(BackendResponse::EmailBody {
                    id,
                    result: Box::new(result),
                });
            }
            BackendCommand::GetEmailForReply { id } => {
                let result = client
                    .get_email_for_reply(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|opt| opt.ok_or_else(|| "Email not found".to_string()));

                let _ = resp_tx.send(BackendResponse::EmailForReply {
                    id,
                    result: Box::new(result),
                });
            }
            BackendCommand::MarkEmailRead { op_id, id } => {
                let result = client.mark_email_read(&id).map_err(|e| {
                    let msg = e.to_string();
                    log_warn!("Failed to mark email {} as read: {}", id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id,
                    action: EmailMutationAction::MarkRead,
                    result,
                });
            }
            BackendCommand::MarkEmailUnread { op_id, id } => {
                let result = client.mark_email_unread(&id).map_err(|e| {
                    let msg = e.to_string();
                    log_warn!("Failed to mark email {} as unread: {}", id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id,
                    action: EmailMutationAction::MarkUnread,
                    result,
                });
            }
            BackendCommand::SetEmailFlagged { op_id, id, flagged } => {
                let result = client.set_email_flagged(&id, flagged).map_err(|e| {
                    let msg = e.to_string();
                    log_warn!("Failed to set email {} flagged={}: {}", id, flagged, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id,
                    action: EmailMutationAction::SetFlagged(flagged),
                    result,
                });
            }
            BackendCommand::MoveEmail {
                op_id,
                id,
                to_mailbox_id,
            } => {
                let result = client.move_email(&id, &to_mailbox_id).map_err(|e| {
                    let msg = e.to_string();
                    log_warn!("Failed to move email {}: {}", id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id,
                    action: EmailMutationAction::Move,
                    result,
                });
            }
            BackendCommand::Shutdown => {
                break;
            }
        }
    }
}
