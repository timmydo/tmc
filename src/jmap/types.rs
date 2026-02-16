use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// JMAP Session (from .well-known/jmap)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapSession {
    pub username: String,
    pub api_url: String,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub primary_accounts: HashMap<String, String>,
    #[serde(default)]
    pub accounts: HashMap<String, JmapAccount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapAccount {
    pub name: String,
    #[serde(default)]
    pub is_personal: bool,
    #[serde(default)]
    pub is_read_only: bool,
}

impl JmapSession {
    pub fn mail_account_id(&self) -> Option<&str> {
        // First try the standard primaryAccounts lookup
        if let Some(id) = self.primary_accounts.get("urn:ietf:params:jmap:mail") {
            return Some(id.as_str());
        }

        // Fallback: if there's exactly one account, use that
        if self.accounts.len() == 1 {
            return self.accounts.keys().next().map(|s| s.as_str());
        }

        None
    }
}

// JMAP Request/Response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapRequest {
    pub using: Vec<&'static str>,
    pub method_calls: Vec<MethodCall>,
}

#[derive(Debug, Serialize)]
pub struct MethodCall(pub &'static str, pub serde_json::Value, pub String);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JmapResponse {
    pub method_responses: Vec<MethodResponse>,
    #[serde(default)]
    #[allow(dead_code)]
    pub session_state: String,
}

#[derive(Debug, Deserialize)]
pub struct MethodResponse(
    pub String,
    pub serde_json::Value,
    #[allow(dead_code)] pub String,
);

// Mailbox types
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub total_emails: u32,
    #[serde(default)]
    pub unread_emails: u32,
    #[serde(default)]
    pub sort_order: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxGetResponse {
    #[allow(dead_code)]
    pub account_id: String,
    #[allow(dead_code)]
    pub state: String,
    pub list: Vec<Mailbox>,
    #[serde(default)]
    #[allow(dead_code)]
    pub not_found: Vec<String>,
}

// Email types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailQueryResponse {
    #[allow(dead_code)]
    pub account_id: String,
    #[allow(dead_code)]
    pub query_state: String,
    pub ids: Vec<String>,
    #[serde(default)]
    pub position: u32,
    #[serde(default)]
    pub total: Option<u32>,
}

