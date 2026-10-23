//! Identity-scoped, process-safe storage for rotating login sessions.

use std::fmt;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hubuum_client::{ApiError, BaseUrl, SessionStore, SessionStoreGuard, StoredSession};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

mod keychain;
mod storage;

pub use storage::SessionStorage;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session username must not be blank")]
    InvalidUsername,
    #[error("Session identity provider must not be blank")]
    InvalidIdentityScope,
    #[error("Session storage must be 'file', 'keychain', or 'none'")]
    InvalidStorage,
    #[error("Session storage error: {0}")]
    Io(#[from] Error),
    #[error("Session storage error: {0}")]
    Store(#[from] ApiError),
}

/// Storage settings supplied by the application; no global configuration is read.
pub struct SessionOptions {
    directory: PathBuf,
    identity: SessionIdentity,
    storage: SessionStorage,
}

impl SessionOptions {
    pub fn new(directory: impl Into<PathBuf>, identity: SessionIdentity) -> Self {
        Self {
            directory: directory.into(),
            identity,
            storage: SessionStorage::default(),
        }
    }

    pub fn storage(mut self, storage: SessionStorage) -> Self {
        self.storage = storage;
        self
    }
}

pub struct SessionCache {
    store: Arc<dyn SessionStore>,
    login_lock: PathBuf,
}

impl SessionCache {
    pub fn open(options: SessionOptions) -> Result<Option<Self>, SessionError> {
        if options.storage == SessionStorage::None {
            return Ok(None);
        }
        let files = SessionFileStore::open(options.directory, options.identity)?;
        let login_lock = files.directory.join("sessions.login.lock");
        let store: Arc<dyn SessionStore> = match options.storage {
            SessionStorage::File => Arc::new(files),
            SessionStorage::Keychain => Arc::new(keychain::KeychainStore::new(files)?),
            SessionStorage::None => unreachable!(),
        };
        Ok(Some(Self { store, login_lock }))
    }

    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }

    /// Serialize the cache recheck and login/save across processes. Acquire this
    /// only after prompting, and before the SDK acquires its session-store lock.
    /// Refreshes keep using the SDK lock and never acquire this login lock.
    pub fn lock_login(&self) -> Result<LoginGuard, SessionError> {
        Ok(LoginGuard {
            _lock: lock_file(&self.login_lock)?,
        })
    }
}

/// Holds the login transaction lock until dropped. Acquire it only after prompting.
pub struct LoginGuard {
    _lock: File,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "IdentityWire")]
pub struct SessionIdentity {
    base_url: String,
    identity_scope: Option<String>,
    username: String,
}

#[derive(Deserialize)]
struct IdentityWire {
    base_url: String,
    identity_scope: Option<String>,
    username: String,
}

impl TryFrom<IdentityWire> for SessionIdentity {
    type Error = SessionError;
    fn try_from(value: IdentityWire) -> Result<Self, Self::Error> {
        Self::new(
            &BaseUrl::new(value.base_url)?,
            value.identity_scope.as_deref(),
            &value.username,
        )
    }
}

impl SessionIdentity {
    pub fn new(
        base_url: &BaseUrl,
        identity_scope: Option<&str>,
        username: &str,
    ) -> Result<Self, SessionError> {
        if username.trim().is_empty() {
            return Err(SessionError::InvalidUsername);
        }
        if identity_scope.is_some_and(|scope| scope.trim().is_empty()) {
            return Err(SessionError::InvalidIdentityScope);
        }
        Ok(Self {
            base_url: base_url.as_str().to_owned(),
            identity_scope: identity_scope.map(str::to_owned),
            username: username.to_owned(),
        })
    }
}

impl fmt::Debug for SessionIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionIdentity").finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    identity: SessionIdentity,
    session: StoredSession,
}

struct SessionFileStore {
    directory: PathBuf,
    identity: SessionIdentity,
}

impl std::fmt::Debug for SessionFileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionFileStore").finish_non_exhaustive()
    }
}

