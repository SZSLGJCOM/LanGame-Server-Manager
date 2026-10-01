use std::collections::{BTreeMap, btree_map::Entry};
use std::future::Future;
use std::time::Duration;

use app_storage::StoragePaths;
use tokio::sync::OwnedMutexGuard;
use tokio::time::{Instant, timeout_at};

use super::DesktopState;

const ACQUISITION_TIMEOUT: Duration = Duration::from_secs(30);
const TIMEOUT_MESSAGE: &str =
    "等待实例变更锁超时：其他操作或持续创建实例使实例集合无法稳定，请稍后重试程序维护";

pub(super) async fn acquire_module_instance_mutations(
    state: &DesktopState,
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Vec<OwnedMutexGuard<()>>, String> {
    let access = ModuleInstances {
        state,
        paths,
        module_id,
    };
    acquire_stable_instance_locks(&access, ACQUISITION_TIMEOUT)
        .await
        .map_err(|error| format!("无法锁定模块“{module_id}”的实例：{error}"))
}

struct ModuleInstances<'a> {
    state: &'a DesktopState,
    paths: &'a StoragePaths,
    module_id: &'a str,
}

// Keep enumeration and locking at one boundary so the scheduling regression
// controls the same acquisition algorithm used by real desktop operations.
trait InstanceMutationAccess: Sync {
    fn instance_ids(&self) -> impl Future<Output = Result<Vec<String>, String>> + Send;
    fn acquire(&self, instance_id: &str) -> impl Future<Output = OwnedMutexGuard<()>> + Send;
}

impl InstanceMutationAccess for ModuleInstances<'_> {
    async fn instance_ids(&self) -> Result<Vec<String>, String> {
        Ok(app_storage::list_instances(self.paths)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|instance| instance.module_id == self.module_id)
            .map(|instance| instance.id)
            .collect())
    }

    async fn acquire(&self, instance_id: &str) -> OwnedMutexGuard<()> {
        self.state.acquire_instance_mutation(instance_id).await
    }
}

async fn acquire_stable_instance_locks(
    access: &impl InstanceMutationAccess,
    timeout: Duration,
) -> Result<Vec<OwnedMutexGuard<()>>, String> {
    let deadline = Instant::now() + timeout;
    timeout_at(deadline, async {
        let mut held = BTreeMap::<String, OwnedMutexGuard<()>>::new();
        loop {
            // Also bound an immediately-ready provider under continuous churn;
            // a timeout future alone cannot interrupt a loop that never yields.
            check_deadline(deadline)?;
            let mut ids = access.instance_ids().await?;
            ids.sort();
            ids.dedup();
            held.retain(|id, _| ids.binary_search(id).is_ok());
            let missing = ids
                .iter()
                .filter(|id| !held.contains_key(*id))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                return Ok(held.into_values().collect());
            }
            if held
                .last_key_value()
                .is_some_and(|(last, _)| missing[0] < last)
            {
                // A newly created lower ID would invert the order against a
                // second uninstall. Release *all* guards before reacquiring.
                held.clear();
            }
            for id in ids {
                check_deadline(deadline)?;
                if let Entry::Vacant(entry) = held.entry(id.clone()) {
                    let guard = access.acquire(&id).await;
                    entry.insert(guard);
                }
            }
            // Creation can complete during acquisition. Confirm the current
            // set again while holding the acquired locks before returning.
        }
    })
    .await
    .map_err(|_| TIMEOUT_MESSAGE.to_owned())?
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err(TIMEOUT_MESSAGE.to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "commands_module_mutation_locks_tests.rs"]
mod tests;
