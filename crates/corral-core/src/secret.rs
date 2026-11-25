//! Persistent secrets (today: just the Linear API key), stored in the OS
//! credential store where one is available, and in an owner-only file
//! otherwise.
//!
//! This is a small layer over three things that have nothing in common except
//! intent, so it deliberately owns only the policy, not the mechanism:
//!
//! - **Linux:** the Secret Service over D-Bus (GNOME Keyring, KWallet), via
//!   `zbus-secret-service-keyring-store`. Not every Linux desktop has a
//!   Secret Service provider — a bare WM, a headless/SSH session, or a distro
//!   without one is the normal case, not the exotic one — so "no store" is a
//!   routine answer here, not an error.
//! - **macOS:** the login keychain (`SecItem*`), via
//!   `apple-native-keyring-store`'s `keychain` module. That's the variant
//!   unsigned, unprovisioned apps can use; the `protected` module needs a
//!   provisioning profile corral doesn't have.
//! - **Windows:** the Credential Manager (DPAPI-encrypted), via
//!   `windows-native-keyring-store`.
//! - **Anything else** (FreeBSD, OpenBSD, …): no backend, file only. Corral
//!   only ships Linux/macOS bundles and Windows is a work in progress, so
//!   those targets get the fallback rather than an untestable implementation.
//!   (Linux's `target_os` already excludes the BSDs, so no extra cfg is needed
//!   to express that.)
//!
//! Whenever the OS store is unavailable — *or* simply refuses to write — the
//! value falls back to a `0600` file in the config dir. That file is the
//! pre-keyring behavior, kept deliberately: a keyring can fail at
//! runtime (locked keychain, dismissed prompt, no session bus) in ways no
//! compile-time check can predict, and losing the user's Linear key because a
//! desktop doesn't run gnome-keyring would be a worse bug than storing it in a
//! file nobody else can read. [`backend_description`] reports which one is in
//! play so Settings can say so instead of implying the stronger guarantee.
//!
//! The environment variable [`SECRET_FILE_ONLY_ENV`] forces the file backend.
//! Used by the test suite (so tests never touch a real keychain), and useful
//! for debugging a keyring problem without changing code.

use std::path::Path;

use crate::error::Result;

/// Set this to `1` to skip the OS store entirely and use only the config file.
pub const SECRET_FILE_ONLY_ENV: &str = "CORRAL_SECRET_FILE_ONLY";

/// Where a secret is actually stored. Ordered by preference; the first that
/// works wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The OS credential store (Secret Service / Keychain / Credential
    /// Manager). The value is not in any corral-readable plaintext file.
    OsStore,
    /// `~/.config/corral/<name>` with `0600` permissions.
    File,
}

impl Backend {
    /// Human-readable wording for Settings, so the UI can be honest about
    /// where the secret actually lives.
    pub fn description(self) -> &'static str {
        match self {
            Backend::OsStore => "saved in your system keychain",
            Backend::File => {
                "no system keychain available — saved in an owner-only file in corral's config directory"
            }
        }
    }
}

/// Service name for the OS store. Carries the config-dir profile so a
/// `just run-dev` instance can't read (or overwrite) the live app's key: the
/// profile dir is the identity everyone else in corral uses.
fn service_name(config_dir: &Path) -> String {
    // A dev profile lives at `<config>/profiles/<name>`, so the *parent*
    // directory name distinguishes it from the default `<config>` root.
    let is_profile = config_dir
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|p| p == "profiles");
    if is_profile {
        let name = config_dir.file_name().unwrap_or_default().to_string_lossy();
        format!("dev.corral.desktop.dev.{name}")
    } else {
        "dev.corral.desktop".to_string()
    }
}

/// The OS store backend, if this build/platform has one and it can be opened.
/// `None` means "fall back to the file" — every failure is logged here and
/// handled by the caller as an ordinary fallback, since a locked or absent
/// keyring must never make corral unusable.
/// The OS store, erased to its trait object so the rest of this module has one
/// concrete type regardless of platform.
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn os_store() -> Option<std::sync::Arc<keyring_core::CredentialStore>> {
    secret_store::registration()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn os_store() -> Option<std::sync::Arc<keyring_core::CredentialStore>> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod secret_store {
    /// Placeholder so the `os_store` signature above compiles everywhere; the
    /// `None` it returns is the whole point of the unsupported-OS branch.
    pub type OsStore = std::convert::Infallible;
}

// One `registration` per platform, each hiding the store crate's exact
// construction and the "is it usable" probe behind the same signature.

#[cfg(target_os = "linux")]
mod secret_store {
    use std::sync::Arc;

    pub type OsStore = Arc<zbus_secret_service_keyring_store::Store>;

    /// zbus's blocking calls need a Tokio runtime on the current thread. corral
    /// already depends on Tokio through Tauri, but a core-level caller (the
    /// CLI) may not have one, so each operation enters a dedicated one.
    pub fn runtime() -> Option<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()
    }

    pub fn registration() -> Option<Arc<keyring_core::CredentialStore>> {
        let _enter = runtime()?.enter();
        let store: OsStore = zbus_secret_service_keyring_store::Store::new().ok()?;
        Some(store)
    }
}