impl SessionFileStore {
    #[cfg(test)]
    fn at(
        directory: PathBuf,
        base_url: &BaseUrl,
        identity_scope: Option<&str>,
        username: &str,
    ) -> Result<Self, SessionError> {
        Self::open(
            directory,
            SessionIdentity::new(base_url, identity_scope, username)?,
        )
    }

    fn open(directory: PathBuf, identity: SessionIdentity) -> Result<Self, SessionError> {
        create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::fs::{set_permissions, Permissions};
            use std::os::unix::fs::PermissionsExt;
            set_permissions(&directory, Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            directory,
            identity,
        })
    }
    fn lock_guard(&self) -> Result<FileGuard, ApiError> {
        // Lock a stable sidecar, never the atomically replaced data inode.
        Ok(FileGuard {
            _lock: lock_file(&self.directory.join("sessions.lock"))?,
            path: self.directory.join("sessions.json"),
            identity: self.identity.clone(),
        })
    }
}

fn lock_file(path: &Path) -> Result<File, ApiError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(path)?;
    secure_file(&lock)?;
    lock.lock()?;
    Ok(lock)
}

impl SessionStore for SessionFileStore {
    fn lock(&self) -> Result<Box<dyn SessionStoreGuard>, ApiError> {
        Ok(Box::new(self.lock_guard()?))
    }
}

struct FileGuard {
    _lock: File,
    path: PathBuf,
    identity: SessionIdentity,
}

