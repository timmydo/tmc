use crate::jmap::client::JmapClient;
use crate::jmap::types::{Email, Mailbox};
use std::collections::HashMap;
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
    QueryThreadEmails {
        thread_id: String,
    },
    MarkThreadRead {
        thread_id: String,
        email_ids: Vec<String>,
    },
    DownloadAttachment {
        blob_id: String,
        name: String,
        content_type: String,
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
        thread_counts: HashMap<String, (usize, usize)>,
    },
    ThreadEmails {
        thread_id: String,
        emails: Result<Vec<Email>, String>,
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
    ThreadMarkedRead {
        #[allow(dead_code)]
        thread_id: String,
        #[allow(dead_code)]
        result: Result<(), String>,
    },
    AttachmentDownloaded {
        name: String,
        result: Result<std::path::PathBuf, String>,
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

                    // Build thread counts map (unread, total)
                    let mut thread_counts = HashMap::new();
                    let thread_ids: Vec<String> =
                        emails.iter().filter_map(|e| e.thread_id.clone()).collect();
                    if !thread_ids.is_empty() {
                        if let Ok(threads) = client.get_threads(&thread_ids) {
                            let all_email_ids: Vec<String> =
                                threads.iter().flat_map(|t| t.email_ids.clone()).collect();
                            let keyword_emails = client
                                .get_email_keywords(&all_email_ids)
                                .unwrap_or_default();
                            let keyword_map: HashMap<String, bool> = keyword_emails
                                .iter()
                                .map(|e| (e.id.clone(), e.keywords.contains_key("$seen")))
                                .collect();
                            for thread in threads {
                                let total_count = thread.email_ids.len();
                                let unread_count = thread
                                    .email_ids
                                    .iter()
                                    .filter(|id| !keyword_map.get(*id).copied().unwrap_or(true))
                                    .count();
                                thread_counts
                                    .insert(thread.id.clone(), (unread_count, total_count));
                            }
                        }
                    }

                    Ok((emails, total, position, loaded, thread_counts))
                })();

                let (emails, total, position, loaded, thread_counts) = match result {
                    Ok((emails, total, position, loaded, thread_counts)) => {
                        (Ok(emails), total, position, loaded, thread_counts)
                    }
                    Err(e) => (Err(e), None, position, 0, HashMap::new()),
                };

                let _ = resp_tx.send(BackendResponse::Emails {
                    mailbox_id,
                    emails,
                    total,
                    position,
                    loaded,
                    thread_counts,
                });
            }
            BackendCommand::QueryThreadEmails { thread_id } => {
                let result = client
                    .query_thread_emails(&thread_id)
                    .map_err(|e| e.to_string());
                let _ = resp_tx.send(BackendResponse::ThreadEmails {
                    thread_id,
                    emails: result,
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
            BackendCommand::MarkThreadRead {
                thread_id,
                email_ids,
            } => {
                let result = client
                    .mark_emails_read(&email_ids)
                    .map_err(|e| e.to_string());
                let _ = resp_tx.send(BackendResponse::ThreadMarkedRead { thread_id, result });
            }
            BackendCommand::DownloadAttachment {
                blob_id,
                name,
                content_type,
            } => {
                let result = (|| {
                    let bytes = client
                        .download_blob(&blob_id, &name, &content_type)
                        .map_err(|e| e.to_string())?;

                    let dir = std::env::temp_dir().join("tmc-attachments");
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| format!("Failed to create temp dir: {}", e))?;

                    let path = dir.join(&name);
                    std::fs::write(&path, &bytes)
                        .map_err(|e| format!("Failed to write file: {}", e))?;

                    log_info!(
                        "[Backend] Attachment saved: {} ({} bytes)",
                        path.display(),
                        bytes.len()
                    );
                    Ok(path)
                })();

                let _ = resp_tx.send(BackendResponse::AttachmentDownloaded { name, result });
            }
            BackendCommand::Shutdown => {
                break;
            }
        }
    }
}
