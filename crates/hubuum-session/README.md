# hubuum-session

Persistent, identity-scoped storage for the Hubuum SDK's rotating sessions.
The application supplies the directory, normalized server URL, identity provider,
username, and storage mode. This crate reads no application configuration and
does not prompt or log in.

- `SessionIdentity::new` rejects blank usernames and provider names.
- `SessionOptions::new(directory, identity).storage(mode)` selects file, native
  keychain, or disabled persistence. Files are the default.
- `SessionCache::open` returns `None` for disabled storage without filesystem or
  keychain access. `store()` intentionally exposes the SDK's `SessionStore`
  integration interface.
- `lock_login()` holds a process lock across the final cache recheck and
  login/save. Acquire it after password entry. Refresh uses only the SDK's store
  lock; never acquire the login lock while holding a store guard.

Files retain the CLI's existing `sessions.json` layout and stable sidecar locks.
Writes are atomic and durable; Unix directories are owner-only and existing
credential/lock files have their permissions repaired before use. Native storage
retains the existing service/account identifiers and file-to-keychain migration.
Passwords are never persisted. Linux requires an unlocked Secret Service; macOS
uses Keychain. Native-store failures never fall back to plaintext storage.

The minimum Rust version is 1.89 for standard filesystem locking. SDK bindings,
rotation, consumed-refresh invalidation, and matching-family logout semantics
remain owned by `hubuum_client`.
