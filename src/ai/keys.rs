//! The user's API keys, one per [`Service`] that needs one. They live in the OS keychain through
//! `keyring`, never in `settings.json`, a project or the debug log. A service's environment variable
//! (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`) wins over its saved key.

use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use super::Service;

#[cfg(not(test))]
const KEYCHAIN_SERVICE: &str = "Kerf";

/// The keychain account a service's key is saved under.
fn account(service: Service) -> &'static str {
    match service {
        Service::Anthropic => "anthropic-api-key",
        Service::OpenAi => "openai-api-key",
        Service::OpenRouter => "openrouter-api-key",
        Service::Local => "local-api-key",
    }
}

/// Where saved keys are kept, by account. Tests use [`MemoryKeys`], so no test reads the real keychain.
pub trait KeyStore: Send {
    fn load(&self, account: &str) -> Option<String>;
    fn save(&self, account: &str, key: &str) -> Result<(), String>;
    fn forget(&self, account: &str) -> Result<(), String>;
}

/// The OS keychain: Keychain on macOS, Credential Manager on Windows, the kernel keyring on Linux.
/// Not built into tests, so none can reach it.
#[cfg(not(test))]
pub struct Keychain;

#[cfg(not(test))]
impl Keychain {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())
    }
}

#[cfg(not(test))]
impl KeyStore for Keychain {
    fn load(&self, account: &str) -> Option<String> {
        Self::entry(account).ok()?.get_password().ok().filter(|k| !k.trim().is_empty())
    }

    fn save(&self, account: &str, key: &str) -> Result<(), String> {
        Self::entry(account)?.set_password(key).map_err(|e| e.to_string())
    }

    fn forget(&self, account: &str) -> Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryKeys(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl KeyStore for MemoryKeys {
    fn load(&self, account: &str) -> Option<String> {
        self.0.lock().ok()?.get(account).cloned()
    }

    fn save(&self, account: &str, key: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.insert(account.into(), key.into());
        Ok(())
    }

    fn forget(&self, account: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(account);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Environment,
    Saved,
}

/// The key to use for each service and where it came from. Each saved key is read once, when the
/// panel first needs it, so the keychain is not asked every frame.
pub struct Keys {
    store: Box<dyn KeyStore>,
    env: HashMap<Service, String>,
    saved: HashMap<Service, Option<String>>,
}

impl Keys {
    /// `env` reads an environment variable.
    pub fn new(store: Box<dyn KeyStore>, env: impl Fn(&str) -> Option<String>) -> Self {
        let env = Service::ALL.into_iter().filter_map(|s| Some((s, env(s.env_var()?)?.trim().to_owned()))).filter(|(_, k)| !k.is_empty()).collect();
        Self { store, env, saved: HashMap::new() }
    }

    /// The keychain, with the services' environment variables.
    #[cfg(not(test))]
    pub fn system() -> Self {
        Self::new(Box::new(Keychain), |name| std::env::var(name).ok())
    }

    pub fn current(&mut self, service: Service) -> Option<(String, Source)> {
        if let Some(key) = self.env.get(&service) {
            return Some((key.clone(), Source::Environment));
        }
        let store = &self.store;
        let saved = self.saved.entry(service).or_insert_with(|| store.load(account(service)));
        saved.as_ref().map(|k| (k.trim().to_owned(), Source::Saved))
    }

    /// Saves a pasted key. Whitespace around it is dropped; whitespace inside means a bad paste.
    pub fn save(&mut self, service: Service, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err("That doesn't look like an API key.".into());
        }
        self.store.save(account(service), key)?;
        self.saved.insert(service, Some(key.to_owned()));
        tracing::info!(target: "ai", ?service, "key saved");
        Ok(())
    }

    pub fn forget(&mut self, service: Service) -> Result<(), String> {
        self.store.forget(account(service))?;
        self.saved.insert(service, None);
        tracing::info!(target: "ai", ?service, "key forgotten");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn each_service_keeps_its_own_key_and_its_variable_wins() {
        let mut keys = Keys::new(Box::new(MemoryKeys::default()), none);
        assert_eq!(keys.current(Service::Anthropic), None);
        keys.save(Service::Anthropic, "  sk-ant-saved  ").unwrap();
        keys.save(Service::OpenAi, "sk-proj-saved").unwrap();
        assert_eq!(keys.current(Service::Anthropic), Some(("sk-ant-saved".into(), Source::Saved)));
        assert_eq!(keys.current(Service::OpenAi), Some(("sk-proj-saved".into(), Source::Saved)));
        assert_eq!(keys.current(Service::OpenRouter), None);
        let env = |name: &str| (name == "OPENROUTER_API_KEY").then(|| "sk-or-env".to_owned());
        let mut keys = Keys::new(Box::new(MemoryKeys::default()), env);
        keys.save(Service::OpenRouter, "sk-or-saved").unwrap();
        assert_eq!(keys.current(Service::OpenRouter), Some(("sk-or-env".into(), Source::Environment)));
    }

    #[test]
    fn a_bad_paste_is_refused_and_forget_clears_only_that_key() {
        let mut keys = Keys::new(Box::new(MemoryKeys::default()), |_| Some("   ".into()));
        assert!(keys.save(Service::Anthropic, "sk-ant one").is_err());
        assert!(keys.save(Service::Anthropic, "").is_err());
        keys.save(Service::Anthropic, "sk-ant-x").unwrap();
        keys.save(Service::OpenAi, "sk-y").unwrap();
        keys.forget(Service::Anthropic).unwrap();
        assert_eq!(keys.current(Service::Anthropic), None);
        assert!(keys.current(Service::OpenAi).is_some());
    }
}
