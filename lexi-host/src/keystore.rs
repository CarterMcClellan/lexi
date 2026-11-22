//! Where saved API keys live. On macOS and Windows that's the OS credential
//! store (the login Keychain / Credential Manager): the key is encrypted at
//! rest and other apps must ask before reading it. Elsewhere — or with
//! `LEXI_KEY_STORE=file` (tests, CI, headless machines) — it's the provider's
//! owner-only `providers/<name>/api-key` file.
//!
//! Keychain items are service `Lexi`, account `<provider>`; a non-default
//! `$LEXI_HOME` gets its own items (`<provider> (<home>)`) so separate homes
//! never share keys.

use std::path::Path;

/// The keychain service name every Lexi item is filed under.
pub(crate) const SERVICE: &str = "Lexi";

/// Whether this platform has an OS credential store Lexi can use.
pub(crate) const NATIVE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Where `providers api-key set` saves a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyStore {
    Keychain,
    File,
}

impl KeyStore {
    /// The store in effect: `LEXI_KEY_STORE=file|keychain`, else the OS store
    /// when there is one.
    pub(crate) fn current() -> KeyStore {
        match std::env::var("LEXI_KEY_STORE").as_deref() {
            Ok("file") => KeyStore::File,
            _ if NATIVE => KeyStore::Keychain,
            _ => KeyStore::File,
        }
    }

    /// `"keychain"` or `"file"` — also the `source` a key read from it reports.
    pub(crate) fn id(self) -> &'static str {
        match self {
            KeyStore::Keychain => "keychain",
            KeyStore::File => "file",
        }
    }

    /// What to call it in messages.
    pub(crate) fn label(self) -> &'static str {
        match self {
            KeyStore::Keychain if cfg!(target_os = "windows") => "Windows Credential Manager",
            KeyStore::Keychain => "macOS Keychain",
            KeyStore::File => "an owner-only key file",
        }
    }
}

/// The keychain account for `provider` under `home`.
pub(crate) fn account(home: &Path, provider: &str) -> String {
    let default_home = dirs::home_dir().map(|h| h.join(".lexi-cli"));
    if default_home.as_deref() == Some(home) {
        provider.to_string()
    } else {
        format!("{provider} ({})", home.display())
    }
}

/// A secret store keyed by account (the OS keychain, or a map in tests).
pub(crate) trait Secrets {
    /// The saved secret, `None` when there's no item.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    /// Delete the item; whether there was one.
    fn delete(&self, account: &str) -> Result<bool, String>;
}

/// The OS credential store.
pub(crate) struct OsSecrets;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Secrets for OsSecrets {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| e.to_string())
    }
    fn delete(&self, account: &str) -> Result<bool, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Secrets for OsSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("this platform has no supported credential store".into())
    }
    fn delete(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
}

/// The secret store to use, or `None` for the key file.
pub(crate) fn current_secrets() -> Option<&'static dyn Secrets> {
    match KeyStore::current() {
        KeyStore::Keychain => Some(&OsSecrets),
        KeyStore::File => None,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// An in-memory secret store; `fail` makes every call error.
    #[derive(Default)]
    pub(crate) struct MemSecrets {
        pub(crate) items: RefCell<HashMap<String, String>>,
        pub(crate) fail: bool,
    }

    impl super::Secrets for MemSecrets {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            Ok(self.items.borrow().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            self.items
                .borrow_mut()
                .insert(account.to_string(), secret.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<bool, String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            Ok(self.items.borrow_mut().remove(account).is_some())
        }
    }
}
