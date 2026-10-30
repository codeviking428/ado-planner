use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STALE_SECS: u64 = 5 * 60;
pub const KEEP_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheFreshness {
    Fresh,
    Stale,
}

#[derive(Debug, Clone)]
pub struct CacheHit {
    pub value: Value,
    pub freshness: CacheFreshness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCache {
    saved_unix: u64,
    entries: HashMap<String, DiskEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskEntry {
    saved_unix: u64,
    json: String,
}

#[derive(Debug, Clone)]
pub struct QueryCache {
    persist_path: PathBuf,
    entries: HashMap<String, DiskEntry>,
}

impl QueryCache {
    pub fn load(path: PathBuf) -> Self {
        let mut cache = Self {
            persist_path: path,
            entries: HashMap::new(),
        };
        cache.read_disk();
        cache
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn read_disk(&mut self) {
        let Ok(bytes) = std::fs::read(&self.persist_path) else {
            return;
        };
        let Ok(file): Result<DiskCache, _> = postcard::from_bytes(&bytes) else {
            return;
        };
        let now = Self::now();
        if now.saturating_sub(file.saved_unix) > KEEP_SECS {
            return;
        }
        self.entries = file
            .entries
            .into_iter()
            .filter(|(key, entry)| {
                !key.starts_with("session:") && now.saturating_sub(entry.saved_unix) <= KEEP_SECS
            })
            .collect();
    }

    pub fn get(&self, key: &str) -> Option<CacheHit> {
        if key.starts_with("session:") {
            return None;
        }
        let entry = self.entries.get(key)?;
        let age = Self::now().saturating_sub(entry.saved_unix);
        if age > KEEP_SECS {
            return None;
        }
        let value = serde_json::from_str(&entry.json).ok()?;
        Some(CacheHit {
            value,
            freshness: if age <= STALE_SECS {
                CacheFreshness::Fresh
            } else {
                CacheFreshness::Stale
            },
        })
    }

    pub fn set(&mut self, key: &str, value: &Value) {
        if key.starts_with("session:") {
            return;
        }
        self.entries.insert(
            key.to_string(),
            DiskEntry {
                saved_unix: Self::now(),
                json: value.to_string(),
            },
        );
        self.persist();
    }

    pub fn persist(&self) {
        let file = DiskCache {
            saved_unix: Self::now(),
            entries: self.entries.clone(),
        };
        if let Ok(bytes) = postcard::to_stdvec(&file) {
            let _ = std::fs::write(&self.persist_path, bytes);
        }
    }

    pub fn path(&self) -> &Path {
        &self.persist_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn never_stores_session_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = QueryCache::load(dir.path().join("cache.bin"));
        cache.set("session:info", &json!({"pat": "secret"}));
        assert!(cache.get("session:info").is_none());
        cache.set("orgs", &json!(["contoso"]));
        assert_eq!(cache.get("orgs").unwrap().value, json!(["contoso"]));
    }

    #[test]
    fn round_trips_postcard_without_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.bin");
        let mut cache = QueryCache::load(path.clone());
        cache.set("projects:contoso", &json!([{"name": "Shop"}]));
        let reloaded = QueryCache::load(path);
        assert_eq!(
            reloaded.get("projects:contoso").unwrap().value,
            json!([{"name": "Shop"}])
        );
    }
}
