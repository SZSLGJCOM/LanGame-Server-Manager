use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct StorageManagement {
    exclusive: Arc<tokio::sync::Mutex<()>>,
    scan: Mutex<Option<(String, Arc<AtomicBool>)>>,
}

pub(crate) struct StorageManagementLease {
    owner: Arc<StorageManagement>,
    _exclusive: tokio::sync::OwnedMutexGuard<()>,
}

impl StorageManagement {
    pub(crate) fn acquire(self: &Arc<Self>) -> Result<StorageManagementLease, String> {
        let exclusive = self.exclusive.clone().try_lock_owned().map_err(|_| {
            String::from("A storage scan or archive operation is in progress. Cancel the scan or wait for the operation to finish.")
        })?;
        Ok(StorageManagementLease {
            owner: self.clone(),
            _exclusive: exclusive,
        })
    }

    pub(crate) fn cancel(&self, scan_id: &str) -> Result<bool, String> {
        let scan = self.scan.lock().map_err(|_| "Storage scan lock poisoned")?;
        if let Some((id, cancellation)) = scan.as_ref()
            && id == scan_id
        {
            cancellation.store(true, Ordering::Release);
            return Ok(true);
        }
        Ok(false)
    }
}

impl StorageManagementLease {
    pub(crate) fn register_scan(
        &self,
        id: String,
        cancellation: Arc<AtomicBool>,
    ) -> Result<(), String> {
        *self
            .owner
            .scan
            .lock()
            .map_err(|_| "Storage scan lock poisoned")? = Some((id, cancellation));
        Ok(())
    }
}

impl Drop for StorageManagementLease {
    fn drop(&mut self) {
        if let Ok(mut scan) = self.owner.scan.lock() {
            *scan = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_keeps_exclusive_ownership_until_the_worker_finishes() {
        let registry = Arc::new(StorageManagement::default());
        let lease = registry.acquire().unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        lease
            .register_scan("first".into(), cancellation.clone())
            .unwrap();
        assert!(!registry.cancel("another-request").unwrap());
        assert!(!cancellation.load(Ordering::Acquire));
        assert!(registry.cancel("first").unwrap());
        assert!(cancellation.load(Ordering::Acquire));
        assert!(registry.acquire().is_err());
        drop(lease);
        assert!(!registry.cancel("first").unwrap());
        assert!(registry.acquire().is_ok());
    }
}
