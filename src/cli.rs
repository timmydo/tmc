use crate::backend::{self, BackendCommand, BackendResponse};
use crate::compose;
use crate::config::Config;
use crate::jmap::types::{Email, Mailbox};
use crate::keybindings;
use crate::rules::{self, CompiledRule};
use regex::Regex;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::{mpsc, Arc};

struct CliState {
    config: Config,
    cmd_tx: Option<mpsc::Sender<BackendCommand>>,
    resp_rx: Option<mpsc::Receiver<BackendResponse>>,
    connected_account: Option<String>,
    connected_username: Option<String>,
    cached_mailboxes: Vec<Mailbox>,
    next_op_id: u64,
    rules: Arc<Vec<CompiledRule>>,
    custom_headers: Arc<Vec<String>>,
    rules_mailbox_regex: Arc<Regex>,
    my_email_regex: Arc<Regex>,
    archive_folder: String,
    deleted_folder: String,
}

impl CliState {
    fn next_op_id(&mut self) -> u64 {
        self.next_op_id += 1;
        self.next_op_id
    }

    fn send_cmd(&self, cmd: BackendCommand) -> Result<(), String> {
        self.cmd_tx
            .as_ref()
            .ok_or_else(|| "not connected".to_string())?
            .send(cmd)
            .map_err(|_| "backend channel closed".to_string())
    }

    fn recv_resp(&self) -> Result<BackendResponse, String> {
        self.resp_rx
            .as_ref()
            .ok_or_else(|| "not connected".to_string())?
            .recv()
            .map_err(|_| "backend channel closed".to_string())
    }

    fn resolve_folder_id(&self, folder_name: &str) -> Option<String> {
        rules::resolve_mailbox_id(folder_name, &self.cached_mailboxes)
    }
}

fn ok_response(data: Value) -> Value {
    let mut obj = match data {
        Value::Object(m) => m,
        _ => {
            let mut m = serde_json::Map::new();
            m.insert("data".to_string(), data);
            m
        }
    };
    obj.insert("ok".to_string(), Value::Bool(true));
    Value::Object(obj)
}

fn err_response(msg: &str) -> Value {
    json!({"ok": false, "error": msg})
}

fn serialize_email(email: &Email, headers_only: bool, max_body_chars: usize) -> Value {
    let from = email.from.as_ref().map(|addrs| {
        addrs
            .iter()
            .map(|a| json!({"name": a.name, "email": a.email}))
            .collect::<Vec<_>>()
    });
    let to = email.to.as_ref().map(|addrs| {
        addrs
            .iter()
            .map(|a| json!({"name": a.name, "email": a.email}))
            .collect::<Vec<_>>()
    });
    let cc = email.cc.as_ref().map(|addrs| {
        addrs
            .iter()
            .map(|a| json!({"name": a.name, "email": a.email}))
            .collect::<Vec<_>>()
    });

    let is_read = email.keywords.contains_key("$seen");
    let is_flagged = email.keywords.contains_key("$flagged");

    let attachments: Option<Vec<Value>> = email.attachments.as_ref().map(|parts| {
        parts
            .iter()
            .map(|p| {
                json!({
                    "part_id": p.part_id,
                    "blob_id": p.blob_id,
                    "type": p.r#type,
                    "name": p.name,
                    "size": p.size,
                })
            })
            .collect()
    });

    let mut obj = json!({
        "id": email.id,
        "thread_id": email.thread_id,
        "from": from,
        "to": to,
        "cc": cc,
        "subject": email.subject,
        "received_at": email.received_at,
        "sent_at": email.sent_at,
        "is_read": is_read,
        "is_flagged": is_flagged,
        "mailbox_ids": email.mailbox_ids.keys().collect::<Vec<_>>(),
        "message_id": email.message_id,
        "references": email.references,
        "attachments": attachments,
    });

    if !headers_only {
        let body_text = compose::extract_body_text(email);
        let (body, truncated) = if max_body_chars > 0 && body_text.len() > max_body_chars {
            let truncated_body: String = body_text.chars().take(max_body_chars).collect();
            (truncated_body, true)
        } else {
            (body_text, false)
        };
        obj["body"] = json!(body);
        obj["body_truncated"] = json!(truncated);
        obj["preview"] = json!(email.preview);
    }

    obj
}

