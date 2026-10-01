use std::collections::HashSet;
use std::sync::Mutex;

use app_core::{InstanceStatus, InstanceSummary};
use app_storage::StorageBootstrap;

#[derive(Default)]
pub struct AutostartQueue {
    inner: Mutex<QueueState>,
}

#[derive(Default)]
struct QueueState {
    modules_ready: bool,
    captured: bool,
    batch: Option<AutostartBatch>,
    eligible: HashSet<String>,
}

pub struct AutostartBatch {
    pub storage: StorageBootstrap,
    pub instances: Vec<InstanceSummary>,
}

impl AutostartQueue {
    pub fn mark_modules_ready(&self) -> Result<(), String> {
        self.inner.lock().map_err(|_| queue_error())?.modules_ready = true;
        Ok(())
    }

    /// Capture only the first reconciled list after initialization. Reloading the
    /// webview or changing a setting must never start a second batch.
    pub fn capture(
        &self,
        storage: &StorageBootstrap,
        instances: &[InstanceSummary],
    ) -> Result<(), String> {
        let mut queue = self.inner.lock().map_err(|_| queue_error())?;
        if !queue.modules_ready || queue.captured {
            return Ok(());
        }
        let instances: Vec<_> = instances
            .iter()
            .filter(|instance| {
                instance.autostart
                    && matches!(
                        instance.status,
                        InstanceStatus::Stopped | InstanceStatus::Error
                    )
            })
            .cloned()
            .collect();
        queue.captured = true;
        queue.eligible = instances
            .iter()
            .map(|instance| instance.id.clone())
            .collect();
        if !instances.is_empty() {
            queue.batch = Some(AutostartBatch {
                storage: storage.clone(),
                instances,
            });
        }
        Ok(())
    }

    pub fn take(&self) -> Result<Option<AutostartBatch>, String> {
        Ok(self.inner.lock().map_err(|_| queue_error())?.batch.take())
    }

    pub fn is_eligible(&self, instance_id: &str) -> Result<bool, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| queue_error())?
            .eligible
            .contains(instance_id))
    }

    /// A manual operation or disabling autostart takes precedence over a queued start.
    pub fn cancel(&self, instance_id: &str) -> Result<(), String> {
        self.inner
            .lock()
            .map_err(|_| queue_error())?
            .eligible
            .remove(instance_id);
        Ok(())
    }

    pub fn cancel_all(&self) -> Result<(), String> {
        let mut queue = self.inner.lock().map_err(|_| queue_error())?;
        queue.captured = true;
        queue.batch = None;
        queue.eligible.clear();
        Ok(())
    }
}

fn queue_error() -> String {
    String::from("instance autostart queue lock poisoned")
}
