use base64::Engine;
use serde_json::json;
use std::io::Read as _;

use super::types::*;

pub struct JmapClient {
    username: String,
    password: String,
    api_url: String,
    account_id: String,
    download_url: Option<String>,
}

#[derive(Debug)]
pub enum JmapError {
    Http(String),
    Parse(String),
    Api(String),
}

impl std::fmt::Display for JmapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JmapError::Http(e) => write!(f, "HTTP error: {}", e),
            JmapError::Parse(e) => write!(f, "Parse error: {}", e),
            JmapError::Api(e) => write!(f, "API error: {}", e),
        }
    }
}

impl JmapClient {
    fn auth_header(username: &str, password: &str) -> String {
        let credentials = format!("{}:{}", username, password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        format!("Basic {}", encoded)
    }

    /// Fetch a URL following redirects manually while preserving the auth header.
    fn fetch_with_auth_following_redirects(
        url: &str,
        auth: &str,
        max_redirects: u32,
    ) -> Result<(String, String), JmapError> {
        let agent = ureq::AgentBuilder::new().redirects(0).build();

        let mut current_url = url.to_string();

        for i in 0..max_redirects {
            log_debug!("[JMAP] Request {} to: {}", i + 1, current_url);

            let response = agent.get(&current_url).set("Authorization", auth).call();

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    log_debug!("[JMAP] Got {} response", status);

                    if (300..400).contains(&status) {
                        if let Some(location) = resp.header("location") {
                            log_debug!("[JMAP] Following redirect {} -> {}", status, location);
                            current_url = Self::resolve_redirect(&current_url, location);
                            continue;
                        } else {
                            return Err(JmapError::Http(format!(
                                "Redirect {} without Location header",
                                status
                            )));
                        }
                    }

                    let body = resp
                        .into_string()
                        .map_err(|e| JmapError::Parse(format!("Failed to read response: {}", e)))?;

                    if body.is_empty() {
                        return Err(JmapError::Http(format!(
                            "Server returned empty response (status {})",
                            status
                        )));
                    }

                    log_debug!("[JMAP] Response body length: {} bytes", body.len());
                    return Ok((current_url, body));
                }
                Err(ureq::Error::Status(code, resp)) if (300..400).contains(&code) => {
                    if let Some(location) = resp.header("location") {
                        log_debug!("[JMAP] Following redirect {} -> {}", code, location);
                        current_url = Self::resolve_redirect(&current_url, location);
                    } else {
                        return Err(JmapError::Http(format!(
                            "Redirect {} without Location header",
                            code
                        )));
                    }
                }
                Err(ureq::Error::Status(code, resp)) => {
                    let body = resp.into_string().unwrap_or_default();
                    log_error!("[JMAP] HTTP error {}: {}", code, body);

                    if code == 401 {
                        return Err(JmapError::Http(
                            "Authentication failed (401 Unauthorized)".to_string(),
                        ));
                    }

                    return Err(JmapError::Http(format!(
                        "HTTP {} error: {}",
                        code,
                        if body.is_empty() {
                            "(empty response)".to_string()
                        } else {
                            truncate_str(&body, 200).to_string()
                        }
                    )));
                }
                Err(e) => {
                    log_error!("[JMAP] Connection error: {}", e);
                    return Err(JmapError::Http(e.to_string()));
                }
            }
        }

