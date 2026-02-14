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
    pub session_state: String,
}

#[derive(Debug, Deserialize)]
pub struct MethodResponse(pub String, pub serde_json::Value, pub String);

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
    pub account_id: String,
    pub state: String,
    pub list: Vec<Mailbox>,
    #[serde(default)]
    pub not_found: Vec<String>,
}

// Email types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailQueryResponse {
    pub account_id: String,
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
    pub message_id: Option<Vec<String>>,
    #[serde(default)]
    pub references: Option<Vec<String>>,
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
    pub r#type: Option<String>,
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
    pub account_id: String,
    pub state: String,
    pub list: Vec<Email>,
    #[serde(default)]
    pub not_found: Vec<String>,
}
