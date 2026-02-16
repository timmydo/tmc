use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

pub struct MockJmapServer {
    port: u16,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockJmapServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let port = listener.local_addr().unwrap().port();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        listener
            .set_nonblocking(true)
            .expect("set_nonblocking on listener");

        let handle = thread::spawn(move || {
            Self::serve(listener, shutdown_clone);
        });

        MockJmapServer {
            port,
            shutdown,
            handle: Some(handle),
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    #[allow(dead_code)]
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    fn serve(listener: TcpListener, shutdown: Arc<AtomicBool>) {
        let port = listener.local_addr().unwrap().port();
        while !shutdown.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_nonblocking(false)
                        .expect("set blocking on stream");
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .ok();
                    Self::handle_connection(stream, port);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Err(_) => break,
            }
        }
    }

    fn handle_connection(mut stream: std::net::TcpStream, port: u16) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));

        // Read request line
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }

        // Read headers
        let mut content_length: usize = 0;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).is_err() {
                return;
            }
            let trimmed = header.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(val) = trimmed.strip_prefix("Content-Length:") {
                if let Ok(len) = val.trim().parse() {
                    content_length = len;
                }
            }
            // Also handle lowercase
            if let Some(val) = trimmed.strip_prefix("content-length:") {
                if let Ok(len) = val.trim().parse() {
                    content_length = len;
                }
            }
        }

        // Read body if present
        let body = if content_length > 0 {
            let mut buf = vec![0u8; content_length];
            if reader.read_exact(&mut buf).is_err() {
                return;
            }
            String::from_utf8_lossy(&buf).to_string()
        } else {
            String::new()
        };

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return;
        }
        let method = parts[0];
        let path = parts[1];

        let (status, response_body) = if method == "GET" && path.contains("/.well-known/jmap") {
            Self::handle_session(port)
        } else if method == "POST" && path.contains("/api") {
            Self::handle_api(&body)
        } else {
            (
                "404 Not Found".to_string(),
                json!({"error": "not found"}).to_string(),
            )
        };

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }

    fn handle_session(port: u16) -> (String, String) {
        let session = json!({
            "username": "test@example.com",
            "apiUrl": format!("http://127.0.0.1:{}/api", port),
            "downloadUrl": format!("http://127.0.0.1:{}/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}", port),
            "primaryAccounts": {
                "urn:ietf:params:jmap:mail": "account-001"
            },
            "accounts": {
                "account-001": {
                    "name": "Test Account",
                    "isPersonal": true,
                    "isReadOnly": false
                }
            }
        });
        // Note: the apiUrl will be fixed up by the test harness writing the correct port in config
        ("200 OK".to_string(), session.to_string())
    }

    fn handle_api(body: &str) -> (String, String) {
        let request: Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(_) => {
                return (
                    "400 Bad Request".to_string(),
                    json!({"error": "invalid JSON"}).to_string(),
                );
            }
        };

        let method_calls = match request.get("methodCalls").and_then(|v| v.as_array()) {
            Some(calls) => calls,
            None => {
                return (
                    "400 Bad Request".to_string(),
                    json!({"error": "missing methodCalls"}).to_string(),
                );
            }
        };

        let mut responses = Vec::new();

        for call in method_calls {
            let arr = match call.as_array() {
                Some(a) if a.len() >= 3 => a,
                _ => continue,
            };
            let method_name = arr[0].as_str().unwrap_or("");
            let _args = &arr[1];
            let call_id = arr[2].as_str().unwrap_or("0");

            let response = match method_name {
                "Mailbox/get" => json!([
                    "Mailbox/get",
                    {
                        "accountId": "account-001",
                        "state": "state-001",
                        "list": [
                            {
                                "id": "mbox-inbox",
                                "name": "INBOX",
                                "role": "inbox",
                                "totalEmails": 3,
                                "unreadEmails": 1,
                                "sortOrder": 1
                            },
                            {
                                "id": "mbox-archive",
                                "name": "Archive",
                                "role": "archive",
                                "totalEmails": 100,
                                "unreadEmails": 0,
                                "sortOrder": 3
                            },
                            {
                                "id": "mbox-trash",
                                "name": "Trash",
                                "role": "trash",
                                "totalEmails": 5,
                                "unreadEmails": 0,
                                "sortOrder": 4
                            }
                        ],
                        "notFound": []
                    },
                    call_id
                ]),
                "Email/query" => json!([
                    "Email/query",
                    {
                        "accountId": "account-001",
                        "queryState": "qstate-001",
                        "ids": ["email-001", "email-002", "email-003"],
                        "position": 0,
                        "total": 3
                    },
                    call_id
                ]),
                "Email/get" => {
                    let requested_ids = _args
                        .get("ids")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                        .unwrap_or_default();

                    let all_emails = vec![
                        test_email(
                            "email-001",
                            "thread-001",
                            "Alice <alice@example.com>",
                            "Hello World",
                            "This is the body of email 001.",
                            "mbox-inbox",
                            true,
                        ),
                        test_email(
                            "email-002",
                            "thread-002",
                            "Bob <bob@example.com>",
                            "Meeting Tomorrow",
                            "Let's meet at 10am.",
                            "mbox-inbox",
                            false,
                        ),
                        test_email(
                            "email-003",
                            "thread-003",
                            "Carol <carol@example.com>",
                            "Project Update",
                            "Here is the update.",
                            "mbox-inbox",
                            false,
                        ),
                    ];

                    let list: Vec<Value> = if requested_ids.is_empty() {
                        all_emails
                    } else {
                        all_emails
                            .into_iter()
                            .filter(|e| {
                                let id = e.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                requested_ids.contains(&id)
                            })
                            .collect()
                    };

                    json!([
                        "Email/get",
                        {
                            "accountId": "account-001",
                            "state": "estate-001",
                            "list": list,
                            "notFound": []
                        },
                        call_id
                    ])
                }
                "Email/set" => json!([
                    "Email/set",
                    {
                        "accountId": "account-001",
                        "oldState": "estate-001",
                        "newState": "estate-002",
                        "updated": {
                            "email-001": null
                        }
                    },
                    call_id
                ]),
                "Thread/get" => json!([
                    "Thread/get",
                    {
                        "accountId": "account-001",
                        "state": "tstate-001",
                        "list": [
                            {
                                "id": "thread-001",
                                "emailIds": ["email-001"]
                            }
                        ],
                        "notFound": []
                    },
                    call_id
                ]),
                _ => json!([
                    "error",
                    {
                        "type": "unknownMethod",
                        "description": format!("Unknown method: {}", method_name)
                    },
                    call_id
                ]),
            };
            responses.push(response);
        }

        let jmap_response = json!({
            "methodResponses": responses,
            "sessionState": "session-001"
        });

        ("200 OK".to_string(), jmap_response.to_string())
    }
}

