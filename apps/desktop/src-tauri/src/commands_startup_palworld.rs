use super::startup_probe::{StartupProcessTarget, poll_owned_tcp};
use crate::live_players::palworld_rest;
use crate::runtime_palworld_health::{self, Observation};
use app_core::InstanceDetails;

// The credential and opaque world identity stay inside this one startup.
pub(super) struct PalworldReadiness {
    endpoint: palworld_rest::PalworldEndpoint,
    previous: Option<Observation>,
}

impl PalworldReadiness {
    pub(super) fn prepare(instance: &InstanceDetails) -> Result<Option<Self>, String> {
        let settings: serde_json::Value = serde_json::from_str(&instance.settings_json)
            .map_err(|_| "Palworld startup settings are invalid")?;
        if settings
            .get("rest_api_enabled")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            return Ok(None);
        }
        let endpoint =
            palworld_rest::resolve_endpoint(instance).map_err(|error| error.summary.to_owned())?;
        Ok(Some(Self {
            endpoint,
            previous: None,
        }))
    }

    pub(super) fn poll(&mut self, target: &StartupProcessTarget) -> Result<bool, String> {
        let port = self.endpoint.address.port();
        let mut observation = None;
        let result = poll_owned_tcp(target, port, || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "Cannot initialize the Palworld startup query runtime")?;
            observation = Some(runtime.block_on(runtime_palworld_health::query_world(
                port,
                &self.endpoint.password,
            )));
            Ok(true)
        });
        match result {
            Ok(true) => {
                // Only retain samples after the post-query ownership check.
                // An endpoint disappearing during I/O cannot supply the first
                // of the two same-world observations required on a later poll.
                let observed = observation
                    .ok_or("Palworld startup query completed without an observation")?
                    .with_previous(self.previous.as_ref());
                let ready = observed.is_ready();
                self.previous = Some(observed);
                Ok(ready)
            }
            result => {
                self.previous = None;
                result
            }
        }
    }
}