fn serialize_mailbox(mbox: &Mailbox) -> Value {
    json!({
        "id": mbox.id,
        "name": mbox.name,
        "parent_id": mbox.parent_id,
        "role": mbox.role,
        "total_emails": mbox.total_emails,
        "unread_emails": mbox.unread_emails,
        "sort_order": mbox.sort_order,
    })
}

fn dispatch(state: &mut CliState, input: &Value) -> Value {
    let command = match input.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return err_response("missing 'command' field"),
    };

    match command {
        "list_accounts" => cmd_list_accounts(state),
        "connect" => cmd_connect(state, input),
        "status" => cmd_status(state),
        "list_mailboxes" => cmd_list_mailboxes(state),
        "create_mailbox" => cmd_create_mailbox(state, input),
        "delete_mailbox" => cmd_delete_mailbox(state, input),
        "query_emails" => cmd_query_emails(state, input),
        "get_email" => cmd_get_email(state, input),
        "get_thread" => cmd_get_thread(state, input),
        "mark_read" => cmd_mark_read(state, input),
        "mark_unread" => cmd_mark_unread(state, input),
        "flag" => cmd_flag(state, input),
        "unflag" => cmd_unflag(state, input),
        "move_email" => cmd_move_email(state, input),
        "archive" => cmd_archive(state, input),
        "delete_email" => cmd_delete_email(state, input),
        "destroy" => cmd_destroy(state, input),
        "mark_mailbox_read" => cmd_mark_mailbox_read(state, input),
        "get_raw_headers" => cmd_get_raw_headers(state, input),
        "download_attachment" => cmd_download_attachment(state, input),
        "compose_draft" => cmd_compose_draft(state),
        "reply_draft" => cmd_reply_draft(state, input),
        "forward_draft" => cmd_forward_draft(state, input),
        "keybindings" => cmd_keybindings(),
        _ => err_response(&format!("unknown command '{}'", command)),
    }
}

// --- Command handlers ---

fn cmd_list_accounts(state: &CliState) -> Value {
    let accounts: Vec<Value> = state
        .config
        .accounts
        .iter()
        .map(|a| {
            json!({
                "name": a.name,
                "username": a.username,
                "well_known_url": a.well_known_url,
            })
        })
        .collect();
    ok_response(json!({"accounts": accounts}))
}

fn cmd_connect(state: &mut CliState, input: &Value) -> Value {
    let account_name = match input.get("account").and_then(|v| v.as_str()) {
        Some(name) => name,
        None => return err_response("missing 'account' field"),
    };

    let account = match state
        .config
        .accounts
        .iter()
        .find(|a| a.name == account_name)
    {
        Some(a) => a.clone(),
        None => return err_response(&format!("unknown account '{}'", account_name)),
    };

    // Disconnect existing backend if any
    if let Some(ref tx) = state.cmd_tx {
        let _ = tx.send(BackendCommand::Shutdown);
    }
    state.cmd_tx = None;
    state.resp_rx = None;
    state.connected_account = None;
    state.connected_username = None;
    state.cached_mailboxes.clear();

    let client = match crate::connect_account(&account) {
        Ok(c) => c,
        Err(e) => return err_response(&format!("connection failed: {}", e)),
    };

    let (cmd_tx, resp_rx) = backend::spawn(
        client,
        state.rules.clone(),
        state.custom_headers.clone(),
        state.rules_mailbox_regex.clone(),
        state.my_email_regex.clone(),
    );

    state.cmd_tx = Some(cmd_tx);
    state.resp_rx = Some(resp_rx);
    state.connected_account = Some(account.name.clone());
    state.connected_username = Some(account.username.clone());

    ok_response(json!({
        "account": account.name,
        "username": account.username,
    }))
}

fn cmd_status(state: &CliState) -> Value {
    ok_response(json!({
        "connected": state.cmd_tx.is_some(),
        "account": state.connected_account,
        "username": state.connected_username,
        "cached_mailboxes": state.cached_mailboxes.len(),
    }))
}

