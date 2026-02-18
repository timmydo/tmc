use crate::jmap::types::{Email, Mailbox};
use redb::{Database, TableDefinition};
use std::path::PathBuf;

const EMAILS: TableDefinition<&str, &[u8]> = TableDefinition::new("emails");
const RULES_PROCESSED: TableDefinition<&str, &[u8]> = TableDefinition::new("rules_processed");
const MAILBOX_INDEX: TableDefinition<&str, &[u8]> = TableDefinition::new("mailbox_index");
const MAILBOXES: TableDefinition<&str, &[u8]> = TableDefinition::new("mailboxes");

pub struct Cache {
    db: Database,
}

fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("tmc")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("tmc")
    } else {
        PathBuf::from("/tmp").join("tmc-cache")
    }
}

fn db_path(account_name: &str) -> PathBuf {
    let safe_name = account_name.replace(['/', '\\', '\0'], "_");
    cache_dir().join(format!("{}.redb", safe_name))
}

impl Cache {
    pub fn open(account_name: &str) -> Result<Cache, String> {
        let path = db_path(account_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create cache dir: {}", e))?;
        }
        let db = Database::create(&path)
            .map_err(|e| format!("failed to open cache db at {}: {}", path.display(), e))?;

        // Ensure tables exist
        let txn = db
            .begin_write()
            .map_err(|e| format!("cache write txn: {}", e))?;
        {
            let _ = txn.open_table(EMAILS);
            let _ = txn.open_table(RULES_PROCESSED);
            let _ = txn.open_table(MAILBOX_INDEX);
            let _ = txn.open_table(MAILBOXES);
        }
        txn.commit().map_err(|e| format!("cache commit: {}", e))?;

        Ok(Cache { db })
    }

    pub fn get_email(&self, id: &str) -> Option<Email> {
        let txn = self.db.begin_read().ok()?;
        let table = txn.open_table(EMAILS).ok()?;
        let value = table.get(id).ok()??;
        serde_json::from_slice(value.value()).ok()
    }