impl Drop for MockJmapServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn test_email(
    id: &str,
    thread_id: &str,
    from: &str,
    subject: &str,
    body_text: &str,
    mailbox_id: &str,
    is_read: bool,
) -> Value {
    let (name, email) = if let Some(idx) = from.find('<') {
        let name = from[..idx].trim().to_string();
        let email = from[idx + 1..].trim_end_matches('>').to_string();
        (Some(name), Some(email))
    } else {
        (None, Some(from.to_string()))
    };

    let mut keywords = serde_json::Map::new();
    if is_read {
        keywords.insert("$seen".to_string(), json!(true));
    }

    json!({
        "id": id,
        "threadId": thread_id,
        "from": [{"name": name, "email": email}],
        "to": [{"name": "Test User", "email": "test@example.com"}],
        "cc": null,
        "replyTo": null,
        "subject": subject,
        "receivedAt": "2025-01-15T10:30:00Z",
        "sentAt": "2025-01-15T10:30:00Z",
        "preview": &body_text[..body_text.len().min(100)],
        "textBody": [{"partId": "1"}],
        "bodyValues": {"1": {"value": body_text, "isEncodingProblem": false, "isTruncated": false}},
        "keywords": keywords,
        "mailboxIds": {mailbox_id: true},
        "messageId": [format!("<{}@example.com>", id)],
        "references": null,
        "attachments": []
    })
}
