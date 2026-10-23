use std::fmt::{Debug, Formatter};
use std::io::{Error, ErrorKind};
use std::sync::Arc;

use hubuum_client::{ApiError, SessionStore, SessionStoreGuard, StoredSession};
use keyring_core::{CredentialStore, Entry, Error as KeyringError};

use super::{FileGuard, SessionFileStore};

const SERVICE: &str = "org.hubuum.cli.sessions";

pub(super) struct KeychainStore {
    files: SessionFileStore,
    entry: Arc<Entry>,
}

impl Debug for KeychainStore {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeychainStore").finish_non_exhaustive()
    }
}

impl KeychainStore {
    pub fn new(files: SessionFileStore) -> Result<Self, ApiError> {
        Self::with_backend(files, native_backend()?)
    }

    fn with_backend(
        files: SessionFileStore,
        backend: Arc<CredentialStore>,
    ) -> Result<Self, ApiError> {
        // Include the canonical data directory so a different lock file can
        // never control the same credential. Identifiers contain no secrets.
        let directory = files.directory.canonicalize()?;
        let account = serde_json::to_string(&(directory, &files.identity))
            .map_err(|_| Error::other("Could not identify keychain session"))?;
        let entry = backend
            .build(SERVICE, &account, None)
            .map_err(keychain_error)?;
        Ok(Self {
            files,
            entry: Arc::new(entry),
        })
    }
}

impl SessionStore for KeychainStore {
    fn lock(&self) -> Result<Box<dyn SessionStoreGuard>, ApiError> {
        Ok(Box::new(KeychainGuard {
            files: self.files.lock_guard()?,
            entry: self.entry.clone(),
        }))
    }
}

struct KeychainGuard {
    files: FileGuard,
    entry: Arc<Entry>,
}

impl KeychainGuard {
    fn save_session(&self, session: Option<&StoredSession>) -> Result<(), ApiError> {
        match session {
            Some(session) => {
                let bytes = serde_json::to_vec(session)
                    .map_err(|_| Error::other("Could not serialize keychain session"))?;
                self.entry.set_secret(&bytes).map_err(keychain_error)?;
            }
            None => match self.entry.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => {}
                Err(error) => return Err(keychain_error(error)),
            },
        }
        self.remove_file_copy()
    }

    fn remove_file_copy(&self) -> Result<(), ApiError> {
        if self.files.load()?.is_some() {
            self.files.write_session(None)?;
        }
        Ok(())
    }
}

impl SessionStoreGuard for KeychainGuard {
    fn load(&self) -> Result<Option<StoredSession>, ApiError> {
        match self.entry.get_secret() {
            Ok(bytes) => {
                let session = serde_json::from_slice(&bytes).map_err(|_| {
                    Error::new(ErrorKind::InvalidData, "Invalid session in keychain; remove the Hubuum CLI entry and sign in again")
                })?;
                // Also completes migration after a previous file cleanup failed.
                self.remove_file_copy()?;
                Ok(Some(session))
            }
            Err(KeyringError::NoEntry) => {
                let session = self.files.load()?;
                if let Some(session) = &session {
                    // Under the shared lock, write the secure copy first. Never
                    // use or remove the file credential if that write fails.
                    self.save_session(Some(session))?;
                }
                Ok(session)
            }
            Err(error) => Err(keychain_error(error)),
        }
    }

    fn save(&mut self, session: Option<&StoredSession>) -> Result<(), ApiError> {
        self.save_session(session)
    }
}

fn keychain_error(_error: KeyringError) -> ApiError {
    // Backend errors may contain credential attributes or reflected secrets.
    Error::other("Could not access the session keychain; unlock macOS Keychain or start/unlock a Linux Secret Service, then retry. No file-storage fallback was used.").into()
}

#[cfg(target_os = "linux")]
fn native_backend() -> Result<Arc<CredentialStore>, ApiError> {
    dbus_secret_service_keyring_store::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(keychain_error)
}

#[cfg(test)]
mod tests {
    use std::fs::{read, write};
    use std::path::Path;

    use hubuum_client::BaseUrl;
    use keyring_core::mock;
    use tempfile::tempdir;

    use super::*;
    use crate::tests::session;

    fn files(directory: &Path, user: &str) -> SessionFileStore {
        SessionFileStore::at(
            directory.to_owned(),
            &BaseUrl::new("https://example.invalid").unwrap(),
            None,
            user,
        )
        .unwrap()
    }

    fn fail_next(entry: &Entry) {
        entry
            .as_any()
            .downcast_ref::<mock::Cred>()
            .unwrap()
            .set_error(KeyringError::Invalid(
                "reflected-secret".into(),
                "reflected-secret".into(),
            ));
    }

    #[test]
    fn migrates_only_matching_identity_and_resumes_from_keychain() {
        let directory = tempdir().unwrap();
        let backend = mock::Store::new().unwrap();
        for user in ["alice", "bob"] {
            files(directory.path(), user)
                .lock()
                .unwrap()
                .save(Some(&session()))
                .unwrap();
        }
        let store =
            KeychainStore::with_backend(files(directory.path(), "alice"), backend.clone()).unwrap();
        assert!(store.lock().unwrap().load().unwrap().is_some());
        assert!(files(directory.path(), "alice")
            .lock()
            .unwrap()
            .load()
            .unwrap()
            .is_none());
        assert!(files(directory.path(), "bob")
            .lock()
            .unwrap()
            .load()
            .unwrap()
            .is_some());
        assert!(!format!("{store:?}").contains("alice"));
        drop(store);

        let resumed =
            KeychainStore::with_backend(files(directory.path(), "alice"), backend).unwrap();
        let mut guard = resumed.lock().unwrap();
        assert!(guard.load().unwrap().is_some());
        guard.save(None).unwrap();
        assert!(guard.load().unwrap().is_none());
        assert!(matches!(
            resumed.entry.get_secret(),
            Err(KeyringError::NoEntry)
        ));
    }

