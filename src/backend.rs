use crate::config::RetentionPolicyConfig;
use crate::jmap::client::JmapClient;
use crate::jmap::types::{Email, Mailbox};
use crate::rules::{self, CompiledRule};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

/// Commands sent from the UI thread to the backend thread.
pub enum BackendCommand {
    FetchMailboxes,
    CreateMailbox {
        name: String,
    },
    DeleteMailbox {
        id: String,
        name: String,
    },
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
    MoveThread {
        op_id: u64,
        thread_id: String,
        to_mailbox_id: String,
    },
    DestroyEmail {
        op_id: u64,
        id: String,
    },
    DestroyThread {
        op_id: u64,
        thread_id: String,
    },
    QueryThreadEmails {
        thread_id: String,
    },
    MarkThreadRead {
        thread_id: String,
        email_ids: Vec<String>,
    },
    GetEmailRawHeaders {
        id: String,
    },
    DownloadAttachment {
        blob_id: String,
        name: String,
        content_type: String,
    },
    PreviewRetentionExpiry {
        policies: Vec<RetentionPolicyConfig>,
    },
    ExecuteRetentionExpiry {
        policies: Vec<RetentionPolicyConfig>,
    },
    PreviewRulesForMailbox {
        mailbox_id: String,
        mailbox_name: String,
        loaded_email_ids: Vec<String>,
    },
    RunRulesForMailbox {
        mailbox_id: String,
        mailbox_name: String,
        loaded_email_ids: Vec<String>,
    },
    Shutdown,
}

/// Responses sent from the backend thread to the UI thread.
pub enum BackendResponse {
    Mailboxes(Result<Vec<Mailbox>, String>),
    MailboxCreated {
        name: String,
        result: Result<(), String>,
    },
    MailboxDeleted {
        name: String,
        result: Result<(), String>,
    },
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
    EmailRawHeaders {
        id: String,
        result: Result<String, String>,
    },
    AttachmentDownloaded {
        name: String,
        result: Result<std::path::PathBuf, String>,
    },
    RetentionPreview {
        result: Result<RetentionPreviewResult, String>,
    },
    RetentionExecuted {
        result: Result<RetentionExecutionResult, String>,
    },
    RulesDryRun {
        mailbox_id: String,
        mailbox_name: String,
        result: Result<RulesDryRunResult, String>,
    },
    RulesRun {
        mailbox_id: String,
        mailbox_name: String,
        result: Result<RulesRunResult, String>,
    },
}

#[derive(Clone, Debug)]
pub struct RetentionCandidate {
    pub id: String,
    pub mailbox: String,
    pub policy: String,
    pub received_at: String,
    pub from: String,
    pub subject: String,
}

#[derive(Clone, Debug)]
pub struct RetentionPreviewResult {
    pub candidates: Vec<RetentionCandidate>,
}