#[cfg(target_os = "macos")]
mod secret_store {
    use std::sync::Arc;

    pub type OsStore = Arc<apple_native_keyring_store::keychain::Store>;

    pub fn registration() -> Option<Arc<keyring_core::CredentialStore>> {
        // The keychain backend needs no runtime; it talks to
        // Security.framework directly.
        let store: OsStore = apple_native_keyring_store::keychain::Store::new().ok()?;
        Some(store)
    }
}

#[cfg(target_os = "windows")]
mod secret_store {
    use std::sync::Arc;

    pub type OsStore = Arc<windows_native_keyring_store::Store>;

    pub fn registration() -> Option<Arc<keyring_core::CredentialStore>> {
        let store: OsStore = windows_native_keyring_store::Store::new().ok()?;
        Some(store)
    }
}

/// Read a secret by name, or `None` when nothing is stored.
///
/// An OS-store read that *fails* falls through to the file rather than
/// returning an error, for the same reason every fallback here exists: a
/// keychain that is locked or prompting must not look like "no key set" to a
/// command that merely wants to call Linear, and must not make Settings
/// unusable.
pub fn get(name: &str, config_dir: &Path) -> Result<Option<String>> {
    if !file_only()
        && let Some(store) = os_store()
        && let Some(value) = read_os(&store, config_dir, name)
    {
        return Ok(Some(value));
    }
    crate::settings::read_secret_file(&secret_path(config_dir, name))
        .map(|v| v.filter(|v| !v.is_empty()))
}

/// Store `value` under `name`, preferring the OS store and falling back to the
/// `0600` config file. Returns the backend actually used.
pub fn set(name: &str, config_dir: &Path, value: &str) -> Result<Backend> {
    let value = value.trim();
    if value.is_empty() {
        return clear(name, config_dir).map(|_| Backend::File);
    }
    if !file_only()
        && let Some(store) = os_store()
        && write_os(&store, config_dir, name, value)
    {
        // A leftover copy must not outlive the keyring write, or a later
        // keyring failure would silently resurrect an old key.
        let _ = crate::settings::remove_secret_file(&secret_path(config_dir, name));
        return Ok(Backend::OsStore);
    }
    crate::settings::write_secret_file(&secret_path(config_dir, name), value)?;
    Ok(Backend::File)
}

/// Forget a secret, in both places. Removing from both is intentional: the
/// value may exist in either (or, after a mid-clear failure, in both), and a
/// "remove key" that leaves a stale copy behind is worse than one extra
/// no-op delete.
pub fn clear(name: &str, config_dir: &Path) -> Result<()> {
    if !file_only()
        && let Some(store) = os_store()
    {
        delete_os(&store, config_dir, name);
    }
    crate::settings::remove_secret_file(&secret_path(config_dir, name))
}

/// Which backend would be used right now — for Settings' "where is this
/// stored" hint. Triggers no writes and never clears anything.
///
/// Takes the config dir for symmetry with the rest of the module and so the
/// call site reads the same regardless of how the probe changes later (a
/// per-profile store, say). Neither backend needs it to answer.
pub fn backend(_config_dir: &Path) -> Backend {
    if !file_only() && os_store().is_some() {
        Backend::OsStore
    } else {
        Backend::File
    }
}

/// [`backend`]'s wording, for surfacing in the UI.
pub fn backend_description(config_dir: &Path) -> &'static str {
    backend(config_dir).description()
}

fn secret_path(config_dir: &Path, name: &str) -> std::path::PathBuf {
    config_dir.join(name)
}

fn file_only() -> bool {
    matches!(
        std::env::var(SECRET_FILE_ONLY_ENV).as_deref(),
        Ok("1") | Ok("true")
    )
}