    #[test]
    fn keychain_errors_do_not_fall_back_or_destroy_file_credentials() {
        let directory = tempdir().unwrap();
        files(directory.path(), "alice")
            .lock()
            .unwrap()
            .save(Some(&session()))
            .unwrap();
        let path = directory.path().join("sessions.json");
        let original = read(&path).unwrap();
        let store = KeychainStore::with_backend(
            files(directory.path(), "alice"),
            mock::Store::new().unwrap(),
        )
        .unwrap();
        let mut guard = store.lock().unwrap();

        fail_next(&store.entry);
        let error = guard.load().unwrap_err();
        assert!(!format!("{error:?} {error}").contains("reflected-secret"));
        fail_next(&store.entry);
        assert!(guard.save(Some(&session())).is_err());
        assert_eq!(read(&path).unwrap(), original);
        assert!(matches!(
            store.entry.get_secret(),
            Err(KeyringError::NoEntry)
        ));

        // A later successful migration removes the plaintext copy.
        assert!(guard.load().unwrap().is_some());
        assert_eq!(read(&path).unwrap(), b"[]");
        fail_next(&store.entry);
        assert!(guard.save(None).is_err());
        assert!(store.entry.get_secret().is_ok());
    }

    #[test]
    fn failed_file_cleanup_is_retried_before_a_session_is_returned() {
        let directory = tempdir().unwrap();
        files(directory.path(), "alice")
            .lock()
            .unwrap()
            .save(Some(&session()))
            .unwrap();
        let path = directory.path().join("sessions.json");
        let original = read(&path).unwrap();
        let store = KeychainStore::with_backend(
            files(directory.path(), "alice"),
            mock::Store::new().unwrap(),
        )
        .unwrap();
        let mut guard = store.lock().unwrap();
        write(&path, "corrupt-secret").unwrap();
        assert!(guard.save(Some(&session())).is_err());
        assert!(store.entry.get_secret().is_ok());
        assert!(guard.load().is_err());
        write(&path, original).unwrap();
        assert!(guard.load().unwrap().is_some());
        assert_eq!(read(&path).unwrap(), b"[]");
    }

    #[test]
    fn corrupt_keychain_credentials_are_not_replaced_by_plaintext_fallback() {
        let directory = tempdir().unwrap();
        files(directory.path(), "alice")
            .lock()
            .unwrap()
            .save(Some(&session()))
            .unwrap();
        let store = KeychainStore::with_backend(
            files(directory.path(), "alice"),
            mock::Store::new().unwrap(),
        )
        .unwrap();
        store.entry.set_secret(b"corrupt-secret").unwrap();
        let error = store.lock().unwrap().load().unwrap_err();
        assert!(!format!("{error:?} {error}").contains("corrupt-secret"));
        assert_eq!(store.entry.get_secret().unwrap(), b"corrupt-secret");
        assert!(files(directory.path(), "alice")
            .lock()
            .unwrap()
            .load()
            .unwrap()
            .is_some());
    }

    #[test]
    fn independent_lock_directories_use_independent_keychain_entries() {
        let one = tempdir().unwrap();
        let two = tempdir().unwrap();
        let backend = mock::Store::new().unwrap();
        let first =
            KeychainStore::with_backend(files(one.path(), "alice"), backend.clone()).unwrap();
        let second = KeychainStore::with_backend(files(two.path(), "alice"), backend).unwrap();
        first.lock().unwrap().save(Some(&session())).unwrap();
        assert!(second.lock().unwrap().load().unwrap().is_none());
        assert!(!one.path().join("sessions.json").exists());
    }

    // This only creates an entry scoped to a unique temporary directory; cleanup
    // removes it even if the test fails. CI supplies a disposable unlocked store.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    #[ignore = "requires an unlocked native credential store"]
    fn native_keychain_round_trip() {
        struct Cleanup(Arc<Entry>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.delete_credential();
            }
        }

        let directory = tempdir().unwrap();
        let first = KeychainStore::new(files(directory.path(), "integration-fixture")).unwrap();
        let _cleanup = Cleanup(first.entry.clone());
        first.lock().unwrap().save(Some(&session())).unwrap();
        drop(first);
        let second = KeychainStore::new(files(directory.path(), "integration-fixture")).unwrap();
        let mut guard = second.lock().unwrap();
        assert!(guard.load().unwrap().is_some());
        guard.save(None).unwrap();
        assert!(guard.load().unwrap().is_none());
        assert!(!directory.path().join("sessions.json").exists());
    }
}

#[cfg(target_os = "macos")]
fn native_backend() -> Result<Arc<CredentialStore>, ApiError> {
    apple_native_keyring_store::keychain::Store::new()
        .map(|store| store as Arc<CredentialStore>)
        .map_err(keychain_error)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn native_backend() -> Result<Arc<CredentialStore>, ApiError> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "Keychain session storage is supported on macOS and Linux",
    )
    .into())
}