#[derive(Clone, Debug)]
pub struct RetentionExecutionResult {
    pub deleted: usize,
    pub failed_batches: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RulesRunResult {
    pub scanned: usize,
    pub matched_rules: usize,
    pub actions: usize,
}

#[derive(Clone, Debug)]
pub struct RulesDryRunEntry {
    pub received_at: String,
    pub from: String,
    pub subject: String,
    pub rule_name: String,
    pub actions: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RulesDryRunResult {
    pub scanned: usize,
    pub matched_rules: usize,
    pub actions: usize,
    pub entries: Vec<RulesDryRunEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmailMutationAction {
    MarkRead,
    MarkUnread,
    SetFlagged(bool),
    Move,
    Destroy,
}

/// Spawn the backend thread. Returns the command sender and response receiver.
pub fn spawn(
    client: JmapClient,
    rules: Arc<Vec<CompiledRule>>,
    custom_headers: Arc<Vec<String>>,
    rules_mailbox_regex: Arc<Regex>,
) -> (
    mpsc::Sender<BackendCommand>,
    mpsc::Receiver<BackendResponse>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCommand>();
    let (resp_tx, resp_rx) = mpsc::channel::<BackendResponse>();

    thread::spawn(move || {
        backend_loop(
            client,
            cmd_rx,
            resp_tx,
            rules,
            custom_headers,
            rules_mailbox_regex,
        );
    });

    (cmd_tx, resp_rx)
}

fn backend_loop(
    client: JmapClient,
    cmd_rx: mpsc::Receiver<BackendCommand>,
    resp_tx: mpsc::Sender<BackendResponse>,
    rules: Arc<Vec<CompiledRule>>,
    custom_headers: Arc<Vec<String>>,
    rules_mailbox_regex: Arc<Regex>,
) {
    let mut cached_mailboxes: Vec<Mailbox> = Vec::new();

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            BackendCommand::FetchMailboxes => {
                let result = client.get_mailboxes().map_err(|e| e.to_string());
                if let Ok(ref mailboxes) = result {
                    cached_mailboxes = mailboxes.clone();
                }
                let _ = resp_tx.send(BackendResponse::Mailboxes(result));
            }
            BackendCommand::CreateMailbox { name } => {
                let result = client.create_mailbox(&name).map_err(|e| e.to_string());
                if result.is_ok() {
                    if let Ok(mailboxes) = client.get_mailboxes() {
                        cached_mailboxes = mailboxes;
                    }
                }
                let _ = resp_tx.send(BackendResponse::MailboxCreated { name, result });
            }
            BackendCommand::DeleteMailbox { id, name } => {
                let result = client.delete_mailbox(&id).map_err(|e| e.to_string());
                if result.is_ok() {
                    if let Ok(mailboxes) = client.get_mailboxes() {
                        cached_mailboxes = mailboxes;
                    }
                }
                let _ = resp_tx.send(BackendResponse::MailboxDeleted { name, result });
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
                        fetch_emails_chunked(&client, &query.ids, &custom_headers)
                    }?;

                    // Apply filtering rules
                    if !rules.is_empty() {
                        let mailbox_name = cached_mailboxes
                            .iter()
                            .find(|m| m.id.as_str() == mailbox_id.as_str())
                            .map(|m| m.name.as_str())
                            .unwrap_or("");
                        if rules_mailbox_regex.is_match(mailbox_name) {
                            let applications =
                                rules::apply_rules(&rules, &emails, &cached_mailboxes);
                            if !applications.is_empty() {
                                log_info!(
                                    "[Rules] Applying {} rule action(s) to fetched emails in mailbox '{}'",
                                    applications.len(),
                                    mailbox_name
                                );
                                rules::execute_rule_actions(
                                    &applications,
                                    &cached_mailboxes,
                                    &client,
                                );
                            }
                        } else {
                            log_debug!(
                                "[Rules] Skipping auto-run for mailbox '{}' (does not match regex '{}')",
                                mailbox_name,
                                rules_mailbox_regex.as_str()
                            );
                        }
                    }

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
            BackendCommand::MoveThread {
                op_id,
                thread_id,
                to_mailbox_id,
            } => {
                let result = (|| {
                    let emails = client
                        .query_thread_emails(&thread_id)
                        .map_err(|e| e.to_string())?;
                    for email in emails {
                        client
                            .move_email(&email.id, &to_mailbox_id)
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(())
                })()
                .map_err(|msg| {
                    log_warn!("Failed to move thread {}: {}", thread_id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id: thread_id,
                    action: EmailMutationAction::Move,
                    result,
                });
            }
            BackendCommand::DestroyEmail { op_id, id } => {
                let ids = vec![id.clone()];
                let result = client.destroy_emails(&ids).map_err(|e| {
                    let msg = e.to_string();
                    log_warn!("Failed to destroy email {}: {}", id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id,
                    action: EmailMutationAction::Destroy,
                    result,
                });
            }
            BackendCommand::DestroyThread { op_id, thread_id } => {
                let result = (|| {
                    let emails = client
                        .query_thread_emails(&thread_id)
                        .map_err(|e| e.to_string())?;
                    let ids: Vec<String> = emails.into_iter().map(|e| e.id).collect();
                    client.destroy_emails(&ids).map_err(|e| e.to_string())
                })()
                .map_err(|msg| {
                    log_warn!("Failed to destroy thread {}: {}", thread_id, msg);
                    msg
                });
                let _ = resp_tx.send(BackendResponse::EmailMutation {
                    op_id,
                    id: thread_id,
                    action: EmailMutationAction::Destroy,
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
            BackendCommand::GetEmailRawHeaders { id } => {
                let result = client
                    .get_email_raw(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|opt| opt.ok_or_else(|| "Email not found".to_string()))
                    .map(|raw| {
                        // Extract just the headers (everything before the first blank line)
                        if let Some(pos) = raw.find("\r\n\r\n") {
                            raw[..pos].to_string()
                        } else if let Some(pos) = raw.find("\n\n") {
                            raw[..pos].to_string()
                        } else {
                            raw
                        }
                    });
                let _ = resp_tx.send(BackendResponse::EmailRawHeaders { id, result });
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
            BackendCommand::PreviewRetentionExpiry { policies } => {
                let result = collect_retention_candidates(&client, &cached_mailboxes, &policies)
                    .map(|candidates| RetentionPreviewResult { candidates });
                let _ = resp_tx.send(BackendResponse::RetentionPreview { result });
            }
            BackendCommand::ExecuteRetentionExpiry { policies } => {
                let result = execute_retention_expiry(&client, &cached_mailboxes, &policies);
                let _ = resp_tx.send(BackendResponse::RetentionExecuted { result });
            }
            BackendCommand::PreviewRulesForMailbox {
                mailbox_id,
                mailbox_name,
                loaded_email_ids,
            } => {
                let result = preview_rules_for_mailbox(
                    &client,
                    &cached_mailboxes,
                    &rules,
                    &custom_headers,
                    &loaded_email_ids,
                );
                let _ = resp_tx.send(BackendResponse::RulesDryRun {
                    mailbox_id,
                    mailbox_name,
                    result,
                });
            }
            BackendCommand::RunRulesForMailbox {
                mailbox_id,
                mailbox_name,
                loaded_email_ids,
            } => {
                let result = run_rules_for_mailbox(
                    &client,
                    &cached_mailboxes,
                    &rules,
                    &custom_headers,
                    &loaded_email_ids,
                );
                let _ = resp_tx.send(BackendResponse::RulesRun {
                    mailbox_id,
                    mailbox_name,
                    result,
                });
            }
            BackendCommand::Shutdown => {
                break;
            }
        }
    }
}

const EMAIL_GET_CHUNK_SIZE: usize = 100;

fn fetch_emails_chunked(
    client: &JmapClient,
    ids: &[String],
    custom_headers: &[String],
) -> Result<Vec<Email>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(EMAIL_GET_CHUNK_SIZE) {
        let mut batch = if custom_headers.is_empty() {
            client.get_emails(chunk)
        } else {
            client.get_emails_with_extra_properties(chunk, custom_headers)
        }
        .map_err(|e| e.to_string())?;
        out.append(&mut batch);
    }
    Ok(out)
}

fn run_rules_for_mailbox(
    client: &JmapClient,
    mailboxes: &[Mailbox],
    rules: &[CompiledRule],
    custom_headers: &[String],
    loaded_email_ids: &[String],
) -> Result<RulesRunResult, String> {
    if rules.is_empty() || loaded_email_ids.is_empty() {
        return Ok(RulesRunResult {
            scanned: 0,
            matched_rules: 0,
            actions: 0,
        });
    }

    let emails = fetch_emails_chunked(client, loaded_email_ids, custom_headers)?;
    let scanned = emails.len();
    let applications = rules::apply_rules(rules, &emails, mailboxes);
    let matched_rules = applications.len();
    let actions = applications.iter().map(|a| a.actions.len()).sum::<usize>();
    if !applications.is_empty() {
        rules::execute_rule_actions(&applications, mailboxes, client);
    }

    Ok(RulesRunResult {
        scanned,
        matched_rules,
        actions,
    })
}

fn preview_rules_for_mailbox(
    client: &JmapClient,
    mailboxes: &[Mailbox],
    rules: &[CompiledRule],
    custom_headers: &[String],
    loaded_email_ids: &[String],
) -> Result<RulesDryRunResult, String> {
    if rules.is_empty() || loaded_email_ids.is_empty() {
        return Ok(RulesDryRunResult {
            scanned: 0,
            matched_rules: 0,
            actions: 0,
            entries: Vec::new(),
        });
    }

    let emails = fetch_emails_chunked(client, loaded_email_ids, custom_headers)?;
    let scanned = emails.len();
    let mut entries = Vec::new();
    let email_by_id: HashMap<String, &Email> = emails.iter().map(|e| (e.id.clone(), e)).collect();
    let applications = rules::apply_rules(rules, &emails, mailboxes);
    let matched_rules = applications.len();
    let actions = applications.iter().map(|a| a.actions.len()).sum::<usize>();

    for app in applications {
        if let Some(email) = email_by_id.get(&app.email_id) {
            let from = email
                .from
                .as_ref()
                .and_then(|f| f.first())
                .map(|a| a.to_string())
                .unwrap_or_else(|| "(unknown)".to_string());
            let received_at = email
                .received_at
                .as_deref()
                .map(|d| d.chars().take(10).collect::<String>())
                .unwrap_or_else(|| "(unknown)".to_string());
            let subject = email
                .subject
                .clone()
                .unwrap_or_else(|| "(no subject)".to_string());
            let action_names = app.actions.iter().map(format_rule_action).collect();
            entries.push(RulesDryRunEntry {
                received_at,
                from,
                subject,
                rule_name: app.rule_name,
                actions: action_names,
            });
        }
    }

    Ok(RulesDryRunResult {
        scanned,
        matched_rules,
        actions,
        entries,
    })
}

fn format_rule_action(action: &rules::Action) -> String {
    match action {
        rules::Action::MarkRead => "mark_read".to_string(),
        rules::Action::MarkUnread => "mark_unread".to_string(),
        rules::Action::Flag => "flag".to_string(),
        rules::Action::Unflag => "unflag".to_string(),
        rules::Action::Move { target } => format!("move_to={}", target),
        rules::Action::Delete => "delete".to_string(),
    }
}

fn execute_retention_expiry(
    client: &JmapClient,
    mailboxes: &[Mailbox],
    policies: &[RetentionPolicyConfig],
) -> Result<RetentionExecutionResult, String> {
    let candidates = collect_retention_candidates(client, mailboxes, policies)?;
    let ids: Vec<String> = candidates.into_iter().map(|c| c.id).collect();
    let mut deleted = 0usize;
    let mut failed_batches = Vec::new();

    for chunk in ids.chunks(50) {
        match client.destroy_emails(chunk) {
            Ok(()) => {
                deleted += chunk.len();
            }
            Err(e) => {
                failed_batches.push(format!("{} IDs: {}", chunk.len(), e));
            }
        }
    }

    Ok(RetentionExecutionResult {
        deleted,
        failed_batches,
    })
}

fn collect_retention_candidates(
    client: &JmapClient,
    mailboxes: &[Mailbox],
    policies: &[RetentionPolicyConfig],
) -> Result<Vec<RetentionCandidate>, String> {
    if policies.is_empty() {
        return Ok(Vec::new());
    }

    let today_days = current_days_since_epoch()?;
    let mut candidates = Vec::new();
    let mut seen_email_ids = HashSet::new();

    for policy in policies {
        let Some(mailbox_id) = rules::resolve_mailbox_id(&policy.folder, mailboxes) else {
            log_warn!(
                "Retention policy '{}' skipped; cannot resolve folder '{}'",
                policy.name,
                policy.folder
            );
            continue;
        };
        let mailbox_name = mailboxes
            .iter()
            .find(|m| m.id == mailbox_id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| policy.folder.clone());
        let cutoff_days = today_days - (policy.days as i64);

        let mut position = 0u32;
        loop {
            let query = client
                .query_emails_uncollapsed(&mailbox_id, 500, position)
                .map_err(|e| e.to_string())?;
            if query.ids.is_empty() {
                break;
            }

            let emails = fetch_emails_chunked(client, &query.ids, &[])?;
            for email in emails {
                if !seen_email_ids.insert(email.id.clone()) {
                    continue;
                }
                let Some(received_days) = email_received_days(&email) else {
                    continue;
                };
                if received_days >= cutoff_days {
                    continue;
                }

                let from = email
                    .from
                    .as_ref()
                    .and_then(|f| f.first())
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "(unknown)".to_string());
                let received_at = email
                    .received_at
                    .as_deref()
                    .map(|d| d.chars().take(10).collect::<String>())
                    .unwrap_or_else(|| "(unknown)".to_string());

                candidates.push(RetentionCandidate {
                    id: email.id,
                    mailbox: mailbox_name.clone(),
                    policy: policy.name.clone(),
                    received_at,
                    from,
                    subject: email.subject.unwrap_or_else(|| "(no subject)".to_string()),
                });
            }

            let loaded = query.ids.len() as u32;
            position = query.position.saturating_add(loaded);
            if loaded == 0 {
                break;
            }
            if let Some(total) = query.total {
                if position >= total {
                    break;
                }
            }
        }
    }

    candidates.sort_by(|a, b| {
        a.received_at
            .cmp(&b.received_at)
            .then_with(|| a.mailbox.cmp(&b.mailbox))
            .then_with(|| a.subject.cmp(&b.subject))
    });
    Ok(candidates)
}

fn current_days_since_epoch() -> Result<i64, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock error: {}", e))?;
    Ok((now.as_secs() / 86_400) as i64)
}

fn email_received_days(email: &Email) -> Option<i64> {
    let received = email.received_at.as_deref()?;
    let y = received.get(0..4)?.parse::<i32>().ok()?;
    let m = received.get(5..7)?.parse::<u32>().ok()?;
    let d = received.get(8..10)?.parse::<u32>().ok()?;
    ymd_to_days_since_epoch(y, m, d)
}

// Convert calendar date to day index since Unix epoch.
fn ymd_to_days_since_epoch(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
