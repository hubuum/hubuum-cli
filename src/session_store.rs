//! Identity-scoped, process-safe storage for rotating login sessions.

use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hubuum_client::{ApiError, BaseUrl, SessionStore, SessionStoreGuard, StoredSession};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::errors::AppError;
use crate::files::{data_root_dir, ensure_root_dir_at};
use crate::models::SessionStorage;

mod keychain;

pub(crate) fn session_store(
    storage: SessionStorage,
    base_url: &BaseUrl,
    identity_scope: Option<&str>,
    username: &str,
) -> Result<Option<Arc<dyn SessionStore>>, AppError> {
    let files = || SessionFileStore::new(base_url, identity_scope, username);
    Ok(match storage {
        SessionStorage::File => Some(Arc::new(files()?)),
        SessionStorage::Keychain => Some(Arc::new(keychain::KeychainStore::new(files()?)?)),
        SessionStorage::None => None,
    })
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Identity {
    base_url: String,
    identity_scope: Option<String>,
    username: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    identity: Identity,
    session: StoredSession,
}

pub(crate) struct SessionFileStore {
    directory: PathBuf,
    identity: Identity,
}

impl std::fmt::Debug for SessionFileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionFileStore").finish_non_exhaustive()
    }
}

impl SessionFileStore {
    pub fn new(
        base_url: &BaseUrl,
        identity_scope: Option<&str>,
        username: &str,
    ) -> Result<Self, AppError> {
        Self::at(data_root_dir()?, base_url, identity_scope, username)
    }

    fn at(
        directory: PathBuf,
        base_url: &BaseUrl,
        identity_scope: Option<&str>,
        username: &str,
    ) -> Result<Self, AppError> {
        ensure_root_dir_at(&directory)?;
        Ok(Self {
            directory,
            identity: Identity {
                base_url: base_url.as_str().to_owned(),
                identity_scope: identity_scope.map(str::to_owned),
                username: username.to_owned(),
            },
        })
    }
    fn lock_guard(&self) -> Result<FileGuard, ApiError> {
        // Lock a stable sidecar, never the atomically replaced data inode.
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.directory.join("sessions.lock"))?;
        secure_file(&lock)?;
        lock.lock()?;
        Ok(FileGuard {
            _lock: lock,
            path: self.directory.join("sessions.json"),
            identity: self.identity.clone(),
        })
    }
}

impl SessionStore for SessionFileStore {
    fn lock(&self) -> Result<Box<dyn SessionStoreGuard>, ApiError> {
        Ok(Box::new(self.lock_guard()?))
    }
}

struct FileGuard {
    _lock: File,
    path: PathBuf,
    identity: Identity,
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
