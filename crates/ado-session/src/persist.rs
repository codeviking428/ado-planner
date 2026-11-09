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
    keyring_backed: bool,
}

impl EncryptedStore {
    pub fn new(inner: Box<dyn SessionStore>, key: Option<[u8; 32]>) -> Self {
        Self {
            inner,
            key,
            keyring_backed: false,
        }
    }

    /// `key` came from the OS keyring, so [`Self::forget`] deletes it there too.
    /// Stores built with [`Self::new`] never touch the keyring.
    pub fn keyring_backed(inner: Box<dyn SessionStore>, key: Option<[u8; 32]>) -> Self {
        Self {
            inner,
            key,
            keyring_backed: true,
        }
    }

    pub fn persist_to_disk(&self) -> bool {
        self.inner.persist_to_disk() && self.key.is_some()
    }

    pub fn read_plain(&self) -> Option<String> {
        if !self.inner.persist_to_disk() {
            return None;
        }
        let key = self.key?;
        let bytes = self.inner.read()?;
        decrypt(&key, &bytes).ok()
    }

    pub fn write_plain(&self, plain: &str) -> Result<(), Error> {
        if !self.inner.persist_to_disk() {
            return Ok(());
        }
        let Some(key) = self.key else {
            return Ok(());
        };
        let bytes = encrypt(&key, plain)?;
        self.inner.write(&bytes)
    }

    pub fn unlink(&self) {
        self.inner.unlink();
    }

    /// Logout: delete the session file and the keyring key. A keyring-backed
    /// store mints a new key so a later sign-in in this run still persists.
    pub fn forget(&mut self) {
        self.inner.unlink();
        if self.keyring_backed {
            delete_keyring_key();
            self.key = load_or_create_keyring_key();
        }
    }
}

pub fn load_or_create_keyring_key() -> Option<[u8; 32]> {
    let entry = keyring::Entry::new("ado-planner", "session-key").ok()?;
    load_or_create_key(&entry)
}

/// `None` when the keyring cannot be read or written, so the Session stays in memory.
fn load_or_create_key(entry: &keyring::Entry) -> Option<[u8; 32]> {
    match entry.get_secret() {
        Ok(secret) if secret.len() >= 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&secret[..32]);
            return Some(key);
        }
        Ok(_) | Err(keyring::Error::NoEntry) => {}
        Err(_) => return None,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredPat {
    pub pat: String,
    pub organization: String,
}

impl std::fmt::Debug for StoredPat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredPat")
            .field("pat", &"<redacted>")
            .field("organization", &self.organization)
            .finish()
    }
}

pub const ACCESS_TOKEN_REFRESH_SKEW_SECS: i64 = 5 * 60;

