use crate::jmap::types::{Email, Mailbox};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::PathBuf;

const EMAILS: TableDefinition<&str, &[u8]> = TableDefinition::new("emails");
const RULES_PROCESSED: TableDefinition<&str, &[u8]> = TableDefinition::new("rules_processed");
const MAILBOX_INDEX: TableDefinition<&str, &[u8]> = TableDefinition::new("mailbox_index");
const MAILBOXES: TableDefinition<&str, &[u8]> = TableDefinition::new("mailboxes");
const OP_QUEUE: TableDefinition<u64, &[u8]> = TableDefinition::new("op_queue");

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
            let _ = txn.open_table(OP_QUEUE);
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

    #[allow(dead_code)]
    pub fn update_email_seen(&self, id: &str, seen: bool) {
        let _ = self.apply_mark_seen(id, seen);
    }

    #[allow(dead_code)]
    pub fn update_email_flagged(&self, id: &str, flagged: bool) {
        let _ = self.apply_set_flagged(id, flagged);
    }

    #[allow(dead_code)]
    pub fn remove_email(&self, id: &str) {
        let _ = self.apply_destroy_email(id);
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

    pub fn get_thread_emails(&self, thread_id: &str) -> Vec<Email> {
        let Ok(txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(table) = txn.open_table(EMAILS) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let Ok(iter) = table.iter() else {
            return out;
        };
        for entry in iter {
            let Ok((_, value)) = entry else {
                continue;
            };
            if let Ok(email) = serde_json::from_slice::<Email>(value.value()) {
                if email.thread_id.as_deref() == Some(thread_id) {
                    out.push(email);
                }
            }
        }
        out
    }

    pub fn apply_mark_seen(&self, id: &str, seen: bool) -> bool {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return false,
        };

        let mut email = {
            let Ok(email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Some(raw) = email_table.get(id).ok().flatten() else {
                return false;
            };
            let raw = raw.value().to_vec();
            let Ok(email) = serde_json::from_slice::<Email>(&raw) else {
                return false;
            };
            email
        };

        let was_seen = email.keywords.contains_key("$seen");
        if was_seen == seen {
            return false;
        }

        if seen {
            email.keywords.insert("$seen".to_string(), true);
        } else {
            email.keywords.remove("$seen");
        }

        let mut adjusted_mailboxes = if let Ok(mailboxes_table) = txn.open_table(MAILBOXES) {
            if let Some(raw) = mailboxes_table.get("mailboxes").ok().flatten() {
                let raw = raw.value().to_vec();
                serde_json::from_slice::<Vec<Mailbox>>(&raw).ok()
            } else {
                None
            }
        } else {
            None
        };

        if let Some(ref mut mailboxes) = adjusted_mailboxes {
            for mailbox in mailboxes {
                if email.mailbox_ids.contains_key(&mailbox.id) {
                    if seen {
                        mailbox.unread_emails = mailbox.unread_emails.saturating_sub(1);
                    } else {
                        mailbox.unread_emails = mailbox.unread_emails.saturating_add(1);
                    }
                }
            }
        }

        {
            let Ok(mut email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Ok(bytes) = serde_json::to_vec(&email) else {
                return false;
            };
            let _ = email_table.insert(id, bytes.as_slice());
        }

        if let Some(mailboxes) = adjusted_mailboxes {
            if let Ok(mut mailboxes_table) = txn.open_table(MAILBOXES) {
                if let Ok(bytes) = serde_json::to_vec(&mailboxes) {
                    let _ = mailboxes_table.insert("mailboxes", bytes.as_slice());
                }
            }
        }

        txn.commit().is_ok()
    }

    pub fn apply_set_flagged(&self, id: &str, flagged: bool) -> bool {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut email = {
            let Ok(email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Some(raw) = email_table.get(id).ok().flatten() else {
                return false;
            };
            let raw = raw.value().to_vec();
            let Ok(email) = serde_json::from_slice::<Email>(&raw) else {
                return false;
            };
            email
        };

        let was_flagged = email.keywords.contains_key("$flagged");
        if was_flagged == flagged {
            return false;
        }
        if flagged {
            email.keywords.insert("$flagged".to_string(), true);
        } else {
            email.keywords.remove("$flagged");
        }
        {
            let Ok(mut email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Ok(bytes) = serde_json::to_vec(&email) else {
                return false;
            };
            let _ = email_table.insert(id, bytes.as_slice());
        }
        txn.commit().is_ok()
    }

    pub fn apply_move_email(&self, id: &str, to_mailbox_id: &str) -> bool {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut email = {
            let Ok(email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Some(raw) = email_table.get(id).ok().flatten() else {
                return false;
            };
            let raw = raw.value().to_vec();
            let Ok(email) = serde_json::from_slice::<Email>(&raw) else {
                return false;
            };
            email
        };
        let was_seen = email.keywords.contains_key("$seen");
        let previous_mailboxes: Vec<String> = email.mailbox_ids.keys().cloned().collect();

        email.mailbox_ids.clear();
        email.mailbox_ids.insert(to_mailbox_id.to_string(), true);

        {
            let Ok(mut email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Ok(bytes) = serde_json::to_vec(&email) else {
                return false;
            };
            let _ = email_table.insert(id, bytes.as_slice());
        }

        if let Ok(mut index_table) = txn.open_table(MAILBOX_INDEX) {
            for mailbox_id in &previous_mailboxes {
                let maybe_raw = index_table
                    .get(mailbox_id.as_str())
                    .ok()
                    .flatten()
                    .map(|v| v.value().to_vec());
                if let Some(raw) = maybe_raw {
                    if let Ok(mut ids) = serde_json::from_slice::<Vec<String>>(&raw) {
                        ids.retain(|eid| eid != id);
                        if let Ok(bytes) = serde_json::to_vec(&ids) {
                            let _ = index_table.insert(mailbox_id.as_str(), bytes.as_slice());
                        }
                    }
                }
            }
            let target_raw = index_table
                .get(to_mailbox_id)
                .ok()
                .flatten()
                .map(|v| v.value().to_vec());
            let mut target_ids = target_raw
                .as_ref()
                .and_then(|v| serde_json::from_slice::<Vec<String>>(v).ok())
                .unwrap_or_default();
            target_ids.retain(|eid| eid != id);
            target_ids.insert(0, id.to_string());
            if let Ok(bytes) = serde_json::to_vec(&target_ids) {
                let _ = index_table.insert(to_mailbox_id, bytes.as_slice());
            }
        }

        if let Ok(mut mailbox_table) = txn.open_table(MAILBOXES) {
            let raw = mailbox_table
                .get("mailboxes")
                .ok()
                .flatten()
                .map(|v| v.value().to_vec());
            if let Some(raw) = raw {
                if let Ok(mut mailboxes) = serde_json::from_slice::<Vec<Mailbox>>(&raw) {
                    for mailbox in &mut mailboxes {
                        let was_in = previous_mailboxes.iter().any(|m| m == &mailbox.id);
                        let now_in = mailbox.id == to_mailbox_id;
                        if was_in && !now_in {
                            mailbox.total_emails = mailbox.total_emails.saturating_sub(1);
                            if !was_seen {
                                mailbox.unread_emails = mailbox.unread_emails.saturating_sub(1);
                            }
                        } else if !was_in && now_in {
                            mailbox.total_emails = mailbox.total_emails.saturating_add(1);
                            if !was_seen {
                                mailbox.unread_emails = mailbox.unread_emails.saturating_add(1);
                            }
                        }
                    }
                    if let Ok(bytes) = serde_json::to_vec(&mailboxes) {
                        let _ = mailbox_table.insert("mailboxes", bytes.as_slice());
                    }
                }
            }
        }

        txn.commit().is_ok()
    }

    pub fn apply_destroy_email(&self, id: &str) -> bool {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return false,
        };

        let existing_email = {
            let Ok(email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let Some(raw) = email_table.get(id).ok().flatten() else {
                return false;
            };
            let raw = raw.value().to_vec();
            let Ok(email) = serde_json::from_slice::<Email>(&raw) else {
                return false;
            };
            email
        };

        let was_seen = existing_email.keywords.contains_key("$seen");
        let mailbox_ids: Vec<String> = existing_email.mailbox_ids.keys().cloned().collect();

        {
            let Ok(mut email_table) = txn.open_table(EMAILS) else {
                return false;
            };
            let _ = email_table.remove(id);
        }

        if let Ok(mut index_table) = txn.open_table(MAILBOX_INDEX) {
            for mailbox_id in &mailbox_ids {
                let maybe_raw = index_table
                    .get(mailbox_id.as_str())
                    .ok()
                    .flatten()
                    .map(|v| v.value().to_vec());
                if let Some(raw) = maybe_raw {
                    if let Ok(mut ids) = serde_json::from_slice::<Vec<String>>(&raw) {
                        ids.retain(|eid| eid != id);
                        if let Ok(bytes) = serde_json::to_vec(&ids) {
                            let _ = index_table.insert(mailbox_id.as_str(), bytes.as_slice());
                        }
                    }
                }
            }
        }

        if let Ok(mut mailbox_table) = txn.open_table(MAILBOXES) {
            let raw = mailbox_table
                .get("mailboxes")
                .ok()
                .flatten()
                .map(|v| v.value().to_vec());
            if let Some(raw) = raw {
                if let Ok(mut mailboxes) = serde_json::from_slice::<Vec<Mailbox>>(&raw) {
                    for mailbox in &mut mailboxes {
                        if mailbox_ids.iter().any(|m| m == &mailbox.id) {
                            mailbox.total_emails = mailbox.total_emails.saturating_sub(1);
                            if !was_seen {
                                mailbox.unread_emails = mailbox.unread_emails.saturating_sub(1);
                            }
                        }
                    }
                    if let Ok(bytes) = serde_json::to_vec(&mailboxes) {
                        let _ = mailbox_table.insert("mailboxes", bytes.as_slice());
                    }
                }
            }
        }

        txn.commit().is_ok()
    }

    pub fn apply_mark_mailbox_read(&self, mailbox_id: &str) -> usize {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return 0,
        };

        let mailbox_email_ids = {
            let Ok(index_table) = txn.open_table(MAILBOX_INDEX) else {
                return 0;
            };
            let raw = index_table
                .get(mailbox_id)
                .ok()
                .flatten()
                .map(|v| v.value().to_vec());
            raw.as_ref()
                .and_then(|v| serde_json::from_slice::<Vec<String>>(v).ok())
                .unwrap_or_default()
        };
        if mailbox_email_ids.is_empty() {
            return 0;
        }

        let mut changed = 0usize;
        let mut unread_delta_by_mailbox: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        {
            let Ok(mut email_table) = txn.open_table(EMAILS) else {
                return 0;
            };
            for id in &mailbox_email_ids {
                let raw = email_table
                    .get(id.as_str())
                    .ok()
                    .flatten()
                    .map(|v| v.value().to_vec());
                let Some(raw) = raw else {
                    continue;
                };
                let Ok(mut email) = serde_json::from_slice::<Email>(&raw) else {
                    continue;
                };
                if email.keywords.contains_key("$seen") {
                    continue;
                }
                email.keywords.insert("$seen".to_string(), true);
                if let Ok(bytes) = serde_json::to_vec(&email) {
                    let _ = email_table.insert(id.as_str(), bytes.as_slice());
                }
                changed = changed.saturating_add(1);
                for mbx in email.mailbox_ids.keys() {
                    *unread_delta_by_mailbox.entry(mbx.clone()).or_insert(0) += 1;
                }
            }
        }

        if changed > 0 {
            if let Ok(mut mailbox_table) = txn.open_table(MAILBOXES) {
                let raw = mailbox_table
                    .get("mailboxes")
                    .ok()
                    .flatten()
                    .map(|v| v.value().to_vec());
                if let Some(raw) = raw {
                    if let Ok(mut mailboxes) = serde_json::from_slice::<Vec<Mailbox>>(&raw) {
                        for mailbox in &mut mailboxes {
                            if let Some(delta) = unread_delta_by_mailbox.get(&mailbox.id) {
                                mailbox.unread_emails =
                                    mailbox.unread_emails.saturating_sub(*delta);
                            }
                        }
                        if let Ok(bytes) = serde_json::to_vec(&mailboxes) {
                            let _ = mailbox_table.insert("mailboxes", bytes.as_slice());
                        }
                    }
                }
            }
        }

        if txn.commit().is_err() {
            return 0;
        }
        changed
    }

    pub fn enqueue_operation(&self, payload: &[u8]) -> Result<u64, String> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| format!("queue write txn: {}", e))?;
        let seq = {
            let mut table = txn
                .open_table(OP_QUEUE)
                .map_err(|e| format!("queue open table: {}", e))?;
            let mut max_id = 0u64;
            if let Ok(iter) = table.iter() {
                for entry in iter {
                    if let Ok((k, _)) = entry {
                        max_id = max_id.max(k.value());
                    }
                }
            }
            let seq = max_id.saturating_add(1);
            table
                .insert(seq, payload)
                .map_err(|e| format!("queue insert: {}", e))?;
            seq
        };
        txn.commit().map_err(|e| format!("queue commit: {}", e))?;
        Ok(seq)
    }

    pub fn queued_operations(&self) -> Vec<(u64, Vec<u8>)> {
        let Ok(txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(table) = txn.open_table(OP_QUEUE) else {
            return Vec::new();
        };
        let Ok(iter) = table.iter() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in iter {
            if let Ok((k, v)) = entry {
                out.push((k.value(), v.value().to_vec()));
            }
        }
        out
    }

    pub fn remove_queued_operation(&self, seq: u64) -> bool {
        let txn = match self.db.begin_write() {
            Ok(t) => t,
            Err(_) => return false,
        };
        {
            let Ok(mut table) = txn.open_table(OP_QUEUE) else {
                return false;
            };
            let _ = table.remove(seq);
        }
        txn.commit().is_ok()
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
            if let Ok(mut table) = txn.open_table(OP_QUEUE) {
                let ids: Vec<u64> = table
                    .iter()
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|(k, _)| k.value())
                    .collect();
                for id in ids {
                    let _ = table.remove(id);
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

    #[test]
    fn test_cache_update_email_seen() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_seen").unwrap();

        let email = make_test_email("e1");
        assert!(!email.keywords.contains_key("$seen"));
        cache.put_emails(&[email]);

        // Mark as seen
        cache.update_email_seen("e1", true);
        let cached = cache.get_email("e1").unwrap();
        assert!(cached.keywords.contains_key("$seen"));

        // Mark as unseen
        cache.update_email_seen("e1", false);
        let cached = cache.get_email("e1").unwrap();
        assert!(!cached.keywords.contains_key("$seen"));

        // No-op on missing email
        cache.update_email_seen("nonexistent", true);
        assert!(cache.get_email("nonexistent").is_none());
    }

    #[test]
    fn test_cache_update_email_flagged() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_flagged").unwrap();

        let email = make_test_email("e1");
        assert!(!email.keywords.contains_key("$flagged"));
        cache.put_emails(&[email]);

        // Flag
        cache.update_email_flagged("e1", true);
        let cached = cache.get_email("e1").unwrap();
        assert!(cached.keywords.contains_key("$flagged"));

        // Unflag
        cache.update_email_flagged("e1", false);
        let cached = cache.get_email("e1").unwrap();
        assert!(!cached.keywords.contains_key("$flagged"));
    }

    #[test]
    fn test_cache_move_and_destroy_updates_indexes_and_counts() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());
        let cache = Cache::open("test_move_destroy").unwrap();

        let mut e1 = make_test_email("e1");
        e1.mailbox_ids.insert("inbox".into(), true);
        let mut e2 = make_test_email("e2");
        e2.mailbox_ids.insert("inbox".into(), true);
        e2.keywords.insert("$seen".into(), true);
        cache.put_emails(&[e1, e2]);
        cache.put_mailbox_index("inbox", &["e1".into(), "e2".into()]);
        cache.put_mailbox_index("archive", &[]);
        cache.put_mailboxes(&[
            Mailbox {
                id: "inbox".into(),
                name: "INBOX".into(),
                parent_id: None,
                role: Some("inbox".into()),
                total_emails: 2,
                unread_emails: 1,
                sort_order: 0,
            },
            Mailbox {
                id: "archive".into(),
                name: "Archive".into(),
                parent_id: None,
                role: Some("archive".into()),
                total_emails: 0,
                unread_emails: 0,
                sort_order: 1,
            },
        ]);

        assert!(cache.apply_move_email("e1", "archive"));
        let mboxes = cache.get_mailboxes().unwrap();
        let inbox = mboxes.iter().find(|m| m.id == "inbox").unwrap();
        let archive = mboxes.iter().find(|m| m.id == "archive").unwrap();
        assert_eq!(inbox.total_emails, 1);
        assert_eq!(inbox.unread_emails, 0);
        assert_eq!(archive.total_emails, 1);
        assert_eq!(archive.unread_emails, 1);

        let inbox_ids: Vec<String> = cache
            .get_mailbox_emails("inbox")
            .unwrap_or_default()
            .iter()
            .map(|e| e.id.clone())
            .collect();
        let archive_ids: Vec<String> = cache
            .get_mailbox_emails("archive")
            .unwrap_or_default()
            .iter()
            .map(|e| e.id.clone())
            .collect();
        assert_eq!(inbox_ids, vec!["e2".to_string()]);
        assert_eq!(archive_ids, vec!["e1".to_string()]);

        assert!(cache.apply_destroy_email("e1"));
        let mboxes = cache.get_mailboxes().unwrap();
        let archive = mboxes.iter().find(|m| m.id == "archive").unwrap();
        assert_eq!(archive.total_emails, 0);
        assert_eq!(archive.unread_emails, 0);
        assert!(cache.get_email("e1").is_none());
    }

    #[test]
    fn test_cache_queue_persistence() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", dir.path());

        let cache = Cache::open("test_queue").unwrap();
        let s1 = cache.enqueue_operation(br#"{"kind":"a"}"#).unwrap();
        let s2 = cache.enqueue_operation(br#"{"kind":"b"}"#).unwrap();
        assert!(s2 > s1);
        let ops = cache.queued_operations();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].0, s1);
        assert_eq!(ops[1].0, s2);
        assert!(cache.remove_queued_operation(s1));
        let ops = cache.queued_operations();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, s2);
    }
}
