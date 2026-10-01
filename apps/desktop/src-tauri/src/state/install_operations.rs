use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_steamcmd::InstallCancellation;

/// Own cancellation handles until the provider has finished its cleanup.
#[derive(Default)]
pub(crate) struct InstallOperationRegistry {
    operations: Mutex<HashMap<String, InstallCancellation>>,
}

impl InstallOperationRegistry {
    pub(crate) fn begin(self: &Arc<Self>, id: String) -> Result<InstallOperationLease, String> {
        let mut operations = self.operations.lock().map_err(|_| lock_error())?;
        if operations.contains_key(&id) {
            return Err(String::from(
                "Installation operation is already registered.",
            ));
        }
        if operations.len() >= 24 {
            return Err(String::from("Too many installation operations are queued."));
        }
        let cancellation = InstallCancellation::new();
        operations.insert(id.clone(), cancellation.clone());
        Ok(InstallOperationLease {
            registry: Arc::clone(self),
            id,
            cancellation,
        })
    }

    pub(crate) fn request_cancel(&self, id: &str) -> Result<bool, String> {
        let operations = self.operations.lock().map_err(|_| lock_error())?;
        let Some(cancellation) = operations.get(id) else {
            return Ok(false);
        };
        cancellation.cancel();
        Ok(true)
    }

    pub(crate) fn request_cancel_all(&self) -> Result<(), String> {
        let operations = self.operations.lock().map_err(|_| lock_error())?;
        for cancellation in operations.values() {
            cancellation.cancel();
        }
        Ok(())
    }
}

pub(crate) struct InstallOperationLease {
    registry: Arc<InstallOperationRegistry>,
    id: String,
    cancellation: InstallCancellation,
}

impl InstallOperationLease {
    pub(crate) fn cancellation(&self) -> &InstallCancellation {
        &self.cancellation
    }
}

impl Drop for InstallOperationLease {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.registry
            .operations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

fn lock_error() -> String {
    String::from("Installation operation lock was poisoned.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_scoped_idempotent_and_remains_owned_until_cleanup() {
        let registry = Arc::new(InstallOperationRegistry::default());
        let first = registry.begin(String::from("first")).unwrap();
        let second = registry.begin(String::from("second")).unwrap();
        assert!(!registry.request_cancel("unknown").unwrap());
        assert!(registry.request_cancel("first").unwrap());
        assert!(registry.request_cancel("first").unwrap());
        assert!(first.cancellation().is_cancelled());
        assert!(!second.cancellation().is_cancelled());
        assert!(registry.begin(String::from("first")).is_err());
        drop(first);
        assert!(!registry.request_cancel("first").unwrap());
        let replacement = registry.begin(String::from("first")).unwrap();
        assert!(!replacement.cancellation().is_cancelled());
    }
}
