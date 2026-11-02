use super::RuntimeStartReservationLease;
use app_storage::managed_console_log::ManagedConsoleLog;
use app_storage::startup_sessions::StartupSessionReceipt;
use std::path::PathBuf;

pub(super) struct OwnedStartupSession {
    root: PathBuf,
    receipt: StartupSessionReceipt,
    writer: Option<ManagedConsoleLog>,
}

impl RuntimeStartReservationLease {
    pub(crate) fn startup_identity(&self) -> (&str, u128) {
        (&self.lease.attempt_id, self.lease.reserved_at_unix_ms)
    }

    pub(crate) fn own_startup_session(
        &self,
        root: PathBuf,
        receipt: StartupSessionReceipt,
        writer: ManagedConsoleLog,
    ) -> Result<(), String> {
        let mut owned = self
            .lease
            .startup_session
            .lock()
            .map_err(|_| "Startup session ownership lock poisoned")?;
        if owned.is_some() {
            return Err("Startup session was already created".into());
        }
        *owned = Some(OwnedStartupSession {
            root,
            receipt,
            writer: Some(writer),
        });
        Ok(())
    }

    pub(crate) fn startup_session(
        &self,
    ) -> Result<Option<(PathBuf, StartupSessionReceipt)>, String> {
        self.lease
            .startup_session
            .lock()
            .map(|owned| {
                owned
                    .as_ref()
                    .map(|session| (session.root.clone(), session.receipt.clone()))
            })
            .map_err(|_| "Startup session ownership lock poisoned".into())
    }

    pub(crate) fn close_startup_output(&self) -> Result<(), String> {
        let mut owned = self
            .lease
            .startup_session
            .lock()
            .map_err(|_| "Startup session ownership lock poisoned")?;
        if let Some(session) = owned.as_mut() {
            session.writer.take();
        }
        Ok(())
    }
}
