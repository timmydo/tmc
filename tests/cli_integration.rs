mod mock_jmap;

use mock_jmap::MockJmapServer;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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
        Self::start_with_mail_config("")
    }

    fn start_with_mail_config(mail_config: &str) -> Self {
        Self::start_with_opts(mail_config, false, None)
    }

    fn start_with_opts(mail_config: &str, offline: bool, cache_home: Option<PathBuf>) -> Self {
        let server = MockJmapServer::start();
        let config_dir = tempfile::tempdir().expect("create temp dir");
        let config_path = config_dir.path().join("config.toml");

        let config_content = format!(
            r#"[account.test]
well_known_url = "{}/.well-known/jmap"
username = "test@example.com"
password_command = "echo test"

[mail]
{}
"#,
            server.url(),
            mail_config
        );
        std::fs::write(&config_path, config_content).expect("write config");

        let tmc_bin = env!("CARGO_BIN_EXE_tmc");
        let mut command = Command::new(tmc_bin);
        command
            .arg("--cli")
            .arg(format!("--config={}", config_path.display()));
        if offline {
            command.arg("--offline");
        }
        if let Some(cache_home) = cache_home {
            command.env("XDG_CACHE_HOME", cache_home);
        }
        let mut child = command
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
    assert_eq!(emails.len(), 4);

    let resp = h.send(json!({"command": "get_email", "id": "email-001"}));
    assert_eq!(resp["ok"], true, "get_email failed: {}", resp);
    assert_eq!(resp["id"], "email-001");
    assert_eq!(resp["subject"], "Hello World");
    assert!(resp["body"].as_str().unwrap().contains("body of email 001"));
}

#[test]
fn test_archive_and_delete_work_without_preloading_mailboxes() {
    let mut h = CliHarness::start();
    assert_eq!(
        h.send(json!({"command": "connect", "account": "test"}))["ok"],
        true
    );

    let archive_resp = h.send(json!({"command": "archive", "id": "email-002"}));
    assert_eq!(archive_resp["ok"], true, "archive failed: {}", archive_resp);

    let delete_resp = h.send(json!({"command": "delete_email", "id": "email-003"}));
    assert_eq!(delete_resp["ok"], true, "delete failed: {}", delete_resp);

    let e2 = h.send(json!({"command": "get_email", "id": "email-002", "headers_only": true}));
    let e3 = h.send(json!({"command": "get_email", "id": "email-003", "headers_only": true}));
    let m2 = e2["mailbox_ids"].as_array().unwrap();
    let m3 = e3["mailbox_ids"].as_array().unwrap();
    assert_eq!(m2[0], "mbox-archive");
    assert_eq!(m3[0], "mbox-trash");
}