fn cmd_list_mailboxes(state: &mut CliState) -> Value {
    if let Err(e) = state.send_cmd(BackendCommand::FetchMailboxes {
        origin: "cli".to_string(),
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::Mailboxes(Ok(mailboxes))) => {
            state.cached_mailboxes = mailboxes.clone();
            let list: Vec<Value> = mailboxes.iter().map(serialize_mailbox).collect();
            ok_response(json!({"mailboxes": list}))
        }
        Ok(BackendResponse::Mailboxes(Err(e))) => err_response(&e),
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_create_mailbox(state: &mut CliState, input: &Value) -> Value {
    let name = match input.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return err_response("missing 'name' field"),
    };

    if let Err(e) = state.send_cmd(BackendCommand::CreateMailbox { name: name.clone() }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::MailboxCreated { name, result }) => match result {
            Ok(()) => ok_response(json!({"name": name})),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_delete_mailbox(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("mailbox_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'mailbox_id' field"),
    };

    let name = state
        .cached_mailboxes
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| id.clone());

    if let Err(e) = state.send_cmd(BackendCommand::DeleteMailbox {
        id,
        name: name.clone(),
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::MailboxDeleted { name, result }) => match result {
            Ok(()) => ok_response(json!({"name": name})),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_query_emails(state: &mut CliState, input: &Value) -> Value {
    let mailbox_id = match input.get("mailbox_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'mailbox_id' field"),
    };
    let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as u32;
    let position = input.get("position").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let search = input
        .get("search")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Err(e) = state.send_cmd(BackendCommand::QueryEmails {
        origin: "cli".to_string(),
        mailbox_id: mailbox_id.clone(),
        page_size: limit,
        position,
        search_query: search,
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::Emails {
            emails: Ok(emails),
            total,
            position,
            loaded,
            thread_counts,
            ..
        }) => {
            let headers_only = input
                .get("headers_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let max_body_chars = input
                .get("max_body_chars")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let list: Vec<Value> = emails
                .iter()
                .map(|e| serialize_email(e, headers_only, max_body_chars))
                .collect();

            let tc: Value = thread_counts
                .iter()
                .map(|(tid, (unread, total))| {
                    (tid.clone(), json!({"unread": unread, "total": total}))
                })
                .collect::<serde_json::Map<String, Value>>()
                .into();

            ok_response(json!({
                "emails": list,
                "total": total,
                "position": position,
                "loaded": loaded,
                "thread_counts": tc,
            }))
        }
        Ok(BackendResponse::Emails { emails: Err(e), .. }) => err_response(&e),
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_get_email(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };

    let headers_only = input
        .get("headers_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_body_chars = input
        .get("max_body_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    if let Err(e) = state.send_cmd(BackendCommand::GetEmail { id }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::EmailBody {
            result: boxed_result,
            ..
        }) => match *boxed_result {
            Ok(email) => {
                let data = serialize_email(&email, headers_only, max_body_chars);
                ok_response(data)
            }
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_get_thread(state: &mut CliState, input: &Value) -> Value {
    let thread_id = match input.get("thread_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'thread_id' field"),
    };

    let headers_only = input
        .get("headers_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let max_body_chars = input
        .get("max_body_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    if let Err(e) = state.send_cmd(BackendCommand::QueryThreadEmails {
        thread_id: thread_id.clone(),
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::ThreadEmails {
            emails: Ok(emails), ..
        }) => {
            let list: Vec<Value> = emails
                .iter()
                .map(|e| serialize_email(e, headers_only, max_body_chars))
                .collect();
            ok_response(json!({
                "thread_id": thread_id,
                "emails": list,
            }))
        }
        Ok(BackendResponse::ThreadEmails { emails: Err(e), .. }) => err_response(&e),
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_mark_read(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::MarkEmailRead { op_id, id }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_mark_unread(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::MarkEmailUnread { op_id, id }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_flag(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::SetEmailFlagged {
        op_id,
        id,
        flagged: true,
    }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_unflag(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::SetEmailFlagged {
        op_id,
        id,
        flagged: false,
    }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_move_email(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let to_mailbox_id = match input.get("to_mailbox_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'to_mailbox_id' field"),
    };
    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::MoveEmail {
        op_id,
        id,
        to_mailbox_id,
    }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_archive(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };

    let archive_id = match state.resolve_folder_id(&state.archive_folder.clone()) {
        Some(id) => id,
        None => {
            return err_response(&format!(
                "cannot resolve archive folder '{}'",
                state.archive_folder
            ))
        }
    };

    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::MoveEmail {
        op_id,
        id,
        to_mailbox_id: archive_id,
    }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_delete_email(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };

    let deleted_id = match state.resolve_folder_id(&state.deleted_folder.clone()) {
        Some(id) => id,
        None => {
            return err_response(&format!(
                "cannot resolve deleted folder '{}'",
                state.deleted_folder
            ))
        }
    };

    let op_id = state.next_op_id();

    if let Err(e) = state.send_cmd(BackendCommand::MoveEmail {
        op_id,
        id,
        to_mailbox_id: deleted_id,
    }) {
        return err_response(&e);
    }

    recv_mutation_response(state)
}

fn cmd_destroy(state: &mut CliState, input: &Value) -> Value {
    let ids = match input.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => {
            let mut ids = Vec::new();
            for v in arr {
                match v.as_str() {
                    Some(s) => ids.push(s.to_string()),
                    None => return err_response("'ids' must be an array of strings"),
                }
            }
            ids
        }
        None => {
            // Support single id too
            match input.get("id").and_then(|v| v.as_str()) {
                Some(id) => vec![id.to_string()],
                None => return err_response("missing 'ids' or 'id' field"),
            }
        }
    };

    let mut results = Vec::new();
    for id in &ids {
        let op_id = state.next_op_id();
        if let Err(e) = state.send_cmd(BackendCommand::DestroyEmail {
            op_id,
            id: id.clone(),
        }) {
            results.push(json!({"id": id, "ok": false, "error": e}));
            continue;
        }
        match state.recv_resp() {
            Ok(BackendResponse::EmailMutation { result, id, .. }) => match result {
                Ok(()) => results.push(json!({"id": id, "ok": true})),
                Err(e) => results.push(json!({"id": id, "ok": false, "error": e})),
            },
            Ok(_) => results.push(json!({"id": id, "ok": false, "error": "unexpected response"})),
            Err(e) => results.push(json!({"id": id, "ok": false, "error": e})),
        }
    }

    ok_response(json!({"results": results}))
}

fn cmd_mark_mailbox_read(state: &mut CliState, input: &Value) -> Value {
    let mailbox_id = match input.get("mailbox_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'mailbox_id' field"),
    };

    let mailbox_name = state
        .cached_mailboxes
        .iter()
        .find(|m| m.id == mailbox_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| mailbox_id.clone());

    if let Err(e) = state.send_cmd(BackendCommand::MarkMailboxRead {
        mailbox_id,
        mailbox_name,
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::MailboxMarkedRead {
            mailbox_name,
            updated,
            result,
            ..
        }) => match result {
            Ok(()) => ok_response(json!({
                "mailbox_name": mailbox_name,
                "updated": updated,
            })),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_get_raw_headers(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };

    if let Err(e) = state.send_cmd(BackendCommand::GetEmailRawHeaders { id }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::EmailRawHeaders { result, .. }) => match result {
            Ok(headers) => ok_response(json!({"headers": headers})),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_download_attachment(state: &mut CliState, input: &Value) -> Value {
    let blob_id = match input.get("blob_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'blob_id' field"),
    };
    let name = input
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("attachment")
        .to_string();
    let content_type = input
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream")
        .to_string();

    if let Err(e) = state.send_cmd(BackendCommand::DownloadAttachment {
        blob_id,
        name,
        content_type,
    }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::AttachmentDownloaded { name, result }) => match result {
            Ok(path) => ok_response(json!({
                "name": name,
                "path": path.to_string_lossy(),
            })),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_compose_draft(state: &CliState) -> Value {
    let from = state
        .connected_username
        .as_deref()
        .unwrap_or("user@example.com");
    let draft = compose::build_compose_draft(from);
    ok_response(json!({"draft": draft}))
}

fn cmd_reply_draft(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };
    let reply_all = input
        .get("reply_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if let Err(e) = state.send_cmd(BackendCommand::GetEmailForReply { id }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::EmailForReply {
            result: boxed_result,
            ..
        }) => match *boxed_result {
            Ok(email) => {
                let from = state
                    .connected_username
                    .as_deref()
                    .unwrap_or("user@example.com");
                let draft = compose::build_reply_draft(&email, reply_all, from);
                ok_response(json!({"draft": draft}))
            }
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_forward_draft(state: &mut CliState, input: &Value) -> Value {
    let id = match input.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return err_response("missing 'id' field"),
    };

    if let Err(e) = state.send_cmd(BackendCommand::GetEmailForReply { id }) {
        return err_response(&e);
    }

    match state.recv_resp() {
        Ok(BackendResponse::EmailForReply {
            result: boxed_result,
            ..
        }) => match *boxed_result {
            Ok(email) => {
                let from = state
                    .connected_username
                    .as_deref()
                    .unwrap_or("user@example.com");
                let draft = compose::build_forward_draft(&email, from);
                ok_response(json!({"draft": draft}))
            }
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

fn cmd_keybindings() -> Value {
    let bindings = keybindings::all_keybindings();
    let list: Vec<Value> = bindings
        .iter()
        .map(|kb| {
            json!({
                "view": kb.view,
                "key": kb.key,
                "action": kb.action,
                "description": kb.description,
            })
        })
        .collect();
    ok_response(json!({"keybindings": list}))
}

fn recv_mutation_response(state: &CliState) -> Value {
    match state.recv_resp() {
        Ok(BackendResponse::EmailMutation {
            op_id,
            id,
            action,
            result,
        }) => match result {
            Ok(()) => ok_response(json!({
                "op_id": op_id,
                "id": id,
                "action": format!("{:?}", action),
            })),
            Err(e) => err_response(&e),
        },
        Ok(_) => err_response("unexpected response from backend"),
        Err(e) => err_response(&e),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_cli(
    config: Config,
    rules: Vec<CompiledRule>,
    custom_headers: Vec<String>,
    rules_mailbox_regex: String,
    my_email_regex: String,
    archive_folder: String,
    deleted_folder: String,
) {
    let rules_regex = Regex::new(&rules_mailbox_regex).expect("invalid rules_mailbox_regex");
    let email_regex = Regex::new(&my_email_regex).expect("invalid my_email_regex");

    let mut state = CliState {
        config,
        cmd_tx: None,
        resp_rx: None,
        connected_account: None,
        connected_username: None,
        cached_mailboxes: Vec::new(),
        next_op_id: 0,
        rules: Arc::new(rules),
        custom_headers: Arc::new(custom_headers),
        rules_mailbox_regex: Arc::new(rules_regex),
        my_email_regex: Arc::new(email_regex),
        archive_folder,
        deleted_folder,
    };

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let input: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = err_response(&format!("JSON parse error: {}", e));
                let _ = serde_json::to_writer(&mut stdout, &resp);
                let _ = stdout.write_all(b"\n");
                let _ = stdout.flush();
                continue;
            }
        };

        let response = dispatch(&mut state, &input);
        let _ = serde_json::to_writer(&mut stdout, &response);
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }

    // Shutdown backend on exit
    if let Some(ref tx) = state.cmd_tx {
        let _ = tx.send(BackendCommand::Shutdown);
    }
}

pub fn print_help_cli() {
    print!(
        r#"tmc --cli: JSON-over-stdin/stdout CLI mode
==========================================

Protocol: Newline-Delimited JSON (NDJSON)
- Send one JSON object per line to stdin
- Receive one JSON response per line from stdout
- Responses have {{"ok": true, ...}} on success or {{"ok": false, "error": "..."}} on failure

Connection Flow
---------------
1. List available accounts:
   > {{"command": "list_accounts"}}
   < {{"ok": true, "accounts": [{{"name": "personal", "username": "me@example.com", ...}}]}}

2. Connect to an account:
   > {{"command": "connect", "account": "personal"}}
   < {{"ok": true, "account": "personal", "username": "me@example.com"}}

3. Check status:
   > {{"command": "status"}}
   < {{"ok": true, "connected": true, "account": "personal", "username": "me@example.com", "cached_mailboxes": 0}}

Mailbox Commands
----------------
list_mailboxes: Fetch and cache all mailboxes.
   > {{"command": "list_mailboxes"}}
   < {{"ok": true, "mailboxes": [{{"id": "...", "name": "INBOX", "role": "inbox", "total_emails": 42, "unread_emails": 3, ...}}]}}

create_mailbox: Create a new mailbox.
   > {{"command": "create_mailbox", "name": "NewFolder"}}
   < {{"ok": true, "name": "NewFolder"}}

delete_mailbox: Delete a mailbox by ID.
   > {{"command": "delete_mailbox", "mailbox_id": "mbox-id"}}
   < {{"ok": true, "name": "NewFolder"}}

mark_mailbox_read: Mark all emails in a mailbox as read.
   > {{"command": "mark_mailbox_read", "mailbox_id": "mbox-id"}}
   < {{"ok": true, "mailbox_name": "INBOX", "updated": 15}}

Email Query Commands
--------------------
query_emails: Query emails in a mailbox.
   > {{"command": "query_emails", "mailbox_id": "mbox-id", "limit": 50, "position": 0, "search": null}}
   Optional: headers_only (bool), max_body_chars (int)
   < {{"ok": true, "emails": [...], "total": 100, "position": 0, "loaded": 50, "thread_counts": {{...}}}}

get_email: Fetch a single email with full body.
   > {{"command": "get_email", "id": "email-id"}}
   Optional: headers_only (bool, default false), max_body_chars (int, 0=unlimited)
   < {{"ok": true, "id": "...", "subject": "...", "body": "...", "body_truncated": false, ...}}

get_thread: Fetch all emails in a thread.
   > {{"command": "get_thread", "thread_id": "thread-id"}}
   Optional: headers_only (bool), max_body_chars (int)
   < {{"ok": true, "thread_id": "...", "emails": [...]}}

get_raw_headers: Get raw RFC headers for an email.
   > {{"command": "get_raw_headers", "id": "email-id"}}
   < {{"ok": true, "headers": "From: ...\nTo: ...\n..."}}

Context Control
---------------
Both get_email, get_thread, and query_emails accept:
  - "headers_only": true — omit body/preview, return only metadata
  - "max_body_chars": 500 — truncate body text; response includes "body_truncated": true if truncated

Email Mutation Commands
-----------------------
mark_read:    {{"command": "mark_read", "id": "email-id"}}
mark_unread:  {{"command": "mark_unread", "id": "email-id"}}
flag:         {{"command": "flag", "id": "email-id"}}
unflag:       {{"command": "unflag", "id": "email-id"}}
move_email:   {{"command": "move_email", "id": "email-id", "to_mailbox_id": "mbox-id"}}
archive:      {{"command": "archive", "id": "email-id"}}  (uses configured archive folder)
delete_email: {{"command": "delete_email", "id": "email-id"}}  (uses configured deleted folder)
destroy:      {{"command": "destroy", "ids": ["id1", "id2"]}}  (permanently delete)

All mutations return: {{"ok": true, "op_id": N, "id": "...", "action": "..."}}

Attachment Commands
-------------------
download_attachment: Download an attachment blob.
   > {{"command": "download_attachment", "blob_id": "blob-id", "name": "file.pdf", "content_type": "application/pdf"}}
   < {{"ok": true, "name": "file.pdf", "path": "/tmp/tmc-attachments/file.pdf"}}

Compose Commands
----------------
compose_draft: Generate a blank compose template.
   > {{"command": "compose_draft"}}
   < {{"ok": true, "draft": "From: me@example.com\nTo: \nSubject: \n\n"}}

reply_draft: Generate a reply draft.
   > {{"command": "reply_draft", "id": "email-id", "reply_all": false}}
   < {{"ok": true, "draft": "From: ...\nTo: ...\nSubject: Re: ...\n\n> ..."}}

forward_draft: Generate a forward draft.
   > {{"command": "forward_draft", "id": "email-id"}}
   < {{"ok": true, "draft": "From: ...\nTo: \nSubject: Fwd: ...\n\n---------- Forwarded message ----------\n..."}}

Keybindings
-----------
keybindings: Export the TUI keybinding dictionary.
   > {{"command": "keybindings"}}
   < {{"ok": true, "keybindings": [{{"view": "global", "key": "?", "action": "help", "description": "Show help"}}, ...]}}
"#
    );
}