// The OS-store calls are all "best effort with a logged reason". They return
// an Option/bool instead of a Result on purpose: the caller has exactly one
// recovery (the file), so distinguishing error kinds here would be dead code.

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(unused_variables)
)]
fn read_os(
    store: &std::sync::Arc<keyring_core::CredentialStore>,
    config_dir: &Path,
    name: &str,
) -> Option<String> {
    let entry = entry(store, config_dir, name).ok()?;
    match entry.get_password() {
        Ok(value) => Some(value),
        Err(e) => {
            log_os_failure("read", name, &e);
            None
        }
    }
}

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(unused_variables)
)]
fn write_os(
    store: &std::sync::Arc<keyring_core::CredentialStore>,
    config_dir: &Path,
    name: &str,
    value: &str,
) -> bool {
    let entry = match entry(store, config_dir, name) {
        Ok(entry) => entry,
        Err(e) => {
            log_os_failure("open", name, &e);
            return false;
        }
    };
    match entry.set_password(value) {
        Ok(()) => true,
        Err(e) => {
            log_os_failure("write", name, &e);
            false
        }
    }
}

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(unused_variables)
)]
fn delete_os(store: &std::sync::Arc<keyring_core::CredentialStore>, config_dir: &Path, name: &str) {
    let Ok(entry) = entry(store, config_dir, name) else {
        return;
    };
    // NoEntry is the normal "wasn't there" case, not a failure.
    if let Err(e) = entry.delete_credential() {
        log_os_failure("delete", name, &e);
    }
}

fn entry(
    store: &std::sync::Arc<keyring_core::CredentialStore>,
    config_dir: &Path,
    name: &str,
) -> keyring_core::Result<keyring_core::Entry> {
    keyring_core::Entry::new_in_store(
        store,
        &service_name(config_dir),
        // The "user" attribute is really "which secret": one entry per name,
        // under a service that already identifies the config profile.
        name,
    )
}

fn log_os_failure(action: &str, name: &str, e: &keyring_core::Error) {
    // `debug`, not `warn`: an absent/locked keyring is routine on Linux, and
    // the fallback is expected. This is here so `corral doctor`-style log
    // digging can tell "file backend because no keyring" from "file backend
    // because the keyring write failed".
    tracing::debug!("secret: OS store {action} for {name:?} failed ({e}); using the file fallback");
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Serializes every test that reads or writes the secret store's
    /// process-global env var — including `settings::tests`, which exercises
    /// the store through save/load and the legacy migrations. Shared so none
    /// of them can ever reach a real keychain (CI has none; a developer's
    /// machine shouldn't collect test entries).
    pub(crate) static FILE_ONLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Tests must never touch a real keychain, so the module's logic is
    /// exercised against the file backend.
    fn force_file_backend() -> std::sync::MutexGuard<'static, ()> {
        // Env vars are process-global; serialize the tests that touch them.
        let guard = FILE_ONLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: Rust 2024 makes `set_var` unsafe because a concurrent reader
        // could see a torn value. The lock above serializes these tests, and
        // the only reader is `file_only()`, called from the same test bodies.
        unsafe { std::env::set_var(SECRET_FILE_ONLY_ENV, "1") };
        guard
    }

    #[test]
    fn round_trips_through_the_file_backend() {
        let _guard = force_file_backend();
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(get("linear_api_key", dir.path()).unwrap(), None);
        assert_eq!(
            set("linear_api_key", dir.path(), " lin_api_x ").unwrap(),
            Backend::File
        );
        assert_eq!(
            get("linear_api_key", dir.path()).unwrap().as_deref(),
            Some("lin_api_x")
        );

        clear("linear_api_key", dir.path()).unwrap();
        assert_eq!(get("linear_api_key", dir.path()).unwrap(), None);
        // Clearing twice is fine — "remove key" must be idempotent.
        clear("linear_api_key", dir.path()).unwrap();
    }

    #[test]
    fn file_backend_is_owner_only() {
        let _guard = force_file_backend();
        let dir = tempfile::tempdir().unwrap();
        set("linear_api_key", dir.path(), "lin_api_y").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("linear_api_key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn setting_an_empty_value_clears() {
        let _guard = force_file_backend();
        let dir = tempfile::tempdir().unwrap();
        set("linear_api_key", dir.path(), "lin_api_z").unwrap();
        set("linear_api_key", dir.path(), "   ").unwrap();
        assert_eq!(get("linear_api_key", dir.path()).unwrap(), None);
    }

    #[test]
    fn profiles_do_not_share_a_service_name() {
        // `just run-dev` must not read or clobber the live app's entry.
        assert_eq!(
            service_name(Path::new("/home/u/.config/corral")),
            "dev.corral.desktop"
        );
        assert_eq!(
            service_name(Path::new("/home/u/.config/corral/profiles/dev")),
            "dev.corral.desktop.dev.dev"
        );
        assert_ne!(
            service_name(Path::new("/home/u/.config/corral")),
            service_name(Path::new("/home/u/.config/corral/profiles/dev"))
        );
    }
}