#[test]
fn test_mailbox_id_overrides_and_bulk_commands() {
    let mut h = CliHarness::start_with_mail_config(
        r#"
archive_folder = "not-a-real-folder"
deleted_folder = "also-not-real"
archive_mailbox_id = "mbox-archive"
deleted_mailbox_id = "mbox-trash"
"#,
    );
    assert_eq!(
        h.send(json!({"command": "connect", "account": "test"}))["ok"],
        true
    );

    let bulk_archive = h.send(json!({
        "command": "bulk_archive",
        "ids": ["email-001", "email-002"]
    }));
    assert_eq!(
        bulk_archive["ok"], true,
        "bulk_archive failed: {}",
        bulk_archive
    );
    assert_eq!(bulk_archive["succeeded"], 2);

    let bulk_delete = h.send(json!({
        "command": "bulk_delete_email",
        "ids": ["email-003"]
    }));
    assert_eq!(
        bulk_delete["ok"], true,
        "bulk_delete failed: {}",
        bulk_delete
    );
    assert_eq!(bulk_delete["succeeded"], 1);

    let inbox = h.send(json!({"command": "query_emails", "mailbox_id": "mbox-inbox", "limit": 50}));
    let ids: Vec<&str> = inbox["emails"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert!(!ids.contains(&"email-001"));
    assert!(!ids.contains(&"email-002"));
    assert!(!ids.contains(&"email-003"));
}

#[test]
fn test_query_emails_date_filters() {
    let mut h = CliHarness::start();
    assert_eq!(
        h.send(json!({"command": "connect", "account": "test"}))["ok"],
        true
    );

    let resp = h.send(json!({
        "command": "query_emails",
        "mailbox_id": "mbox-inbox",
        "received_after": "2025-12-01T00:00:00Z",
        "received_before": "2025-12-21T00:00:00Z",
        "limit": 50
    }));
    assert_eq!(resp["ok"], true, "query with date filters failed: {}", resp);

    let ids: Vec<&str> = resp["emails"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["email-003", "email-002"]);
}

#[test]
fn test_triage_plan_and_apply() {
    let mut h = CliHarness::start();
    assert_eq!(
        h.send(json!({"command": "connect", "account": "test"}))["ok"],
        true
    );

    let plan = h.send(json!({
        "command": "triage_suggest",
        "mailbox_id": "mbox-inbox",
        "received_after": "2025-12-01T00:00:00Z",
        "received_before": "2026-01-01T00:00:00Z",
        "limit": 50
    }));
    assert_eq!(plan["ok"], true, "triage_suggest failed: {}", plan);
    let plan_id = plan["plan_id"].as_str().expect("plan_id");

    let archive_ids: Vec<&str> = plan["archive"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    let trash_ids: Vec<&str> = plan["trash"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();

    assert!(archive_ids.contains(&"email-004"));
    assert!(trash_ids.contains(&"email-003"));

    let apply = h.send(json!({"command": "apply_triage_plan", "plan_id": plan_id}));
    assert_eq!(apply["ok"], true, "apply_triage_plan failed: {}", apply);

    let e4 = h.send(json!({"command": "get_email", "id": "email-004", "headers_only": true}));
    let e3 = h.send(json!({"command": "get_email", "id": "email-003", "headers_only": true}));
    assert_eq!(e4["mailbox_ids"][0], "mbox-archive");
    assert_eq!(e3["mailbox_ids"][0], "mbox-trash");
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

#[test]
fn test_offline_queue_replay_on_reconnect() {
    let cache_dir = tempfile::tempdir().expect("create cache dir");

    // Prime cache from an online session.
    {
        let mut online =
            CliHarness::start_with_opts("", false, Some(cache_dir.path().to_path_buf()));
        assert_eq!(
            online.send(json!({"command": "connect", "account": "test"}))["ok"],
            true
        );
        assert_eq!(
            online.send(json!({"command": "list_mailboxes"}))["ok"],
            true
        );
        assert_eq!(
            online.send(json!({
                "command": "query_emails",
                "mailbox_id": "mbox-inbox",
                "limit": 50
            }))["ok"],
            true
        );
    }

    // Queue writes offline; they should succeed and update local cache projection.
    {
        let mut offline =
            CliHarness::start_with_opts("", true, Some(cache_dir.path().to_path_buf()));
        assert_eq!(
            offline.send(json!({"command": "connect", "account": "test"}))["ok"],
            true
        );

        let mark = offline.send(json!({"command": "mark_read", "id": "email-002"}));
        assert_eq!(mark["ok"], true, "offline mark_read failed: {}", mark);

        let archive = offline.send(json!({"command": "archive", "id": "email-001"}));
        assert_eq!(archive["ok"], true, "offline archive failed: {}", archive);

        let inbox = offline.send(json!({
            "command": "query_emails",
            "mailbox_id": "mbox-inbox",
            "limit": 50
        }));
        assert_eq!(inbox["ok"], true, "offline query failed: {}", inbox);
        let ids: Vec<&str> = inbox["emails"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["id"].as_str())
            .collect();
        assert!(!ids.contains(&"email-001"));
    }

    // Reconnect online with the same cache; queued ops should replay to server.
    {
        let mut online =
            CliHarness::start_with_opts("", false, Some(cache_dir.path().to_path_buf()));
        assert_eq!(
            online.send(json!({"command": "connect", "account": "test"}))["ok"],
            true
        );

        let e1 =
            online.send(json!({"command": "get_email", "id": "email-001", "headers_only": true}));
        assert_eq!(e1["ok"], true, "post-replay get_email e1 failed: {}", e1);
        assert_eq!(e1["mailbox_ids"][0], "mbox-archive");

        let e2 =
            online.send(json!({"command": "get_email", "id": "email-002", "headers_only": true}));
        assert_eq!(e2["ok"], true, "post-replay get_email e2 failed: {}", e2);
        assert_eq!(e2["is_read"], true);
    }
}

#[test]
fn test_download_attachment() {
    let mut h = CliHarness::start();

    let resp = h.send(json!({"command": "connect", "account": "test"}));
    assert_eq!(resp["ok"], true, "connect failed: {}", resp);

    // Verify email-001 has an attachment
    let resp = h.send(json!({"command": "get_email", "id": "email-001"}));
    assert_eq!(resp["ok"], true, "get_email failed: {}", resp);
    let attachments = resp["attachments"].as_array().expect("attachments array");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["name"], "test-document.pdf");
    assert_eq!(attachments[0]["blob_id"], "blob-att-001");

    // Download the attachment
    let resp = h.send(json!({
        "command": "download_attachment",
        "blob_id": "blob-att-001",
        "name": "test-document.pdf",
        "content_type": "application/pdf"
    }));
    assert_eq!(resp["ok"], true, "download_attachment failed: {}", resp);
    assert_eq!(resp["name"], "test-document.pdf");

    let path_str = resp["path"].as_str().expect("path string");
    let path = Path::new(path_str);
    assert!(
        path.exists(),
        "downloaded file should exist at {}",
        path_str
    );

    let contents = std::fs::read_to_string(path).expect("read downloaded file");
    assert!(
        contents.contains("blob-att-001"),
        "file should contain expected content"
    );

    // Clean up
    let _ = std::fs::remove_file(path);
}
