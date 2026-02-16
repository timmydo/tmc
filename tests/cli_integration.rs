mod mock_jmap;

use mock_jmap::MockJmapServer;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

struct CliHarness {
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    _server: MockJmapServer,
    _config_dir: tempfile::TempDir,
}

impl CliHarness {
    fn start() -> Self {
        let server = MockJmapServer::start();
        let config_dir = tempfile::tempdir().expect("create temp dir");
        let config_path = config_dir.path().join("config.toml");

        let config_content = format!(
            r#"[account.test]
well_known_url = "{}/.well-known/jmap"
username = "test@example.com"
password_command = "echo test"
"#,
            server.url()
        );
        std::fs::write(&config_path, config_content).expect("write config");

        let tmc_bin = env!("CARGO_BIN_EXE_tmc");
        let mut child = Command::new(tmc_bin)
            .arg("--cli")
            .arg(format!("--config={}", config_path.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tmc --cli");

        let stdin = child.stdin.take().expect("take stdin");
        let stdout = child.stdout.take().expect("take stdout");
        let reader = BufReader::new(stdout);

        CliHarness {
            child,
            stdin,
            reader,
            _server: server,
            _config_dir: config_dir,
        }
    }

    fn send(&mut self, cmd: Value) -> Value {
        let line = serde_json::to_string(&cmd).expect("serialize command");
        writeln!(self.stdin, "{}", line).expect("write to stdin");
        self.stdin.flush().expect("flush stdin");

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("read response");
        serde_json::from_str(response_line.trim()).expect("parse response JSON")
    }
}

impl Drop for CliHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn test_list_accounts() {
    let mut h = CliHarness::start();
    let resp = h.send(json!({"command": "list_accounts"}));

    assert_eq!(resp["ok"], true);
    let accounts = resp["accounts"].as_array().expect("accounts array");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["name"], "test");
    assert_eq!(accounts[0]["username"], "test@example.com");
}

#[test]
fn test_status_before_connect() {
    let mut h = CliHarness::start();
    let resp = h.send(json!({"command": "status"}));

    assert_eq!(resp["ok"], true);
    assert_eq!(resp["connected"], false);
    assert!(resp["account"].is_null());
}

#[test]
fn test_connect_and_status() {
    let mut h = CliHarness::start();

    let resp = h.send(json!({"command": "connect", "account": "test"}));
    assert_eq!(resp["ok"], true, "connect failed: {}", resp);
    assert_eq!(resp["account"], "test");
    assert_eq!(resp["username"], "test@example.com");

    let resp = h.send(json!({"command": "status"}));
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["connected"], true);
    assert_eq!(resp["account"], "test");
}

#[test]
fn test_connect_and_list_mailboxes() {
    let mut h = CliHarness::start();

    let resp = h.send(json!({"command": "connect", "account": "test"}));
    assert_eq!(resp["ok"], true, "connect failed: {}", resp);

    let resp = h.send(json!({"command": "list_mailboxes"}));
    assert_eq!(resp["ok"], true, "list_mailboxes failed: {}", resp);

    let mailboxes = resp["mailboxes"].as_array().expect("mailboxes array");
    assert_eq!(mailboxes.len(), 3);

    let names: Vec<&str> = mailboxes
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"INBOX"));
    assert!(names.contains(&"Archive"));
    assert!(names.contains(&"Trash"));
}

#[test]
fn test_query_and_get_email() {
    let mut h = CliHarness::start();

    let resp = h.send(json!({"command": "connect", "account": "test"}));
    assert_eq!(resp["ok"], true, "connect failed: {}", resp);

    let resp = h.send(json!({
        "command": "query_emails",
        "mailbox_id": "mbox-inbox",
        "limit": 50
    }));
    assert_eq!(resp["ok"], true, "query_emails failed: {}", resp);

    let emails = resp["emails"].as_array().expect("emails array");
    assert_eq!(emails.len(), 3);
    assert_eq!(resp["total"], 3);

    // Get a specific email
    let resp = h.send(json!({"command": "get_email", "id": "email-001"}));
    assert_eq!(resp["ok"], true, "get_email failed: {}", resp);
    assert_eq!(resp["id"], "email-001");
    assert_eq!(resp["subject"], "Hello World");
    assert!(resp["body"].as_str().unwrap().contains("body of email 001"));
}

#[test]
fn test_mark_read_unread() {
    let mut h = CliHarness::start();

    let resp = h.send(json!({"command": "connect", "account": "test"}));
    assert_eq!(resp["ok"], true, "connect failed: {}", resp);

    let resp = h.send(json!({"command": "mark_read", "id": "email-002"}));
    assert_eq!(resp["ok"], true, "mark_read failed: {}", resp);
    assert_eq!(resp["action"], "MarkRead");

    let resp = h.send(json!({"command": "mark_unread", "id": "email-002"}));
    assert_eq!(resp["ok"], true, "mark_unread failed: {}", resp);
    assert_eq!(resp["action"], "MarkUnread");
}
