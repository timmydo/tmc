use std::fs;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Build a blank compose draft template.
pub fn build_compose_draft(from: &str) -> String {
    format!(
        "From: {}\nTo: \nCc: \nSubject: \n--text follows this line--\n\n",
        from
    )
}

/// Build a reply draft from an existing email.
pub fn build_reply_draft(email: &crate::jmap::types::Email, reply_all: bool, from: &str) -> String {
    // Determine To: address
    let to = if let Some(ref reply_to) = email.reply_to {
        format_address_list(reply_to)
    } else if let Some(ref email_from) = email.from {
        format_address_list(email_from)
    } else {
        String::new()
    };

    // Determine Cc: for reply-all
    let cc = if reply_all {
        let from_email_lower = extract_email_addr(from).map(|s| s.to_lowercase());
        let mut cc_addrs = Vec::new();

        // Add original To recipients (minus self)
        if let Some(ref orig_to) = email.to {
            for addr in orig_to {
                if let Some(ref email_addr) = addr.email {
                    if from_email_lower
                        .as_ref()
                        .map(|me| email_addr.to_lowercase() != *me)
                        .unwrap_or(true)
                    {
                        cc_addrs.push(addr.to_string());
                    }
                }
            }
        }

        // Add original Cc recipients (minus self)
        if let Some(ref orig_cc) = email.cc {
            for addr in orig_cc {
                if let Some(ref email_addr) = addr.email {
                    if from_email_lower
                        .as_ref()
                        .map(|me| email_addr.to_lowercase() != *me)
                        .unwrap_or(true)
                    {
                        cc_addrs.push(addr.to_string());
                    }
                }
            }
        }

        cc_addrs.join(", ")
    } else {
        String::new()
    };

    // Subject with Re: prefix
    let subject = match email.subject.as_deref() {
        Some(s) if s.starts_with("Re: ") || s.starts_with("re: ") => s.to_string(),
        Some(s) => format!("Re: {}", s),
        None => "Re: ".to_string(),
    };

    // In-Reply-To header
    let in_reply_to = email
        .message_id
        .as_ref()
        .and_then(|ids| ids.first())
        .map(|id| format!("<{}>", id.trim_matches(|c| c == '<' || c == '>')));

    // References header
    let references = {
        let mut refs = Vec::new();
        if let Some(ref orig_refs) = email.references {
            for r in orig_refs {
                refs.push(format!("<{}>", r.trim_matches(|c| c == '<' || c == '>')));
            }
        }
        if let Some(ref msg_ids) = email.message_id {
            if let Some(id) = msg_ids.first() {
                let formatted = format!("<{}>", id.trim_matches(|c| c == '<' || c == '>'));
                if !refs.contains(&formatted) {
                    refs.push(formatted);
                }
            }
        }
        if refs.is_empty() {
            None
        } else {
            Some(refs.join(" "))
        }
    };

    // Quoted body
    let body_text = extract_body_text(email);
    let sender_display = email
        .from
        .as_ref()
        .and_then(|addrs| addrs.first())
        .map(|a| a.to_string())
        .unwrap_or_else(|| "(unknown)".to_string());
    let date = email
        .sent_at
        .as_deref()
        .or(email.received_at.as_deref())
        .unwrap_or("(unknown date)");

    let quoted: String = body_text
        .lines()
        .map(|line| format!("> {}", line))
        .collect::<Vec<_>>()
        .join("\n");

    let mut draft = format!("From: {}\nTo: {}\n", from, to);
    if !cc.is_empty() {
        draft.push_str(&format!("Cc: {}\n", cc));
    }
    draft.push_str(&format!("Subject: {}\n", subject));
    if let Some(ref irt) = in_reply_to {
        draft.push_str(&format!("In-Reply-To: {}\n", irt));
    }
    if let Some(ref refs) = references {
        draft.push_str(&format!("References: {}\n", refs));
    }
    draft.push_str("--text follows this line--\n");
    draft.push_str(&format!(
        "\nOn {}, {} wrote:\n{}\n",
        date, sender_display, quoted
    ));

    draft
}

