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
        search_query: Option<String>,
    },
    GetEmail {
        id: String,
    },
    GetEmailForReply {
        id: String,
    },
    MarkEmailRead {
        id: String,
    },
    MarkEmailUnread {
        id: String,
    },
    SetEmailFlagged {
        id: String,
        flagged: bool,
    },
    MoveEmail {
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
    },
    EmailBody {
        id: String,
        result: Box<Result<Email, String>>,
    },
    EmailForReply {
        id: String,
        result: Box<Result<Email, String>>,
    },
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
                let result = client
                    .get_mailboxes()
                    .map_err(|e| e.to_string());
                let _ = resp_tx.send(BackendResponse::Mailboxes(result));
            }
            BackendCommand::QueryEmails {
                mailbox_id,
                page_size,
                search_query,
            } => {
                let result = (|| {
                    let query = client
                        .query_emails(&mailbox_id, page_size, 0, search_query.as_deref())
                        .map_err(|e| e.to_string())?;
                    let total = query.total;
                    let emails = if query.ids.is_empty() {
                        Ok(Vec::new())
                    } else {
                        client.get_emails(&query.ids).map_err(|e| e.to_string())
                    }?;
                    Ok((emails, total))
                })();

                let (emails, total) = match result {
                    Ok((emails, total)) => (Ok(emails), total),
                    Err(e) => (Err(e), None),
                };

                let _ = resp_tx.send(BackendResponse::Emails {
                    mailbox_id,
                    emails,
                    total,
                });
            }
            BackendCommand::GetEmail { id } => {
                let result = client
                    .get_email(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|opt| {
                        opt.ok_or_else(|| "Email not found".to_string())
                    });

                let _ = resp_tx.send(BackendResponse::EmailBody {
                    id,
                    result: Box::new(result),
                });
            }
            BackendCommand::GetEmailForReply { id } => {
                let result = client
                    .get_email_for_reply(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|opt| {
                        opt.ok_or_else(|| "Email not found".to_string())
                    });

                let _ = resp_tx.send(BackendResponse::EmailForReply {
                    id,
                    result: Box::new(result),
                });
            }
            BackendCommand::MarkEmailRead { id } => {
                if let Err(e) = client.mark_email_read(&id) {
                    log_warn!("Failed to mark email {} as read: {}", id, e);
                }
            }
            BackendCommand::MarkEmailUnread { id } => {
                if let Err(e) = client.mark_email_unread(&id) {
                    log_warn!("Failed to mark email {} as unread: {}", id, e);
                }
            }
            BackendCommand::SetEmailFlagged { id, flagged } => {
                if let Err(e) = client.set_email_flagged(&id, flagged) {
                    log_warn!("Failed to set email {} flagged={}: {}", id, flagged, e);
                }
            }
            BackendCommand::MoveEmail { id, to_mailbox_id } => {
                if let Err(e) = client.move_email(&id, &to_mailbox_id) {
                    log_warn!("Failed to move email {}: {}", id, e);
                }
            }
            BackendCommand::Shutdown => {
                break;
            }
        }
    }
}
