use keyring::{Entry, Error};

const SERVICE: &str = "LanGame.ServerManager.Satisfactory";

pub(super) async fn read(identity: &str, kind: &str) -> Result<Option<String>, String> {
    let account = format!("{identity}/{kind}");
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE, &account)
            .map_err(|_| "The system credential store is unavailable.".to_string())?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(Error::NoEntry) => Ok(None),
            Err(_) => {
                Err("The Satisfactory credential could not be read from the system store.".into())
            }
        }
    })
    .await
    .map_err(|_| "The Satisfactory credential read did not complete.".to_string())?
}

pub(super) async fn write(identity: &str, kind: &str, secret: String) -> Result<(), String> {
    let account = format!("{identity}/{kind}");
    tokio::task::spawn_blocking(move || {
        let entry = Entry::new(SERVICE, &account)
            .map_err(|_| "The system credential store is unavailable.".to_string())?;
        entry.set_password(&secret).map_err(|_| {
            "The Satisfactory credential could not be saved in the system store.".to_string()
        })
    })
    .await
    .map_err(|_| "The Satisfactory credential write did not complete.".to_string())?
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    struct SyntheticCredential(Entry);

    impl Drop for SyntheticCredential {
        fn drop(&mut self) {
            if let Err(error) = self.0.delete_credential()
                && !matches!(error, Error::NoEntry)
            {
                eprintln!("Synthetic Satisfactory credential cleanup failed: {error}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "writes and deletes one uniquely named synthetic credential in the current Windows user's system store"]
    async fn windows_store_round_trips_a_unique_synthetic_credential() {
        let identity = format!("synthetic-smoke-{}", uuid::Uuid::new_v4().simple());
        let kind = "api-token";
        let entry = Entry::new(SERVICE, &format!("{identity}/{kind}")).unwrap();
        assert!(matches!(entry.get_password(), Err(Error::NoEntry)));
        // The guard only owns this new UUID namespace, never an existing entry.
        let guard = SyntheticCredential(entry);
        assert!(read(&identity, kind).await.unwrap().is_none());
        let synthetic = format!("MOCK-ONLY-{}", uuid::Uuid::new_v4().simple());
        write(&identity, kind, synthetic.clone()).await.unwrap();
        assert_eq!(read(&identity, kind).await.unwrap(), Some(synthetic));
        guard.0.delete_credential().unwrap();
        assert!(read(&identity, kind).await.unwrap().is_none());
    }
}