/// Build a forward draft from an existing email.
pub fn build_forward_draft(email: &crate::jmap::types::Email, from: &str) -> String {
    // Subject with Fwd: prefix
    let subject = match email.subject.as_deref() {
        Some(s) if s.starts_with("Fwd: ") || s.starts_with("fwd: ") => s.to_string(),
        Some(s) => format!("Fwd: {}", s),
        None => "Fwd: ".to_string(),
    };

    // Original message info
    let orig_from = email
        .from
        .as_ref()
        .map(|addrs| format_address_list(addrs))
        .unwrap_or_else(|| "(unknown)".to_string());
    let orig_to = email
        .to
        .as_ref()
        .map(|addrs| format_address_list(addrs))
        .unwrap_or_default();
    let orig_cc = email
        .cc
        .as_ref()
        .map(|addrs| format_address_list(addrs))
        .unwrap_or_default();
    let date = email
        .sent_at
        .as_deref()
        .or(email.received_at.as_deref())
        .unwrap_or("(unknown date)");
    let orig_subject = email.subject.as_deref().unwrap_or("(no subject)");

    let body_text = extract_body_text(email);

    let mut draft = format!("From: {}\nTo: \nSubject: {}\n", from, subject);

    draft.push_str("--text follows this line--\n");
    draft.push_str("\n---------- Forwarded message ----------\n");
    draft.push_str(&format!("From: {}\n", orig_from));
    draft.push_str(&format!("Date: {}\n", date));
    draft.push_str(&format!("Subject: {}\n", orig_subject));
    draft.push_str(&format!("To: {}\n", orig_to));
    if !orig_cc.is_empty() {
        draft.push_str(&format!("Cc: {}\n", orig_cc));
    }
    draft.push('\n');
    draft.push_str(&body_text);
    draft.push('\n');

    draft
}