    pub fn remove_email(&self, id: &str) {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return,
        };
        {
            if let Ok(mut table) = txn.open_table(EMAILS) {
                let _ = table.remove(id);
            }
        }
        let _ = txn.commit();
    }

    pub fn put_emails(&self, emails: &[Email]) {
        if emails.is_empty() {
            return;
        }
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(e) => {
                log_warn!("[Cache] failed to begin write txn: {}", e);
                return;
            }
        };
        {
            let mut table = match txn.open_table(EMAILS) {
                Ok(t) => t,
                Err(e) => {
                    log_warn!("[Cache] failed to open emails table: {}", e);
                    return;
                }
            };
            for email in emails {
                if let Ok(bytes) = serde_json::to_vec(email) {
                    let _ = table.insert(email.id.as_str(), bytes.as_slice());
                }
            }
        }
        if let Err(e) = txn.commit() {
            log_warn!("[Cache] failed to commit emails: {}", e);
        }
    }

    pub fn get_mailbox_emails(&self, mailbox_id: &str) -> Option<Vec<Email>> {
        let txn = self.db.begin_read().ok()?;
        let index_table = txn.open_table(MAILBOX_INDEX).ok()?;
        let value = index_table.get(mailbox_id).ok()??;
        let email_ids: Vec<String> = serde_json::from_slice(value.value()).ok()?;
        if email_ids.is_empty() {
            return Some(Vec::new());
        }

        let email_table = txn.open_table(EMAILS).ok()?;
        let mut emails = Vec::with_capacity(email_ids.len());
        for id in &email_ids {
            if let Some(entry) = email_table.get(id.as_str()).ok()? {
                if let Ok(email) = serde_json::from_slice::<Email>(entry.value()) {
                    emails.push(email);
                }
            }
        }
        if emails.is_empty() {
            None
        } else {
            Some(emails)
        }
    }

    pub fn put_mailbox_index(&self, mailbox_id: &str, email_ids: &[String]) {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(e) => {
                log_warn!("[Cache] failed to begin write txn: {}", e);
                return;
            }
        };
        {
            let mut table = match txn.open_table(MAILBOX_INDEX) {
                Ok(t) => t,
                Err(e) => {
                    log_warn!("[Cache] failed to open mailbox_index table: {}", e);
                    return;
                }
            };
            if let Ok(bytes) = serde_json::to_vec(email_ids) {
                let _ = table.insert(mailbox_id, bytes.as_slice());
            }
        }
        if let Err(e) = txn.commit() {
            log_warn!("[Cache] failed to commit mailbox_index: {}", e);
        }
    }

    pub fn get_mailboxes(&self) -> Option<Vec<Mailbox>> {
        let txn = self.db.begin_read().ok()?;
        let table = txn.open_table(MAILBOXES).ok()?;
        let value = table.get("mailboxes").ok()??;
        serde_json::from_slice(value.value()).ok()
    }

    pub fn put_mailboxes(&self, mailboxes: &[Mailbox]) {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(e) => {
                log_warn!("[Cache] failed to begin write txn: {}", e);
                return;
            }
        };
        {
            let mut table = match txn.open_table(MAILBOXES) {
                Ok(t) => t,
                Err(e) => {
                    log_warn!("[Cache] failed to open mailboxes table: {}", e);
                    return;
                }
            };
            if let Ok(bytes) = serde_json::to_vec(mailboxes) {
                let _ = table.insert("mailboxes", bytes.as_slice());
            }
        }
        if let Err(e) = txn.commit() {
            log_warn!("[Cache] failed to commit mailboxes: {}", e);
        }
    }

    pub fn filter_unprocessed(&self, ids: &[String]) -> Vec<String> {
        let Ok(txn) = self.db.begin_read() else {
            return ids.to_vec();
        };
        let Ok(table) = txn.open_table(RULES_PROCESSED) else {
            return ids.to_vec();
        };
        ids.iter()
            .filter(|id| !matches!(table.get(id.as_str()), Ok(Some(_))))
            .cloned()
            .collect()
    }

    pub fn mark_rules_processed(&self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(e) => {
                log_warn!("[Cache] failed to begin write txn: {}", e);
                return;
            }
        };
        {
            let mut table = match txn.open_table(RULES_PROCESSED) {
                Ok(t) => t,
                Err(e) => {
                    log_warn!("[Cache] failed to open rules_processed table: {}", e);
                    return;
                }
            };
            let empty: &[u8] = &[];
            for id in ids {
                let _ = table.insert(id.as_str(), empty);
            }
        }
        if let Err(e) = txn.commit() {
            log_warn!("[Cache] failed to commit rules_processed: {}", e);
        }
    }

    pub fn clear_all_accounts() {
        let dir = cache_dir();
        if !dir.exists() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("redb") {
                    if let Err(e) = std::fs::remove_file(&path) {
                        eprintln!(
                            "Warning: failed to remove cache file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
use redb::ReadableTable;

#[cfg(test)]
impl Cache {
    fn is_rules_processed(&self, id: &str) -> bool {
        let Ok(txn) = self.db.begin_read() else {
            return false;
        };
        let Ok(table) = txn.open_table(RULES_PROCESSED) else {
            return false;
        };
        matches!(table.get(id), Ok(Some(_)))
    }

    fn clear(&self) {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return,
        };
        {
            if let Ok(mut table) = txn.open_table(EMAILS) {
                let ids: Vec<String> = table
                    .iter()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|(k, _)| k.value().to_string())
                    .collect();
                for id in &ids {
                    let _ = table.remove(id.as_str());
                }
            }
            if let Ok(mut table) = txn.open_table(RULES_PROCESSED) {
                let ids: Vec<String> = table
                    .iter()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|(k, _)| k.value().to_string())
                    .collect();
                for id in &ids {
                    let _ = table.remove(id.as_str());
                }
            }
            if let Ok(mut table) = txn.open_table(MAILBOX_INDEX) {
                let ids: Vec<String> = table
                    .iter()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|(k, _)| k.value().to_string())
                    .collect();
                for id in &ids {
                    let _ = table.remove(id.as_str());
                }
            }
            if let Ok(mut table) = txn.open_table(MAILBOXES) {
                let ids: Vec<String> = table
                    .iter()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|(k, _)| k.value().to_string())
                    .collect();
                for id in &ids {
                    let _ = table.remove(id.as_str());
                }
            }
        }
        let _ = txn.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_test_email(id: &str) -> Email {
        Email {
            id: id.to_string(),
            thread_id: None,
            from: None,
            to: None,
            cc: None,
            reply_to: None,
            subject: Some(format!("Test {}", id)),
            received_at: None,
            sent_at: None,
            preview: None,
            text_body: None,
            html_body: None,
            body_values: HashMap::new(),
            keywords: HashMap::new(),
            mailbox_ids: HashMap::new(),
            message_id: None,
            references: None,
            attachments: None,
            extra: HashMap::new(),
        }
    }

    #[test]
    fn test_cache_put_get() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_account").unwrap();

        let email = make_test_email("e1");
        cache.put_emails(&[email.clone()]);

        let cached = cache.get_email("e1").unwrap();
        assert_eq!(cached.id, "e1");
        assert_eq!(cached.subject.as_deref(), Some("Test e1"));

        assert!(cache.get_email("nonexistent").is_none());
    }

    #[test]
    fn test_cache_rules_processed() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_rules").unwrap();

        assert!(!cache.is_rules_processed("e1"));

        let unprocessed = cache.filter_unprocessed(&["e1".into(), "e2".into(), "e3".into()]);
        assert_eq!(unprocessed.len(), 3);

        cache.mark_rules_processed(&["e1".into(), "e2".into()]);

        assert!(cache.is_rules_processed("e1"));
        assert!(cache.is_rules_processed("e2"));
        assert!(!cache.is_rules_processed("e3"));

        let unprocessed = cache.filter_unprocessed(&["e1".into(), "e2".into(), "e3".into()]);
        assert_eq!(unprocessed, vec!["e3".to_string()]);
    }

    #[test]
    fn test_cache_mailboxes() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_mailboxes").unwrap();

        assert!(cache.get_mailboxes().is_none());

        let mailboxes = vec![
            Mailbox {
                id: "m1".into(),
                name: "Inbox".into(),
                parent_id: None,
                role: Some("inbox".into()),
                total_emails: 42,
                unread_emails: 3,
                sort_order: 1,
            },
            Mailbox {
                id: "m2".into(),
                name: "Sent".into(),
                parent_id: None,
                role: Some("sent".into()),
                total_emails: 100,
                unread_emails: 0,
                sort_order: 2,
            },
        ];
        cache.put_mailboxes(&mailboxes);

        let cached = cache.get_mailboxes().unwrap();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].id, "m1");
        assert_eq!(cached[0].name, "Inbox");
        assert_eq!(cached[0].unread_emails, 3);
        assert_eq!(cached[1].id, "m2");
        assert_eq!(cached[1].name, "Sent");
    }

    #[test]
    fn test_cache_mailbox_index() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_mbx_idx").unwrap();

        // No index yet
        assert!(cache.get_mailbox_emails("mbx1").is_none());

        // Store emails and index
        let e1 = make_test_email("e1");
        let e2 = make_test_email("e2");
        cache.put_emails(&[e1.clone(), e2.clone()]);
        cache.put_mailbox_index("mbx1", &["e1".into(), "e2".into()]);

        let cached = cache.get_mailbox_emails("mbx1").unwrap();
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].id, "e1");
        assert_eq!(cached[1].id, "e2");

        // Index with missing email returns only found ones
        cache.put_mailbox_index("mbx2", &["e1".into(), "missing".into()]);
        let cached = cache.get_mailbox_emails("mbx2").unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, "e1");

        // Empty index returns empty vec
        cache.put_mailbox_index("mbx3", &[]);
        let cached = cache.get_mailbox_emails("mbx3").unwrap();
        assert!(cached.is_empty());
    }

    #[test]
    fn test_cache_clear() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_clear").unwrap();

        cache.put_emails(&[make_test_email("e1")]);
        cache.mark_rules_processed(&["e1".into()]);
        assert!(cache.get_email("e1").is_some());
        assert!(cache.is_rules_processed("e1"));

        cache.clear();
        assert!(cache.get_email("e1").is_none());
        assert!(!cache.is_rules_processed("e1"));
    }
}