#[derive(Debug)]
pub struct EmailQueryResult {
    pub ids: Vec<String>,
    pub total: Option<u32>,
    pub position: u32,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub from: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub cc: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub reply_to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub received_at: Option<String>,
    #[serde(default)]
    pub sent_at: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub text_body: Option<Vec<BodyPart>>,
    #[serde(default)]
    pub body_values: HashMap<String, BodyValue>,
    #[serde(default)]
    pub keywords: HashMap<String, bool>,
    #[serde(default)]
    pub mailbox_ids: HashMap<String, bool>,
    #[serde(default)]
    pub message_id: Option<Vec<String>>,
    #[serde(default)]
    pub references: Option<Vec<String>>,
    #[serde(default)]
    pub attachments: Option<Vec<BodyPart>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmailAddress {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

impl std::fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.name, &self.email) {
            (Some(name), Some(email)) => write!(f, "{} <{}>", name, email),
            (None, Some(email)) => write!(f, "{}", email),
            (Some(name), None) => write!(f, "{}", name),
            (None, None) => write!(f, "(unknown)"),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BodyPart {
    pub part_id: String,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BodyValue {
    pub value: String,
    #[serde(default)]
    pub is_encoding_problem: bool,
    #[serde(default)]
    pub is_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailGetResponse {
    #[allow(dead_code)]
    pub account_id: String,
    #[allow(dead_code)]
    pub state: String,
    pub list: Vec<Email>,
    #[serde(default)]
    #[allow(dead_code)]
    pub not_found: Vec<String>,
}

// Thread types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub email_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGetResponse {
    #[allow(dead_code)]
    pub account_id: String,
    #[allow(dead_code)]
    pub state: String,
    pub list: Vec<Thread>,
    #[serde(default)]
    #[allow(dead_code)]
    pub not_found: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deserialize_minimal_jmap_session() {
        let data = json!({
            "username": "user@example.com",
            "apiUrl": "https://api.example.com/jmap"
        });
        let session: JmapSession = serde_json::from_value(data).unwrap();
        assert_eq!(session.username, "user@example.com");
        assert_eq!(session.api_url, "https://api.example.com/jmap");
        assert!(session.download_url.is_none());
        assert!(session.primary_accounts.is_empty());
        assert!(session.accounts.is_empty());
    }

    #[test]
    fn test_deserialize_full_jmap_session() {
        let data = json!({
            "username": "user@example.com",
            "apiUrl": "https://api.example.com/jmap",
            "downloadUrl": "https://api.example.com/download/{blobId}",
            "primaryAccounts": {
                "urn:ietf:params:jmap:mail": "acc-001"
            },
            "accounts": {
                "acc-001": {
                    "name": "Personal",
                    "isPersonal": true,
                    "isReadOnly": false
                }
            }
        });
        let session: JmapSession = serde_json::from_value(data).unwrap();
        assert_eq!(
            session.download_url.as_deref(),
            Some("https://api.example.com/download/{blobId}")
        );
        assert_eq!(session.primary_accounts.len(), 1);
        assert_eq!(session.accounts.len(), 1);
        let acc = &session.accounts["acc-001"];
        assert_eq!(acc.name, "Personal");
        assert!(acc.is_personal);
        assert!(!acc.is_read_only);
    }

    #[test]
    fn test_mail_account_id_primary() {
        let data = json!({
            "username": "u@e.com",
            "apiUrl": "https://api.e.com/jmap",
            "primaryAccounts": {
                "urn:ietf:params:jmap:mail": "primary-id"
            },
            "accounts": {
                "primary-id": { "name": "Main", "isPersonal": true },
                "other-id": { "name": "Other", "isPersonal": false }
            }
        });
        let session: JmapSession = serde_json::from_value(data).unwrap();
        assert_eq!(session.mail_account_id(), Some("primary-id"));
    }

    #[test]
    fn test_mail_account_id_fallback_single() {
        let data = json!({
            "username": "u@e.com",
            "apiUrl": "https://api.e.com/jmap",
            "accounts": {
                "only-one": { "name": "Solo", "isPersonal": true }
            }
        });
        let session: JmapSession = serde_json::from_value(data).unwrap();
        assert_eq!(session.mail_account_id(), Some("only-one"));
    }

    #[test]
    fn test_mail_account_id_none_multiple_no_primary() {
        let data = json!({
            "username": "u@e.com",
            "apiUrl": "https://api.e.com/jmap",
            "accounts": {
                "acc-1": { "name": "A", "isPersonal": false },
                "acc-2": { "name": "B", "isPersonal": false }
            }
        });
        let session: JmapSession = serde_json::from_value(data).unwrap();
        assert_eq!(session.mail_account_id(), None);
    }

    #[test]
    fn test_deserialize_email_minimal() {
        let data = json!({
            "id": "email-001"
        });
        let email: Email = serde_json::from_value(data).unwrap();
        assert_eq!(email.id, "email-001");
        assert!(email.thread_id.is_none());
        assert!(email.from.is_none());
        assert!(email.to.is_none());
        assert!(email.cc.is_none());
        assert!(email.reply_to.is_none());
        assert!(email.subject.is_none());
        assert!(email.received_at.is_none());
        assert!(email.sent_at.is_none());
        assert!(email.preview.is_none());
        assert!(email.text_body.is_none());
        assert!(email.body_values.is_empty());
        assert!(email.keywords.is_empty());
        assert!(email.mailbox_ids.is_empty());
        assert!(email.message_id.is_none());
        assert!(email.references.is_none());
        assert!(email.attachments.is_none());
    }

    #[test]
    fn test_deserialize_email_full() {
        let data = json!({
            "id": "email-002",
            "threadId": "thread-001",
            "from": [{"name": "Alice", "email": "alice@example.com"}],
            "to": [{"name": "Bob", "email": "bob@example.com"}],
            "cc": [{"email": "cc@example.com"}],
            "replyTo": [{"name": "Alice", "email": "alice@example.com"}],
            "subject": "Hello World",
            "receivedAt": "2025-01-01T00:00:00Z",
            "sentAt": "2025-01-01T00:00:00Z",
            "preview": "This is a preview",
            "textBody": [{"partId": "1"}],
            "bodyValues": {"1": {"value": "body text", "isEncodingProblem": false, "isTruncated": false}},
            "keywords": {"$seen": true, "$flagged": true},
            "mailboxIds": {"mbox-1": true},
            "messageId": ["<msg-id@example.com>"],
            "references": ["<ref-1@example.com>"],
            "attachments": [{"partId": "2", "blobId": "blob-1", "type": "application/pdf", "name": "doc.pdf", "size": 1024}]
        });
        let email: Email = serde_json::from_value(data).unwrap();
        assert_eq!(email.id, "email-002");
        assert_eq!(email.thread_id.as_deref(), Some("thread-001"));
        assert_eq!(email.subject.as_deref(), Some("Hello World"));
        assert_eq!(email.from.as_ref().unwrap().len(), 1);
        assert_eq!(email.keywords.len(), 2);
        assert_eq!(email.attachments.as_ref().unwrap().len(), 1);
        assert_eq!(
            email.attachments.as_ref().unwrap()[0].name.as_deref(),
            Some("doc.pdf")
        );
    }

    #[test]
    fn test_deserialize_mailbox_defaults() {
        let data = json!({
            "id": "mbox-1",
            "name": "Inbox"
        });
        let mbox: Mailbox = serde_json::from_value(data).unwrap();
        assert_eq!(mbox.id, "mbox-1");
        assert_eq!(mbox.name, "Inbox");
        assert!(mbox.parent_id.is_none());
        assert!(mbox.role.is_none());
        assert_eq!(mbox.total_emails, 0);
        assert_eq!(mbox.unread_emails, 0);
        assert_eq!(mbox.sort_order, 0);
    }

    #[test]
    fn test_deserialize_thread() {
        let data = json!({
            "id": "thread-001",
            "emailIds": ["email-1", "email-2", "email-3"]
        });
        let thread: Thread = serde_json::from_value(data).unwrap();
        assert_eq!(thread.id, "thread-001");
        assert_eq!(thread.email_ids, vec!["email-1", "email-2", "email-3"]);
    }

    #[test]
    fn test_deserialize_email_query_response_with_total() {
        let data = json!({
            "accountId": "acc-1",
            "queryState": "state-1",
            "ids": ["e1", "e2"],
            "position": 0,
            "total": 42
        });
        let resp: EmailQueryResponse = serde_json::from_value(data).unwrap();
        assert_eq!(resp.ids, vec!["e1", "e2"]);
        assert_eq!(resp.total, Some(42));
        assert_eq!(resp.position, 0);
    }

    #[test]
    fn test_deserialize_email_query_response_without_total() {
        let data = json!({
            "accountId": "acc-1",
            "queryState": "state-1",
            "ids": [],
            "position": 10
        });
        let resp: EmailQueryResponse = serde_json::from_value(data).unwrap();
        assert!(resp.ids.is_empty());
        assert_eq!(resp.total, None);
        assert_eq!(resp.position, 10);
    }

    #[test]
    fn test_email_address_display_name_and_email() {
        let addr = EmailAddress {
            name: Some("Alice".to_string()),
            email: Some("alice@example.com".to_string()),
        };
        assert_eq!(format!("{}", addr), "Alice <alice@example.com>");
    }

    #[test]
    fn test_email_address_display_email_only() {
        let addr = EmailAddress {
            name: None,
            email: Some("alice@example.com".to_string()),
        };
        assert_eq!(format!("{}", addr), "alice@example.com");
    }

    #[test]
    fn test_email_address_display_name_only() {
        let addr = EmailAddress {
            name: Some("Alice".to_string()),
            email: None,
        };
        assert_eq!(format!("{}", addr), "Alice");
    }

    #[test]
    fn test_email_address_display_unknown() {
        let addr = EmailAddress {
            name: None,
            email: None,
        };
        assert_eq!(format!("{}", addr), "(unknown)");
    }
}
