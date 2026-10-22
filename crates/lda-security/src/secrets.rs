use std::collections::HashMap;
use std::sync::Mutex;

use lda_common::{Error, Result, SECRET_SERVICE_NAME};

/// Stores API keys outside the configuration file.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

/// In-memory store for tests. Never used as the default on a desktop.
#[derive(Default)]
pub struct MemorySecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self
            .values
            .lock()
            .expect("secret lock")
            .get(account)
            .cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.values
            .lock()
            .expect("secret lock")
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.values.lock().expect("secret lock").remove(account);
        Ok(())
    }
}

/// Freedesktop secret service via the `keyring` crate (gnome-keyring, KWallet, …).
///
/// When the service is unavailable, callers fall back to the environment
/// variable named by `ProviderConfig::auth_env`. That fallback is not written
/// to disk by this process.
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    pub fn new() -> Self {
        Self {
            service: SECRET_SERVICE_NAME.to_string(),
        }
    }
}

impl Default for KeyringSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        let entry = keyring_entry(&self.service, account)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(err) if is_missing(&err) => Ok(None),
            Err(err) => Err(Error::Other(format!("secret service: {err}"))),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        let entry = keyring_entry(&self.service, account)?;
        entry
            .set_password(secret)
            .map_err(|err| Error::Other(format!("secret service: {err}")))
    }

    fn delete(&self, account: &str) -> Result<()> {
        let entry = keyring_entry(&self.service, account)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(err) if is_missing(&err) => Ok(()),
            Err(err) => Err(Error::Other(format!("secret service: {err}"))),
        }
    }
}

fn keyring_entry(service: &str, account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(service, account)
        .map_err(|err| Error::Other(format!("secret service: {err}")))
}

fn is_missing(err: &keyring::Error) -> bool {
    matches!(err, keyring::Error::NoEntry)
}

/// Resolve a key for a provider.
///
/// Secret service wins when it holds a value. Otherwise the named environment
/// variable is read. Neither value is logged by this function.
pub fn resolve_api_key(
    store: &dyn SecretStore,
    account: &str,
    auth_env: Option<&str>,
) -> Result<Option<String>> {
    if let Some(value) = store.get(account)? {
        if !value.is_empty() {
            return Ok(Some(value));
        }
    }
    let Some(name) = auth_env else {
        return Ok(None);
    };
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(Some(value)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `set_var` is process-global. Cargo runs these tests on several threads.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct ClearEnv<'a>(&'a str);

    impl Drop for ClearEnv<'_> {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    fn with_env(name: &str, value: &str, body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().expect("env test lock");
        std::env::set_var(name, value);
        let _clear = ClearEnv(name);
        body();
    }

    #[test]
    fn memory_store_round_trip() {
        let store = MemorySecretStore::default();
        assert!(store.get("openai").unwrap().is_none());
        store.set("openai", "sk-test-value").unwrap();
        assert_eq!(
            store.get("openai").unwrap().as_deref(),
            Some("sk-test-value")
        );
        store.delete("openai").unwrap();
        assert!(store.get("openai").unwrap().is_none());
    }

    #[test]
    fn env_fallback_when_store_empty() {
        let store = MemorySecretStore::default();
        with_env(
            "LDA_TEST_API_KEY_EMPTY_STORE",
            "from-env-not-a-real-key",
            || {
                let got = resolve_api_key(&store, "missing", Some("LDA_TEST_API_KEY_EMPTY_STORE"))
                    .unwrap();
                assert_eq!(got.as_deref(), Some("from-env-not-a-real-key"));
            },
        );
    }

    #[test]
    fn secret_service_wins_over_env() {
        let store = MemorySecretStore::default();
        store.set("openai", "from-store").unwrap();
        with_env("LDA_TEST_API_KEY_STORE_WINS", "from-env", || {
            let got =
                resolve_api_key(&store, "openai", Some("LDA_TEST_API_KEY_STORE_WINS")).unwrap();
            assert_eq!(got.as_deref(), Some("from-store"));
        });
    }
}
