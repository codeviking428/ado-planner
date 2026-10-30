use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use ado_core::Error;

pub trait SessionStore: Send + Sync {
    fn persist_to_disk(&self) -> bool;
    fn read(&self) -> Option<Vec<u8>>;
    fn write(&self, bytes: &[u8]) -> Result<(), Error>;
    fn unlink(&self);
}

pub struct MemoryStore {
    persist: bool,
    value: Mutex<Option<Vec<u8>>>,
}

impl MemoryStore {
    pub fn new(persist: bool) -> Self {
        Self {
            persist,
            value: Mutex::new(None),
        }
    }

    pub fn with_initial(persist: bool, bytes: Option<Vec<u8>>) -> Self {
        Self {
            persist,
            value: Mutex::new(bytes),
        }
    }
}

impl SessionStore for MemoryStore {
    fn persist_to_disk(&self) -> bool {
        self.persist
    }

    fn read(&self) -> Option<Vec<u8>> {
        self.value.lock().ok()?.clone()
    }

    fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        *self
            .value
            .lock()
            .map_err(|err| Error::message(err.to_string()))? = Some(bytes.to_vec());
        Ok(())
    }

    fn unlink(&self) {
        if let Ok(mut value) = self.value.lock() {
            *value = None;
        }
    }
}

pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SessionStore for FileStore {
    fn persist_to_disk(&self) -> bool {
        true
    }

    fn read(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.path).ok()
    }

    fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| Error::message(err.to_string()))?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|err| Error::message(err.to_string()))?;
        std::fs::rename(&tmp, &self.path).map_err(|err| Error::message(err.to_string()))?;
        Ok(())
    }

    fn unlink(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn should_persist_session(keyring_available: bool) -> bool {
    keyring_available
}

pub struct EncryptedStore {
    inner: Box<dyn SessionStore>,
    key: Option<[u8; 32]>,
}

impl EncryptedStore {
    pub fn new(inner: Box<dyn SessionStore>, key: Option<[u8; 32]>) -> Self {
        Self { inner, key }
    }

    pub fn persist_to_disk(&self) -> bool {
        self.inner.persist_to_disk() && self.key.is_some()
    }

    pub fn read_plain(&self) -> Option<String> {
        let key = self.key?;
        let bytes = self.inner.read()?;
        decrypt(&key, &bytes).ok()
    }

    pub fn write_plain(&self, plain: &str) -> Result<(), Error> {
        let Some(key) = self.key else {
            return Ok(());
        };
        let bytes = encrypt(&key, plain)?;
        self.inner.write(&bytes)
    }

    pub fn unlink(&self) {
        self.inner.unlink();
    }
}

pub fn load_or_create_keyring_key() -> Option<[u8; 32]> {
    let entry = keyring::Entry::new("ado-planner", "session-key").ok()?;
    if let Ok(secret) = entry.get_secret()
        && secret.len() >= 32
    {
        let mut key = [0u8; 32];
        key.copy_from_slice(&secret[..32]);
        return Some(key);
    }
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    entry.set_secret(&key).ok()?;
    Some(key)
}

pub fn delete_keyring_key() {
    if let Ok(entry) = keyring::Entry::new("ado-planner", "session-key") {
        let _ = entry.delete_credential();
    }
}

pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("session")
}

fn encrypt(key: &[u8; 32], plain: &str) -> Result<Vec<u8>, Error> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, plain.as_bytes())
        .map_err(|err| Error::message(err.to_string()))?;
    let mut out = nonce_bytes.to_vec();
    out.extend(ciphertext);
    Ok(out)
}

fn decrypt(key: &[u8; 32], bytes: &[u8]) -> Result<String, Error> {
    if bytes.len() < 12 {
        return Err(Error::message("session file truncated"));
    }
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::from(*<&[u8; 12]>::try_from(&bytes[..12]).unwrap());
    let plain = cipher
        .decrypt(&nonce, &bytes[12..])
        .map_err(|err| Error::message(err.to_string()))?;
    String::from_utf8(plain).map_err(|err| Error::message(err.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPat {
    pub pat: String,
    pub organization: String,
}

pub const ACCESS_TOKEN_REFRESH_SKEW_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOauth {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub redirect: Option<String>,
    pub display_name: Option<String>,
    pub username: Option<String>,
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

impl StoredOauth {
    pub fn needs_refresh(&self, now_unix: i64) -> bool {
        if self.refresh_token.is_none() {
            return false;
        }
        match self.expires_at {
            Some(expires_at) => {
                expires_at <= now_unix.saturating_add(ACCESS_TOKEN_REFRESH_SKEW_SECS)
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_macos_persist_when_keyring_available() {
        assert!(should_persist_session(true));
        assert!(!should_persist_session(false));
    }

    #[test]
    fn memory_store_round_trips_when_persist_enabled() {
        let store = MemoryStore::new(true);
        store.write(b"secret").unwrap();
        assert_eq!(store.read().as_deref(), Some(b"secret".as_slice()));
        store.unlink();
        assert!(store.read().is_none());
    }

    #[test]
    fn encrypted_store_without_key_never_writes() {
        let inner = MemoryStore::new(true);
        let store = EncryptedStore::new(Box::new(inner), None);
        store.write_plain("pat").unwrap();
        assert!(!store.persist_to_disk());
    }

    #[test]
    fn legacy_oauth_json_without_expiry_deserializes() {
        let stored: StoredOauth = serde_json::from_str(
            r#"{"access_token":"a","refresh_token":"r","display_name":null,"username":null}"#,
        )
        .unwrap();
        assert_eq!(stored.access_token, "a");
        assert!(stored.expires_at.is_none());
        assert!(stored.needs_refresh(unix_now()));
    }

    #[test]
    fn fresh_access_token_does_not_need_refresh() {
        let stored = StoredOauth {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at: Some(unix_now() + 3600),
            redirect: Some("http://127.0.0.1:9/".into()),
            display_name: None,
            username: None,
        };
        assert!(!stored.needs_refresh(unix_now()));
        assert!(stored.needs_refresh(unix_now() + 3600));
    }
}