fn format_address_list(addrs: &[crate::jmap::types::EmailAddress]) -> String {
    addrs
        .iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn extract_email_addr(from_header: &str) -> Option<String> {
    if let (Some(start), Some(end)) = (from_header.find('<'), from_header.rfind('>')) {
        if end > start + 1 {
            let addr = from_header[start + 1..end].trim();
            if !addr.is_empty() {
                return Some(addr.to_string());
            }
        }
    }
    let trimmed = from_header.trim();
    if trimmed.contains('@') {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub(crate) fn extract_body_text(email: &crate::jmap::types::Email) -> String {
    // Prefer textBody (plain text) — it preserves the author's formatting.
    if let Some(ref text_body) = email.text_body {
        for part in text_body {
            if let Some(value) = email.body_values.get(&part.part_id) {
                if looks_like_html(&value.value)
                    || part
                        .r#type
                        .as_deref()
                        .map(|t| t.eq_ignore_ascii_case("text/html"))
                        .unwrap_or(false)
                {
                    return html_to_plain(&value.value);
                }
                return value.value.clone();
            }
        }
    }
    // Fall back to htmlBody when no plain text is available
    if let Some(ref html_body) = email.html_body {
        for part in html_body {
            if let Some(value) = email.body_values.get(&part.part_id) {
                return html_to_plain(&value.value);
            }
        }
    }
    email.preview.as_deref().unwrap_or("(no body)").to_string()
}

/// Heuristic check: does this text look like HTML rather than plain text?
/// Checks for common HTML structural tags anywhere in the content.
fn looks_like_html(text: &str) -> bool {
    let sample = if text.len() > 2000 {
        &text[..2000]
    } else {
        text
    };
    let lower = sample.to_ascii_lowercase();
    lower.contains("<!doctype")
        || lower.contains("<html")
        || lower.contains("<head")
        || lower.contains("<body")
        || lower.contains("<style")
        || lower.contains("<table")
        || lower.contains("<div")
}

/// Convert HTML to plain text (no ANSI formatting). Used for drafts and CLI.
fn html_to_plain(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 80).unwrap_or_else(|_| html.to_string())
}

/// Write content to a temp file with restrictive permissions (0600).
pub fn write_temp_file(content: &str) -> io::Result<PathBuf> {
    let dir = draft_dir();
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let filename = format!("tmc-draft-{}-{}.eml", std::process::id(), ts);
    let path = dir.join(filename);

    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;

    io::Write::write_all(&mut file, content.as_bytes())?;
    Ok(path)
}

fn draft_dir() -> PathBuf {
    draft_dir_from_env(
        std::env::var("XDG_RUNTIME_DIR").ok(),
        std::env::var("XDG_STATE_HOME").ok(),
        std::env::var("HOME").ok(),
    )
}

fn draft_dir_from_env(
    xdg_runtime_dir: Option<String>,
    xdg_state_home: Option<String>,
    home: Option<String>,
) -> PathBuf {
    if let Some(runtime_dir) = xdg_runtime_dir {
        let trimmed = runtime_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("tmc").join("drafts");
        }
    }

    let state_dir = if let Some(xdg) = xdg_state_home {
        PathBuf::from(xdg)
    } else if let Some(home) = home {
        PathBuf::from(home).join(".local").join("state")
    } else {
        PathBuf::from(".")
    };

    state_dir.join("tmc").join("drafts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_compose_draft() {
        let draft = build_compose_draft("me@example.com");
        assert!(draft.contains("From: me@example.com"));
        assert!(draft.contains("To: \n"));
        assert!(draft.contains("Subject: \n"));
        assert!(draft.contains("--text follows this line--"));
    }

    #[test]
    fn test_build_reply_draft() {
        use crate::jmap::types::{Email, EmailAddress};
        use std::collections::HashMap;

        let email = Email {
            id: "test-id".to_string(),
            thread_id: None,
            from: Some(vec![EmailAddress {
                name: Some("Sender".to_string()),
                email: Some("sender@example.com".to_string()),
            }]),
            to: Some(vec![EmailAddress {
                name: None,
                email: Some("me@example.com".to_string()),
            }]),
            cc: None,
            reply_to: None,
            subject: Some("Hello".to_string()),
            received_at: Some("2024-01-01T00:00:00Z".to_string()),
            sent_at: Some("2024-01-01T00:00:00Z".to_string()),
            preview: Some("Preview text".to_string()),
            text_body: None,
            html_body: None,
            body_values: HashMap::new(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
            message_id: Some(vec!["abc@example.com".to_string()]),
            references: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let draft = build_reply_draft(&email, false, "me@example.com");
        assert!(draft.contains("To: Sender <sender@example.com>"));
        assert!(draft.contains("Subject: Re: Hello"));
        assert!(draft.contains("In-Reply-To: <abc@example.com>"));
        assert!(draft.contains("> Preview text"));

        // Reply-all should include original To minus self
        let draft_all = build_reply_draft(&email, true, "me@example.com");
        assert!(!draft_all.contains("Cc:")); // self was the only To recipient
    }

    #[test]
    fn test_build_reply_draft_reply_all_skips_self_for_named_from_header() {
        use crate::jmap::types::{Email, EmailAddress};
        use std::collections::HashMap;

        let email = Email {
            id: "test-id".to_string(),
            thread_id: None,
            from: Some(vec![EmailAddress {
                name: Some("Sender".to_string()),
                email: Some("sender@example.com".to_string()),
            }]),
            to: Some(vec![
                EmailAddress {
                    name: Some("Example User".to_string()),
                    email: Some("user@example.com".to_string()),
                },
                EmailAddress {
                    name: Some("Other".to_string()),
                    email: Some("other@example.com".to_string()),
                },
            ]),
            cc: None,
            reply_to: None,
            subject: Some("Hello".to_string()),
            received_at: Some("2024-01-01T00:00:00Z".to_string()),
            sent_at: Some("2024-01-01T00:00:00Z".to_string()),
            preview: Some("Preview text".to_string()),
            text_body: None,
            html_body: None,
            body_values: HashMap::new(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
            message_id: Some(vec!["abc@example.com".to_string()]),
            references: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let draft = build_reply_draft(&email, true, "Example User <user@example.com>");
        assert!(!draft.contains("Cc: Example User <user@example.com>"));
        assert!(draft.contains("Cc: Other <other@example.com>"));
    }

    #[test]
    fn test_build_forward_draft() {
        use crate::jmap::types::{Email, EmailAddress};
        use std::collections::HashMap;

        let email = Email {
            id: "test-id".to_string(),
            thread_id: None,
            from: Some(vec![EmailAddress {
                name: Some("Sender".to_string()),
                email: Some("sender@example.com".to_string()),
            }]),
            to: Some(vec![EmailAddress {
                name: None,
                email: Some("me@example.com".to_string()),
            }]),
            cc: Some(vec![EmailAddress {
                name: Some("Other".to_string()),
                email: Some("other@example.com".to_string()),
            }]),
            reply_to: None,
            subject: Some("Hello".to_string()),
            received_at: Some("2024-01-01T00:00:00Z".to_string()),
            sent_at: Some("2024-01-01T00:00:00Z".to_string()),
            preview: Some("Preview text".to_string()),
            text_body: None,
            html_body: None,
            body_values: HashMap::new(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
            message_id: None,
            references: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let draft = build_forward_draft(&email, "me@example.com");
        assert!(draft.contains("From: me@example.com"));
        assert!(draft.contains("To: \n"));
        assert!(draft.contains("Subject: Fwd: Hello"));
        assert!(draft.contains("---------- Forwarded message ----------"));
        assert!(draft.contains("From: Sender <sender@example.com>"));
        assert!(draft.contains("Date: 2024-01-01T00:00:00Z"));
        assert!(draft.contains("Subject: Hello"));
        assert!(draft.contains("To: me@example.com"));
        assert!(draft.contains("Cc: Other <other@example.com>"));
        assert!(draft.contains("Preview text"));
    }

    #[test]
    fn test_build_forward_draft_already_prefixed() {
        use crate::jmap::types::{Email, EmailAddress};
        use std::collections::HashMap;

        let email = Email {
            id: "test-id".to_string(),
            thread_id: None,
            from: Some(vec![EmailAddress {
                name: None,
                email: Some("sender@example.com".to_string()),
            }]),
            to: None,
            cc: None,
            reply_to: None,
            subject: Some("Fwd: Already forwarded".to_string()),
            received_at: None,
            sent_at: None,
            preview: Some("body".to_string()),
            text_body: None,
            html_body: None,
            body_values: HashMap::new(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
            message_id: None,
            references: None,
            attachments: None,
            extra: HashMap::new(),
        };

        let draft = build_forward_draft(&email, "me@example.com");
        assert!(draft.contains("Subject: Fwd: Already forwarded\n"));
        // Should not double-prefix
        assert!(!draft.contains("Fwd: Fwd:"));
    }

    #[test]
    fn test_draft_dir_uses_xdg_runtime_dir() {
        assert_eq!(
            draft_dir_from_env(
                Some("/tmp/runtime-test".to_string()),
                Some("/tmp/state-test".to_string()),
                Some("/home/example".to_string())
            ),
            PathBuf::from("/tmp/runtime-test")
                .join("tmc")
                .join("drafts")
        );
    }

    #[test]
    fn test_draft_dir_falls_back_to_state_home() {
        assert_eq!(
            draft_dir_from_env(
                None,
                Some("/tmp/state-test".to_string()),
                Some("/home/example".to_string())
            ),
            PathBuf::from("/tmp/state-test").join("tmc").join("drafts")
        );
    }

    #[test]
    fn test_draft_dir_falls_back_to_home_state() {
        assert_eq!(
            draft_dir_from_env(None, None, Some("/home/example".to_string())),
            PathBuf::from("/home/example")
                .join(".local")
                .join("state")
                .join("tmc")
                .join("drafts")
        );
    }
}
