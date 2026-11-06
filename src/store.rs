//! On-disk persistence. Plain JSON under the platform config dir; secrets go to the OS keychain
//! (macOS Keychain, Windows Credential Manager, Linux Secret Service), falling back to a 0600
//! `secrets.json` when the keychain refuses the write or no keychain service is reachable.
//!
//! All secrets live in one keychain item (the vault), read the first time a secret is needed,
//! never at launch. macOS asks for permission per item and again for every new build, so one
//! item read on demand means at most one prompt per build instead of three per connection.
//! Items from older versions (one per secret) are read once, when that secret is first needed,
//! and copied into the vault.

use crate::model::*;
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

const KEYRING_SERVICE: &str = "dev.strata.app";
/// The one keychain item that holds every secret, as a JSON map.
const VAULT_KEY: &str = "vault";

/// Where secrets are kept.
enum Keychain {
    /// The OS keychain, under this service name.
    System(String),
    /// No keychain (tests): every write falls back to `secrets.json`.
    #[cfg_attr(not(test), allow(dead_code))]
    Off,
    /// An in-memory keychain for tests, with a count of reads that found an item.
    #[cfg(test)]
    Memory(Mutex<HashMap<String, String>>, std::sync::atomic::AtomicUsize),
}

impl Keychain {
    /// Reads one item. `Ok(None)` when it does not exist.
    fn get(&self, key: &str) -> std::result::Result<Option<String>, String> {
        match self {
            Keychain::System(service) => match keyring::Entry::new(service, key).and_then(|e| e.get_password()) {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            Keychain::Off => Ok(None),
            #[cfg(test)]
            Keychain::Memory(m, reads) => {
                // Only reads that find an item count: macOS prompts for those, not for misses.
                let v = m.lock().unwrap().get(key).cloned();
                if v.is_some() {
                    reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(v)
            }
        }
    }

    /// Stores `value` under `key`, or removes the item when `value` is empty.
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), String> {
        match self {
            Keychain::System(service) => keychain_write(service, key, value),
            Keychain::Off => Err("keychain disabled".into()),
            #[cfg(test)]
            Keychain::Memory(m, _) => {
                let mut m = m.lock().unwrap();
                if value.is_empty() { m.remove(key); } else { m.insert(key.into(), value.into()); }
                Ok(())
            }
        }
    }
}

/// The keychain service for a profile. A `STRATA_HOME` profile (development, screenshots)
/// gets its own service, so it never reads the real profile's item and never prompts for it.
fn service_for(custom_home: Option<&Path>) -> String {
    match custom_home {
        None => KEYRING_SERVICE.to_string(),
        Some(home) => {
            use sha2::{Digest, Sha256};
            let hash = hex::encode(Sha256::digest(home.to_string_lossy().as_bytes()));
            format!("{KEYRING_SERVICE}.profile-{}", &hash[..12])
        }
    }
}

/// The vault as last read from the keychain.
enum Vault {
    NotRead,
    Read(HashMap<String, String>),
    /// The keychain refused the read. Writing now would replace secrets we could not see,
    /// so changes go to `secrets.json` and reads use the old per-secret items.
    Refused,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionFile {
    pub connections: Vec<ConnectionConfig>,
    pub groups: Vec<Group>,
}

/// Exported connection bundle (`.strataconnection`), passwords never included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionExport {
    pub format: String,
    pub connections: Vec<ConnectionConfig>,
}

pub struct Store {
    root: PathBuf,
    keychain: Keychain,
    vault: Mutex<Vault>,
    /// Last value read or written per secret key; unchanged secrets are not rewritten, which
    /// keeps routine saves (last used, safe mode) from hitting the keychain at all.
    known: Mutex<HashMap<String, String>>,
    keychain_error: Mutex<Option<String>>,
}

impl Store {
    #[cfg(test)]
    pub fn for_test(root: PathBuf) -> Self { Self::with_keychain(root, Keychain::Off) }
    fn with_keychain(root: PathBuf, keychain: Keychain) -> Self {
        Self { root, keychain, vault: Mutex::new(Vault::NotRead), known: Mutex::default(), keychain_error: Mutex::default() }
    }
    pub fn open() -> Result<Self> {
        let custom = std::env::var_os("STRATA_HOME").map(PathBuf::from);
        let root = custom
            .clone()
            .or_else(|| dirs::config_dir().map(|d| d.join("Strata")))
            .context("no config directory on this platform")?;
        std::fs::create_dir_all(root.join("history"))?;
        std::fs::create_dir_all(root.join("themes"))?;
        Ok(Self::with_keychain(root.clone(), Keychain::System(service_for(custom.as_deref()))))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn read<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        let path = self.root.join(name);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                // Keep the unreadable file for the user instead of silently overwriting it.
                let _ = std::fs::copy(&path, path.with_extension("json.bk"));
                eprintln!("strata: {name} is unreadable ({e}); a backup was written next to it");
                T::default()
            }),
            Err(_) => T::default(),
        }
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let path = self.root.join(name);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Connections without their secrets: reading them here would ask for keychain access at
    /// launch. Call [`Store::fill_secrets`] before connecting or editing.
    pub fn load_connections(&self) -> ConnectionFile {
        self.read("connections.json")
    }

    /// Loads a connection's passwords into the empty fields of `c`.
    pub fn fill_secrets(&self, c: &mut ConnectionConfig) {
        for (kind, field) in [("db", &mut c.password), ("ssh", &mut c.ssh.password), ("sshkey", &mut c.ssh.key_passphrase)] {
            if field.is_empty() {
                *field = self.secret(c.id, kind);
            }
        }
    }

    pub fn save_connections(&self, file: &ConnectionFile) -> Result<()> {
        for c in &file.connections {
            for (kind, value) in [("db", &c.password), ("ssh", &c.ssh.password), ("sshkey", &c.ssh.key_passphrase)] {
                // An empty secret that was never read is one we have not loaded, not one the
                // user cleared: leave the stored value alone.
                let key = format!("{}:{kind}", c.id);
                if value.is_empty() && !self.known.lock().map(|k| k.contains_key(&key)).unwrap_or(false) {
                    continue;
                }
                self.set_secret_raw(&key, value);
            }
        }
        self.write("connections.json", file)
    }

    pub fn forget_secrets(&self, id: Uuid) {
        for kind in ["db", "ssh", "sshkey"] {
            let key = format!("{id}:{kind}");
            self.set_secret_raw(&key, "");
            // Remove the item an older version kept for it, if any.
            if let Keychain::System(service) = &self.keychain {
                if let Ok(e) = keyring::Entry::new(service, &key) {
                    let _ = e.delete_credential();
                }
            }
        }
    }

    pub fn load_settings(&self) -> Settings {
        self.read::<Option<Settings>>("settings.json").unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.write("settings.json", s)
    }

    pub fn load_queries(&self) -> Vec<SavedQuery> {
        self.read("queries.json")
    }

    pub fn save_queries(&self, q: &[SavedQuery]) -> Result<()> {
        self.write("queries.json", &q)
    }

    pub fn load_boards(&self) -> Vec<MetricsBoard> {
        self.read("boards.json")
    }

    pub fn save_boards(&self, b: &[MetricsBoard]) -> Result<()> {
        self.write("boards.json", &b)
    }

    pub fn load_secret_value(&self, key: &str) -> String {
        self.secret_raw(key)
    }

    pub fn save_secret_value(&self, key: &str, value: &str) {
        self.set_secret_raw(key, value)
    }

    /// Why the last secret write fell back to `secrets.json`, reported once.
    pub fn take_keychain_error(&self) -> Option<String> {
        self.keychain_error.lock().ok()?.take()
    }

    pub fn append_history(&self, conn: Uuid, entry: &HistoryEntry) {
        use std::io::Write;
        let path = self.root.join("history").join(format!("{conn}.jsonl"));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn load_history(&self, conn: Uuid, keep_days: u32) -> Vec<HistoryEntry> {
        let path = self.root.join("history").join(format!("{conn}.jsonl"));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        let cutoff = chrono::Utc::now() - chrono::Duration::days(keep_days.max(1) as i64);
        let all: Vec<HistoryEntry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let kept: Vec<HistoryEntry> = all.iter().filter(|e| e.at >= cutoff).cloned().collect();
        if kept.len() != all.len() {
            let body: String = kept.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
            let _ = std::fs::write(&path, body);
        }
        kept
    }

    pub fn clear_history(&self, conn: Uuid) {
        let _ = std::fs::remove_file(self.root.join("history").join(format!("{conn}.jsonl")));
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.root.join("themes")
    }

    fn secret(&self, id: Uuid, kind: &str) -> String {
        self.secret_raw(&format!("{id}:{kind}"))
    }

    /// Runs `f` on the vault, reading it from the keychain the first time.
    fn with_vault<R>(&self, f: impl FnOnce(&mut Vault) -> R) -> R {
        let mut v = self.vault.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*v, Vault::NotRead) {
            *v = match self.keychain.get(VAULT_KEY) {
                Ok(Some(json)) => Vault::Read(serde_json::from_str(&json).unwrap_or_default()),
                Ok(None) => Vault::Read(HashMap::new()),
                Err(_) => Vault::Refused,
            };
        }
        f(&mut v)
    }

    fn write_vault(&self, map: &HashMap<String, String>) -> std::result::Result<(), String> {
        self.keychain.set(VAULT_KEY, &serde_json::to_string(map).unwrap_or_default())
    }

    fn secret_raw(&self, key: &str) -> String {
        if let Some(v) = self.known.lock().ok().and_then(|k| k.get(key).cloned()) {
            return v;
        }
        // A file copy only exists because the latest keychain write failed, so it is newer than
        // whatever the keychain still holds.
        // It is left out of `known`, so the next save retries the keychain and moves it there.
        if let Some(v) = self.fallback_secrets().remove(key) {
            return v;
        }
        let value = self.with_vault(|vault| match vault {
            Vault::Read(map) => match map.get(key) {
                Some(v) => v.clone(),
                None => {
                    // Not in the vault yet: read the older per-secret item once and copy it in.
                    // An empty entry records that there was nothing to copy.
                    let v = self.keychain.get(key).ok().flatten().unwrap_or_default();
                    map.insert(key.to_string(), v.clone());
                    let _ = self.write_vault(map);
                    v
                }
            },
            _ => self.keychain.get(key).ok().flatten().unwrap_or_default(),
        });
        if let Ok(mut k) = self.known.lock() {
            k.insert(key.to_string(), value.clone());
        }
        value
    }

    fn set_secret_raw(&self, key: &str, value: &str) {
        if self.known.lock().ok().and_then(|k| k.get(key).cloned()).as_deref() == Some(value) {
            return;
        }
        let keychain_result = self.with_vault(|vault| match vault {
            Vault::Read(map) => {
                let before = map.insert(key.to_string(), value.to_string());
                let r = self.write_vault(map);
                if r.is_err() {
                    match before { Some(b) => { map.insert(key.to_string(), b); } None => { map.remove(key); } }
                }
                r
            }
            _ => Err("the keychain refused access to Strata's passwords".to_string()),
        });
        let mut fallback = self.fallback_secrets();
        let changed = match &keychain_result {
            Ok(()) => fallback.remove(key).is_some(),
            Err(_) if value.is_empty() => fallback.remove(key).is_some(),
            Err(e) => {
                if !matches!(self.keychain, Keychain::Off) {
                    if let Ok(mut slot) = self.keychain_error.lock() {
                        *slot = Some(e.clone());
                    }
                }
                fallback.insert(key.to_string(), value.to_string()).as_deref() != Some(value)
            }
        };
        if changed {
            self.write_fallback_secrets(&fallback);
        }
        if let Ok(mut k) = self.known.lock() {
            k.insert(key.to_string(), value.to_string());
        }
    }

    fn fallback_secrets(&self) -> HashMap<String, String> {
        std::fs::read(self.root.join("secrets.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_fallback_secrets(&self, map: &HashMap<String, String>) {
        let path = self.root.join("secrets.json");
        if map.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        if std::fs::write(&path, serde_json::to_vec(map).unwrap_or_default()).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

/// Stores `value` under `key`, or removes the item when `value` is empty.
fn keychain_write(service: &str, key: &str, value: &str) -> std::result::Result<(), String> {
    let entry = keyring::Entry::new(service, key).map_err(|e| e.to_string())?;
    if value.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    entry.set_password(value).or_else(|first| {
        // macOS ties an item to the build that created it; a rebuilt or updated app may be
        // refused an in-place update but allowed to replace the item.
        let _ = entry.delete_credential();
        entry.set_password(value).map_err(|_| first.to_string())
    })
}

pub fn hash_passcode(code: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"strata-passcode:");
    h.update(code.as_bytes());
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let root = std::env::temp_dir().join(format!("strata-store-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Store::for_test(root)
    }

    fn memory_store(root: &Path, items: &[(&str, &str)]) -> Store {
        let map = items.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Store::with_keychain(root.to_path_buf(), Keychain::Memory(Mutex::new(map), Default::default()))
    }

    fn reads(store: &Store) -> usize {
        match &store.keychain { Keychain::Memory(_, r) => r.load(std::sync::atomic::Ordering::SeqCst), _ => 0 }
    }

    #[test]
    fn passwords_survive_a_restart_without_a_keychain() {
        let store = temp_store("restart");
        let mut c = ConnectionConfig::new(DriverKind::Postgres);
        c.password = "hunter2".into();
        store.save_connections(&ConnectionFile { connections: vec![c.clone()], groups: vec![] }).unwrap();

        let reopened = Store::for_test(store.root().to_path_buf());
        let mut loaded = reopened.load_connections().connections.remove(0);
        reopened.fill_secrets(&mut loaded);
        assert_eq!(loaded.password, "hunter2");
        assert!(!std::fs::read_to_string(store.root().join("connections.json")).unwrap().contains("hunter2"));
    }

    #[test]
    fn a_changed_password_replaces_the_old_one_and_clearing_removes_it() {
        let store = temp_store("change");
        store.save_secret_value("k", "old");
        store.save_secret_value("k", "new");
        assert_eq!(Store::for_test(store.root().to_path_buf()).load_secret_value("k"), "new");
        store.save_secret_value("k", "");
        assert_eq!(Store::for_test(store.root().to_path_buf()).load_secret_value("k"), "");
        assert!(!store.root().join("secrets.json").exists());
    }

    #[test]
    fn launch_reads_no_keychain_items_and_connecting_reads_only_the_vault() {
        let root = temp_store("vault").root().to_path_buf();
        let first = memory_store(&root, &[]);
        let conns: Vec<ConnectionConfig> = (0..5).map(|i| { let mut c = ConnectionConfig::new(DriverKind::Postgres); c.password = format!("pw{i}"); c }).collect();
        first.save_connections(&ConnectionFile { connections: conns.clone(), groups: vec![] }).unwrap();
        let items = match &first.keychain { Keychain::Memory(m, _) => m.lock().unwrap().clone(), _ => unreachable!() };
        assert_eq!(items.len(), 1, "every secret goes in one item: {:?}", items.keys());

        // A new build starts: listing connections touches nothing.
        let items: Vec<(String, String)> = items.into_iter().collect();
        let refs: Vec<(&str, &str)> = items.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let next = memory_store(&root, &refs);
        let mut listed = next.load_connections();
        assert_eq!(reads(&next), 0);
        // Connecting to two of them reads the vault once.
        for c in listed.connections.iter_mut().take(2) {
            next.fill_secrets(c);
        }
        assert_eq!(listed.connections[1].password, "pw1");
        assert_eq!(reads(&next), 1);
        // Saving keeps the passwords of connections that were never opened.
        next.save_connections(&listed).unwrap();
        let mut last = listed.connections[4].clone();
        assert!(last.password.is_empty());
        next.fill_secrets(&mut last);
        assert_eq!(last.password, "pw4");
    }

    #[test]
    fn a_custom_home_uses_its_own_keychain_service() {
        assert_eq!(service_for(None), KEYRING_SERVICE);
        let a = service_for(Some(Path::new("/tmp/a")));
        assert_ne!(a, KEYRING_SERVICE);
        assert_ne!(a, service_for(Some(Path::new("/tmp/b"))));
        assert_eq!(a, service_for(Some(Path::new("/tmp/a"))));
    }

    #[test]
    fn a_password_from_an_older_version_moves_into_the_vault_once() {
        let root = temp_store("legacy").root().to_path_buf();
        let mut c = ConnectionConfig::new(DriverKind::Postgres);
        let legacy = format!("{}:db", c.id);
        let store = memory_store(&root, &[(legacy.as_str(), "old-secret")]);
        store.fill_secrets(&mut c);
        assert_eq!(c.password, "old-secret");
        let vault = match &store.keychain { Keychain::Memory(m, _) => m.lock().unwrap().get(VAULT_KEY).cloned().unwrap(), _ => unreachable!() };
        assert!(vault.contains("old-secret"));
    }
}