        Err(JmapError::Http("Too many redirects".to_string()))
    }

    /// Resolve a redirect location against a base URL.
    fn resolve_redirect(base_url: &str, location: &str) -> String {
        if location.starts_with("http://") || location.starts_with("https://") {
            location.to_string()
        } else if location.starts_with('/') {
            if let Some(idx) = base_url.find("://") {
                let after_scheme = &base_url[idx + 3..];
                if let Some(path_start) = after_scheme.find('/') {
                    let host_part = &base_url[..idx + 3 + path_start];
                    format!("{}{}", host_part, location)
                } else {
                    format!("{}{}", base_url, location)
                }
            } else {
                location.to_string()
            }
        } else if let Some(last_slash) = base_url.rfind('/') {
            format!("{}/{}", &base_url[..last_slash], location)
        } else {
            location.to_string()
        }
    }

    pub fn discover(
        well_known_url: &str,
        username: &str,
        password: &str,
    ) -> Result<(JmapSession, Self), JmapError> {
        log_info!("[JMAP] Discovering JMAP session from: {}", well_known_url);
        let auth = Self::auth_header(username, password);

        let (_final_url, response_text) =
            Self::fetch_with_auth_following_redirects(well_known_url, &auth, 5)?;

        log_debug!("[JMAP] Session response received, parsing...");

        let session: JmapSession = serde_json::from_str(&response_text).map_err(|e| {
            JmapError::Parse(format!(
                "Failed to parse session: {}. Response was: {}",
                e,
                truncate_str(&response_text, 500)
            ))
        })?;

        log_debug!("[JMAP] Session parsed, api_url: {}", session.api_url);

        let account_id = session
            .mail_account_id()
            .ok_or_else(|| {
                JmapError::Api(format!(
                    "No mail account found in session response: {}",
                    truncate_str(&response_text, 500)
                ))
            })?
            .to_string();

        log_info!("[JMAP] Discovery successful, account_id: {}", account_id);

        let client = JmapClient {
            username: username.to_string(),
            password: password.to_string(),
            api_url: session.api_url.clone(),
            account_id,
            download_url: session.download_url.clone(),
        };

        Ok((session, client))
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    fn call(&self, request: JmapRequest) -> Result<JmapResponse, JmapError> {
        let auth = Self::auth_header(&self.username, &self.password);

        let request_json = serde_json::to_string(&request)
            .map_err(|e| JmapError::Parse(format!("Failed to serialize request: {}", e)))?;
        log_debug!("[JMAP] Request body: {}", truncate_str(&request_json, 500));

        let response = ureq::post(&self.api_url)
            .set("Authorization", &auth)
            .set("Content-Type", "application/json")
            .send_json(&request)
            .map_err(|e| {
                log_error!("[JMAP] API call failed: {}", e);
                JmapError::Http(e.to_string())
            })?;

        let response_text = response
            .into_string()
            .map_err(|e| JmapError::Parse(format!("Failed to read response: {}", e)))?;

        log_debug!(
            "[JMAP] Response body ({} bytes): {}",
            response_text.len(),
            truncate_str(&response_text, 1000)
        );

        let parsed: JmapResponse = serde_json::from_str(&response_text)
            .map_err(|e| JmapError::Parse(format!("Failed to parse response: {}", e)))?;

        Ok(parsed)
    }

    pub fn get_mailboxes(&self) -> Result<Vec<Mailbox>, JmapError> {
        log_info!("[JMAP] Fetching mailboxes for account: {}", self.account_id);

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Mailbox/get",
                json!({
                    "accountId": self.account_id,
                    "ids": null
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Mailbox/get" {
                let mailbox_response: MailboxGetResponse =
                    serde_json::from_value(method_response.1.clone())
                        .map_err(|e| JmapError::Parse(e.to_string()))?;
                log_info!(
                    "[JMAP] Mailbox/get returned {} mailboxes",
                    mailbox_response.list.len()
                );
                return Ok(mailbox_response.list);
            }
        }

        Err(JmapError::Api("Unexpected response".to_string()))
    }

    pub fn query_emails(
        &self,
        mailbox_id: &str,
        limit: u32,
        position: u32,
        search_text: Option<&str>,
    ) -> Result<EmailQueryResult, JmapError> {
        log_info!(
            "[JMAP] Email/query for mailbox: {} (limit: {}, position: {}, search: {:?})",
            mailbox_id,
            limit,
            position,
            search_text
        );

        let filter = if let Some(text) = search_text {
            json!({
                "operator": "AND",
                "conditions": [
                    { "inMailbox": mailbox_id },
                    { "text": text }
                ]
            })
        } else {
            json!({ "inMailbox": mailbox_id })
        };

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/query",
                json!({
                    "accountId": self.account_id,
                    "filter": filter,
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "limit": limit,
                    "position": position
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/query" {
                let query_response: EmailQueryResponse =
                    serde_json::from_value(method_response.1.clone())
                        .map_err(|e| JmapError::Parse(e.to_string()))?;
                log_info!(
                    "[JMAP] Email/query returned {} email IDs (total: {:?})",
                    query_response.ids.len(),
                    query_response.total
                );
                return Ok(EmailQueryResult {
                    ids: query_response.ids,
                    total: query_response.total,
                    position: query_response.position,
                });
            }
        }

        Err(JmapError::Api("Unexpected response".to_string()))
    }

    pub fn get_emails(&self, ids: &[String]) -> Result<Vec<Email>, JmapError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        log_info!("[JMAP] Email/get for {} email IDs", ids.len());

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/get",
                json!({
                    "accountId": self.account_id,
                    "ids": ids,
                    "properties": [
                        "id", "from", "to", "cc", "subject",
                        "receivedAt", "preview", "textBody", "bodyValues", "keywords",
                        "mailboxIds", "attachments"
                    ],
                    "fetchTextBodyValues": true
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/get" {
                let email_response: EmailGetResponse =
                    serde_json::from_value(method_response.1.clone())
                        .map_err(|e| JmapError::Parse(e.to_string()))?;
                log_info!(
                    "[JMAP] Email/get returned {} emails",
                    email_response.list.len()
                );
                return Ok(email_response.list);
            }
        }

        Err(JmapError::Api("Unexpected response".to_string()))
    }

    pub fn get_email(&self, id: &str) -> Result<Option<Email>, JmapError> {
        let emails = self.get_emails(&[id.to_string()])?;
        Ok(emails.into_iter().next())
    }

    pub fn get_email_for_reply(&self, id: &str) -> Result<Option<Email>, JmapError> {
        log_info!("[JMAP] Email/get for reply: {}", id);

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/get",
                json!({
                    "accountId": self.account_id,
                    "ids": [id],
                    "properties": [
                        "id", "from", "to", "cc", "replyTo", "subject",
                        "receivedAt", "sentAt", "textBody", "bodyValues",
                        "messageId", "references"
                    ],
                    "fetchTextBodyValues": true
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/get" {
                let email_response: EmailGetResponse =
                    serde_json::from_value(method_response.1.clone())
                        .map_err(|e| JmapError::Parse(e.to_string()))?;
                return Ok(email_response.list.into_iter().next());
            }
        }

        Err(JmapError::Api("Unexpected response".to_string()))
    }

    pub fn mark_email_read(&self, id: &str) -> Result<(), JmapError> {
        log_info!("[JMAP] Email/set marking as read: {}", id);

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/set",
                json!({
                    "accountId": self.account_id,
                    "update": {
                        id: {
                            "keywords/$seen": true
                        }
                    }
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/set" {
                if let Some(not_updated) = method_response.1.get("notUpdated") {
                    if not_updated.get(id).is_some() {
                        return Err(JmapError::Api(format!(
                            "Failed to mark email as read: {:?}",
                            not_updated
                        )));
                    }
                }
                return Ok(());
            }
        }

        Err(JmapError::Api(
            "Unexpected response for Email/set".to_string(),
        ))
    }

    pub fn mark_email_unread(&self, id: &str) -> Result<(), JmapError> {
        log_info!("[JMAP] Email/set marking as unread: {}", id);

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/set",
                json!({
                    "accountId": self.account_id,
                    "update": {
                        id: {
                            "keywords/$seen": null
                        }
                    }
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/set" {
                if let Some(not_updated) = method_response.1.get("notUpdated") {
                    if not_updated.get(id).is_some() {
                        return Err(JmapError::Api(format!(
                            "Failed to mark email as unread: {:?}",
                            not_updated
                        )));
                    }
                }
                return Ok(());
            }
        }

        Err(JmapError::Api(
            "Unexpected response for Email/set".to_string(),
        ))
    }

    pub fn set_email_flagged(&self, id: &str, flagged: bool) -> Result<(), JmapError> {
        log_info!("[JMAP] Email/set flagged={} for: {}", flagged, id);

        let update_val = if flagged {
            json!({ "keywords/$flagged": true })
        } else {
            json!({ "keywords/$flagged": null })
        };

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/set",
                json!({
                    "accountId": self.account_id,
                    "update": {
                        id: update_val
                    }
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/set" {
                if let Some(not_updated) = method_response.1.get("notUpdated") {
                    if not_updated.get(id).is_some() {
                        return Err(JmapError::Api(format!(
                            "Failed to set email flagged: {:?}",
                            not_updated
                        )));
                    }
                }
                return Ok(());
            }
        }

        Err(JmapError::Api(
            "Unexpected response for Email/set".to_string(),
        ))
    }

    pub fn move_email(&self, id: &str, to_mailbox_id: &str) -> Result<(), JmapError> {
        log_info!(
            "[JMAP] Email/set moving {} to mailbox {}",
            id,
            to_mailbox_id
        );

        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/set",
                json!({
                    "accountId": self.account_id,
                    "update": {
                        id: {
                            "mailboxIds": { to_mailbox_id: true }
                        }
                    }
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/set" {
                if let Some(not_updated) = method_response.1.get("notUpdated") {
                    if not_updated.get(id).is_some() {
                        return Err(JmapError::Api(format!(
                            "Failed to move email: {:?}",
                            not_updated
                        )));
                    }
                }
                return Ok(());
            }
        }

        Err(JmapError::Api(
            "Unexpected response for Email/set".to_string(),
        ))
    }

    pub fn download_blob(
        &self,
        blob_id: &str,
        name: &str,
        content_type: &str,
    ) -> Result<Vec<u8>, JmapError> {
        let download_url = match &self.download_url {
            Some(url) => url,
            None => {
                return Err(JmapError::Api("No download URL available".to_string()));
            }
        };

        let url = download_url
            .replace("{accountId}", &self.account_id)
            .replace("{blobId}", blob_id)
            .replace("{name}", name)
            .replace("{type}", content_type);

        log_debug!("[JMAP] Downloading blob from: {}", url);

        let auth = Self::auth_header(&self.username, &self.password);
        let agent = ureq::AgentBuilder::new().redirects(0).build();

        let mut current_url = url;
        for _ in 0..5 {
            let response = agent
                .get(&current_url)
                .set("Authorization", &auth)
                .call();

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if (300..400).contains(&status) {
                        if let Some(location) = resp.header("location") {
                            current_url = Self::resolve_redirect(&current_url, location);
                            continue;
                        }
                        return Err(JmapError::Http(format!(
                            "Redirect {} without Location header",
                            status
                        )));
                    }

                    let mut bytes = Vec::new();
                    resp.into_reader()
                        .read_to_end(&mut bytes)
                        .map_err(|e| JmapError::Parse(format!("Failed to read blob: {}", e)))?;

                    log_info!("[JMAP] Blob downloaded, {} bytes", bytes.len());
                    return Ok(bytes);
                }
                Err(ureq::Error::Status(code, resp)) if (300..400).contains(&code) => {
                    if let Some(location) = resp.header("location") {
                        current_url = Self::resolve_redirect(&current_url, location);
                    } else {
                        return Err(JmapError::Http(format!(
                            "Redirect {} without Location header",
                            code
                        )));
                    }
                }
                Err(ureq::Error::Status(code, _)) => {
                    return Err(JmapError::Http(format!("HTTP {} error", code)));
                }
                Err(e) => {
                    return Err(JmapError::Http(e.to_string()));
                }
            }
        }

        Err(JmapError::Http("Too many redirects".to_string()))
    }

    pub fn get_email_raw(&self, id: &str) -> Result<Option<String>, JmapError> {
        log_info!("[JMAP] Fetching raw email via blob: {}", id);

        // First, get the blobId for this email
        let request = JmapRequest {
            using: vec!["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            method_calls: vec![MethodCall(
                "Email/get",
                json!({
                    "accountId": self.account_id,
                    "ids": [id],
                    "properties": ["id", "blobId"]
                }),
                "0".to_string(),
            )],
        };

        let response = self.call(request)?;

        let blob_id = if let Some(method_response) = response.method_responses.first() {
            if method_response.0 == "Email/get" {
                method_response.1["list"]
                    .as_array()
                    .and_then(|list| list.first())
                    .and_then(|email| email["blobId"].as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        } else {
            None
        };

        let blob_id = match blob_id {
            Some(id) => id,
            None => {
                log_error!("[JMAP] No blobId found for email: {}", id);
                return Ok(None);
            }
        };

        let download_url = match &self.download_url {
            Some(url) => url,
            None => {
                return Err(JmapError::Api("No download URL available".to_string()));
            }
        };

        let url = download_url
            .replace("{accountId}", &self.account_id)
            .replace("{blobId}", &blob_id)
            .replace("{name}", "email.eml")
            .replace("{type}", "message/rfc822");

        log_debug!("[JMAP] Downloading blob from: {}", url);

        let auth = Self::auth_header(&self.username, &self.password);
        let (_, body) = Self::fetch_with_auth_following_redirects(&url, &auth, 5)?;

        log_info!("[JMAP] Raw email downloaded, {} bytes", body.len());
        Ok(Some(body))
    }
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a valid UTF-8 boundary
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
