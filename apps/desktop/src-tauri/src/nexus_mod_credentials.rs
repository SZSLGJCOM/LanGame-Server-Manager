//! Nexus credentials belong to the host user's system store, never instance data.
use keyring::{Entry, Error};

const SERVICE: &str = "LanGameServerManager/NexusMods";
const ACCOUNT: &str = "personal-api-key";

// Deliberately has no Debug, Display or serialization implementation.
pub(super) struct ApiKey(String);

impl ApiKey {
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}

pub(super) async fn read() -> Result<Option<ApiKey>, String> {
    tokio::task::spawn_blocking(|| {
        let entry = Entry::new(SERVICE, ACCOUNT)
            .map_err(|_| "The system credential store is unavailable.".to_string())?;
        match entry.get_password() {
            Ok(secret) if secret.is_empty() => Ok(None),
            Ok(secret) => Ok(Some(ApiKey(secret))),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => Err("The Nexus API key could not be read from the system store.".into()),
        }
    })
    .await
    .map_err(|_| "The Nexus credential read did not complete.".to_string())?
}

pub(super) async fn write(api_key: String) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("Enter a Nexus API key before saving it.".into());
    }
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE, ACCOUNT)
            .map_err(|_| "The system credential store is unavailable.".to_string())?;
        entry
            .set_password(&api_key)
            .map_err(|_| "The Nexus API key could not be saved in the system store.".to_string())
    })
    .await
    .map_err(|_| "The Nexus credential write did not complete.".to_string())?
}

pub(super) async fn clear() -> Result<(), String> {
    tokio::task::spawn_blocking(|| {
        let entry = Entry::new(SERVICE, ACCOUNT)
            .map_err(|_| "The system credential store is unavailable.".to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(_) => Err("The Nexus API key could not be cleared from the system store.".into()),
        }
    })
    .await
    .map_err(|_| "The Nexus credential deletion did not complete.".to_string())?
}