impl FileGuard {
    fn entries(&self) -> Result<Vec<Entry>, ApiError> {
        match File::open(&self.path) {
            Ok(mut file) => {
                secure_file(&file)?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                serde_json::from_slice(&bytes).map_err(|_| {
                    Error::new(
                        ErrorKind::InvalidData,
                        "Invalid session cache; remove sessions.json and sign in again",
                    )
                    .into()
                })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }
}

fn secure_file(file: &File) -> Result<(), Error> {
    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        file.set_permissions(Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

impl SessionStoreGuard for FileGuard {
    fn load(&self) -> Result<Option<StoredSession>, ApiError> {
        Ok(self
            .entries()?
            .into_iter()
            .find(|entry| entry.identity == self.identity)
            .map(|entry| entry.session))
    }

    fn save(&mut self, session: Option<&StoredSession>) -> Result<(), ApiError> {
        self.write_session(session)
    }
}

impl FileGuard {
    fn write_session(&self, session: Option<&StoredSession>) -> Result<(), ApiError> {
        let mut entries = self.entries()?;
        entries.retain(|entry| entry.identity != self.identity);
        if let Some(session) = session {
            entries.push(Entry {
                identity: self.identity.clone(),
                session: session.clone(),
            });
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Session cache has no directory"))?;
        let mut temporary = NamedTempFile::new_in(parent)?;
        serde_json::to_writer(&mut temporary, &entries)
            .map_err(|_| Error::other("Could not serialize session cache"))?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        sync_directory(parent)?;
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), Error> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    pub(super) fn session() -> StoredSession {
        let now = chrono::Utc::now();
        serde_json::from_value(json!({
            "base_url":"https://example.invalid/", "client_id":"hubuum-cli",
            "refresh_at": now + chrono::Duration::minutes(14),
            "pair": {
                "session_id":"fixture-session", "token_type":"Bearer",
                "access_token":"access-secret", "refresh_token":"refresh-secret",
                "access_expires_at":now + chrono::Duration::minutes(15),
                "refresh_expires_at":now + chrono::Duration::days(7),
                "absolute_expires_at":now + chrono::Duration::days(30)
            }
        }))
        .unwrap()
    }

    #[test]
    fn disabled_storage_does_not_access_the_directory() {
        let directory = tempdir().unwrap();
        let absent = directory.path().join("never-created");
        let identity = SessionIdentity::new(
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            "alice",
        )
        .unwrap();
        assert!(SessionCache::open(
            SessionOptions::new(&absent, identity).storage(SessionStorage::None)
        )
        .unwrap()
        .is_none());
        assert!(!absent.exists());
    }

    #[test]
    fn identity_deserialization_enforces_constructor_invariants() {
        for (base, scope, user) in [
            ("not a url", None, "alice"),
            ("https://example.invalid", Some(" "), "alice"),
            ("https://example.invalid", None, " "),
        ] {
            let value = json!({"base_url":base,"identity_scope":scope,"username":user});
            assert!(serde_json::from_value::<SessionIdentity>(value).is_err());
        }
        let identity = SessionIdentity::new(
            &BaseUrl::new("https://example.invalid").unwrap(),
            Some("corp"),
            "alice",
        )
        .unwrap();
        let restored: SessionIdentity =
            serde_json::from_value(serde_json::to_value(&identity).unwrap()).unwrap();
        assert_eq!(identity, restored);
    }

    #[test]
    fn identity_scoping_and_atomic_replacement_preserve_other_accounts() {
        let directory = tempdir().unwrap();
        let identities = [
            ("https://example.invalid", None, "alice"),
            ("http://example.invalid", None, "alice"),
            ("https://example.invalid:8443", None, "alice"),
            ("https://example.invalid/prefix", None, "alice"),
            ("https://example.invalid", Some("corp"), "alice"),
            ("https://example.invalid", None, "bob"),
        ];
        for (base, scope, user) in identities {
            let store = SessionFileStore::at(
                directory.path().join("data"),
                &BaseUrl::new(base).unwrap(),
                scope,
                user,
            )
            .unwrap();
            let mut guard = store.lock().unwrap();
            assert!(guard.load().unwrap().is_none());
            guard.save(Some(&session())).unwrap();
        }
        let store = SessionFileStore::at(
            directory.path().join("data"),
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            "alice",
        )
        .unwrap();
        store.lock().unwrap().save(None).unwrap();
        for (index, (base, scope, user)) in identities.into_iter().enumerate() {
            let store = SessionFileStore::at(
                directory.path().join("data"),
                &BaseUrl::new(base).unwrap(),
                scope,
                user,
            )
            .unwrap();
            assert_eq!(store.lock().unwrap().load().unwrap().is_some(), index != 0);
        }
        assert!(!format!("{store:?}").contains("alice"));
    }

    #[test]
    fn corrupt_cache_is_preserved_and_errors_do_not_echo_secrets() {
        let directory = tempdir().unwrap();
        let store = SessionFileStore::at(
            directory.path().to_owned(),
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            "alice",
        )
        .unwrap();
        let path = directory.path().join("sessions.json");
        std::fs::write(&path, "secret-invalid-json").unwrap();
        let mut guard = store.lock().unwrap();
        let error = guard.load().unwrap_err();
        assert!(!format!("{error:?} {error}").contains("secret-invalid-json"));
        assert!(guard.save(Some(&session())).is_err());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "secret-invalid-json"
        );
    }

    #[cfg(unix)]
    #[test]
    fn saved_credentials_and_lock_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempdir().unwrap();
        let root = directory.path().join("data");
        let store = SessionFileStore::at(
            root.clone(),
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            "alice",
        )
        .unwrap();
        store.lock().unwrap().save(Some(&session())).unwrap();
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in ["sessions.json", "sessions.lock"] {
            assert_eq!(
                std::fs::metadata(root.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn reading_existing_credentials_repairs_permissions_without_rewriting() {
        use std::fs::{read, set_permissions, Permissions};
        use std::os::unix::fs::PermissionsExt;

        let directory = tempdir().unwrap();
        let store = SessionFileStore::at(
            directory.path().to_owned(),
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            "alice",
        )
        .unwrap();
        store.lock().unwrap().save(Some(&session())).unwrap();
        let path = directory.path().join("sessions.json");
        let original = read(&path).unwrap();
        for name in ["sessions.json", "sessions.lock"] {
            set_permissions(directory.path().join(name), Permissions::from_mode(0o666)).unwrap();
        }

        assert!(store.lock().unwrap().load().unwrap().is_some());
        assert_eq!(read(path).unwrap(), original);
        for name in ["sessions.json", "sessions.lock"] {
            assert_eq!(
                std::fs::metadata(directory.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