#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for StoredOauth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredOauth")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .field("redirect", &self.redirect)
            .field("display_name", &self.display_name)
            .field("username", &self.username)
            .finish()
    }
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

    /// Legacy rows without `expires_at` count as live; the API rejects them if not.
    pub fn is_expired(&self, now_unix: i64) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= now_unix)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use keyring::mock::MockCredential;

    use super::*;

    fn mock_entry() -> keyring::Entry {
        let credential = keyring::mock::default_credential_builder()
            .build(None, "ado-planner", "session-key")
            .unwrap();
        keyring::Entry::new_with_credential(credential)
    }

    fn fail_next(entry: &keyring::Entry, err: keyring::Error) {
        entry
            .get_credential()
            .downcast_ref::<MockCredential>()
            .unwrap()
            .set_error(err);
    }

    fn persists_with(entry: &keyring::Entry) -> bool {
        should_persist_session(load_or_create_key(entry).is_some())
    }

    /// Counts reads and records writes so a test can assert on disk traffic.
    #[derive(Clone, Default)]
    struct SpyStore {
        persist: bool,
        reads: Arc<AtomicUsize>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl SpyStore {
        fn persisting() -> Self {
            Self {
                persist: true,
                ..Self::default()
            }
        }

        fn write_count(&self) -> usize {
            self.writes.lock().unwrap().len()
        }
    }

    impl SessionStore for SpyStore {
        fn persist_to_disk(&self) -> bool {
            self.persist
        }

        fn read(&self) -> Option<Vec<u8>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.writes.lock().unwrap().last().cloned()
        }

        fn write(&self, bytes: &[u8]) -> Result<(), Error> {
            self.writes.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }

        fn unlink(&self) {
            self.writes.lock().unwrap().clear();
        }
    }

    // `msal-cache.test.ts`: Windows and macOS persist when encryption is available.
    #[test]
    fn a_working_keyring_persists_and_reuses_its_key() {
        let entry = mock_entry();
        let first = load_or_create_key(&entry).expect("new key");
        assert_eq!(load_or_create_key(&entry), Some(first));
        assert!(persists_with(&entry));
    }

    // `msal-cache.test.ts`: Windows is memory-only when encryption is unavailable.
    #[test]
    fn an_unreadable_keyring_is_memory_only() {
        let entry = mock_entry();
        fail_next(&entry, keyring::Error::NoStorageAccess("locked".into()));
        assert!(!persists_with(&entry));
    }

    // `msal-cache.test.ts`: Linux gnome_libsecret persists. The Secret Service
    // is the Linux keyring, so a store that can save the key persists.
    #[test]
    fn a_keyring_that_saves_the_key_persists() {
        let entry = mock_entry();
        fail_next(&entry, keyring::Error::NoEntry);
        assert!(persists_with(&entry));
    }

    /// Empty keyring that refuses every save.
    struct ReadOnlyKeyring;

    impl keyring::credential::CredentialApi for ReadOnlyKeyring {
        fn set_secret(&self, _: &[u8]) -> keyring::Result<()> {
            Err(keyring::Error::PlatformFailure("no secret service".into()))
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            Err(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            Err(keyring::Error::NoEntry)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // `msal-cache.test.ts`: Linux basic_text is memory-only. Without a Secret
    // Service the key cannot be saved, and a plaintext key would defeat the cipher.
    #[test]
    fn a_keyring_that_cannot_save_the_key_is_memory_only() {
        let entry = keyring::Entry::new_with_credential(Box::new(ReadOnlyKeyring));
        assert!(!persists_with(&entry));
    }

    // `msal-cache.test.ts`: Linux unknown backend is memory-only.
    #[test]
    fn an_unknown_keyring_failure_is_memory_only() {
        let entry = mock_entry();
        fail_next(
            &entry,
            keyring::Error::PlatformFailure("org.freedesktop.DBus.Error.ServiceUnknown".into()),
        );
        assert!(!persists_with(&entry));
    }

    // `msal-cache.test.ts`: plugin writes only when persistToDisk and cache changed.
    #[test]
    fn a_persisting_store_reads_back_what_it_wrote_and_encrypts_it() {
        let spy = SpyStore::persisting();
        let store = EncryptedStore::new(Box::new(spy.clone()), Some([5u8; 32]));
        assert!(store.read_plain().is_none());
        store.write_plain(r#"{"pat":"pat-secret-value"}"#).unwrap();
        assert_eq!(spy.write_count(), 1);
        let on_disk = spy.writes.lock().unwrap()[0].clone();
        assert!(
            !String::from_utf8_lossy(&on_disk).contains("pat-secret-value"),
            "the session file must not hold the PAT in plain text"
        );
        assert_eq!(
            store.read_plain().as_deref(),
            Some(r#"{"pat":"pat-secret-value"}"#)
        );
        let other_key = EncryptedStore::new(Box::new(spy.clone()), Some([6u8; 32]));
        assert!(other_key.read_plain().is_none());
    }

    // `msal-cache.test.ts`: memory-only plugin never writes.
    #[test]
    fn a_memory_only_store_never_reads_or_writes() {
        for key in [None, Some([5u8; 32])] {
            let spy = SpyStore::default();
            let store = EncryptedStore::new(Box::new(spy.clone()), key);
            assert!(!store.persist_to_disk());
            store.write_plain("secret").unwrap();
            assert!(store.read_plain().is_none());
            assert_eq!(spy.write_count(), 0);
            assert_eq!(spy.reads.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn a_persisting_store_without_a_key_never_writes() {
        let spy = SpyStore::persisting();
        let store = EncryptedStore::new(Box::new(spy.clone()), None);
        store.write_plain("pat").unwrap();
        assert!(!store.persist_to_disk());
        assert_eq!(spy.write_count(), 0);
    }

    #[test]
    fn forget_on_a_test_store_leaves_the_keyring_alone() {
        let spy = SpyStore::persisting();
        let mut store = EncryptedStore::new(Box::new(spy.clone()), Some([5u8; 32]));
        store.write_plain("pat").unwrap();
        store.forget();
        assert_eq!(spy.write_count(), 0);
        assert!(!store.keyring_backed);
        assert!(store.persist_to_disk(), "the in-memory key is kept");
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
