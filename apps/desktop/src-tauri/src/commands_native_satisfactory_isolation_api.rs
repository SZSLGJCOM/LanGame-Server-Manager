//! Owned HTTPS observations for the opt-in two-instance fixture only.
use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::super::{api, context, credentials};
use super::*;

#[derive(PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) session_name: String,
    pub(crate) manual_saves: BTreeSet<String>,
    pub(crate) credential_fingerprints: [String; 2],
    state: serde_json::Value,
}

impl NativeWorld {
    pub(crate) async fn isolation_snapshot(
        &self,
        state: &tauri::State<'_, DesktopState>,
        running: &InstanceDetails,
    ) -> Result<Snapshot, String> {
        let snapshot = self.read(state, running).await?;
        if !loaded_world(&snapshot, &self.instance_id, &self.session_name)
            || snapshot.sessions.len() != 1
            || snapshot.sessions[0].session_name != self.session_name
            || !snapshot
                .server_options
                .get("FG.DSAutoPause")
                .is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1")
        {
            return Err("Satisfactory isolation requires the unique owned no-client world and native auto-pause readback.".into());
        }
        // A save header's duration/date describe that fixed checkpoint, not
        // the live world's clock. Autosave headers and the changing snapshot
        // revision are excluded; a foreign world or manual checkpoint is not.
        let saves = snapshot.sessions[0]
            .saves
            .iter()
            .filter(|save| save.save_name.starts_with("LGSM_manual_save_"))
            .map(|save| {
                (
                    save.save_name.clone(),
                    serde_json::json!({
                        "date": save.save_date_time,
                        "duration": save.play_duration_seconds,
                        "creative": save.is_creative_mode_enabled,
                    }),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if saves.is_empty() {
            return Err(
                "Satisfactory isolation has no confirmed formal manual-save checkpoint.".into(),
            );
        }
        Ok(Snapshot {
            session_name: self.session_name.clone(),
            manual_saves: saves.keys().cloned().collect(),
            credential_fingerprints: self.isolation_credential_fingerprints().await?,
            state: serde_json::json!({
                "instance": snapshot.instance_id,
                "active_session": snapshot.active_session_name,
                "auto_load_session": snapshot.auto_load_session_name,
                "server_name": snapshot.server_name,
                "creative": snapshot.creative_mode_enabled,
                "rules": snapshot.advanced_game_settings,
                "manual_saves": saves,
            }),
        })
    }

    pub(crate) async fn isolation_credential_fingerprints(&self) -> Result<[String; 2], String> {
        let secrets = self.isolation_secrets().await?;
        Ok(secrets.map(|secret| {
            Sha256::digest(secret.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }))
    }

    async fn isolation_secrets(&self) -> Result<[String; 2], String> {
        let admin = credentials::read(&self.credential_identity, "admin-password")
            .await?
            .filter(|secret| !secret.is_empty())
            .ok_or("The owned Satisfactory isolation administrator credential is absent.")?;
        let token = credentials::read(&self.credential_identity, "api-token")
            .await?
            .filter(|secret| !secret.is_empty())
            .ok_or("The owned Satisfactory isolation API credential is absent.")?;
        Ok([admin, token])
    }

    pub(crate) async fn verify_isolated_credentials(
        &self,
        state: &tauri::State<'_, DesktopState>,
        running: &InstanceDetails,
        other: &Self,
        other_running: &InstanceDetails,
    ) -> Result<(), String> {
        self.current(state, running).await?;
        other.current(state, other_running).await?;
        if self.instance_id == other.instance_id
            || self.session_name == other.session_name
            || self.credential_identity == other.credential_identity
        {
            return Err(
                "Satisfactory isolation reused an instance, world or credential namespace.".into(),
            );
        }
        let own = self.isolation_secrets().await?;
        let peer = other.isolation_secrets().await?;
        if own[0] == peer[0] || own[1] == peer[1] {
            return Err(
                "Satisfactory isolation reused an administrator password or API token.".into(),
            );
        }
        // Keep both native endpoints owned across each read. Never print the
        // tokens or use a rejected request as evidence of a durable save.
        for (details, token) in [(running, &peer[1]), (other_running, &own[1])] {
            let endpoint = context::resolve(state, details)?
                .ok_or("Satisfactory isolation has no owned HTTPS endpoint.")?;
            let api = api::Api::new(endpoint)?;
            if !matches!(
                api.call(
                    "EnumerateSessions",
                    serde_json::json!({}),
                    Some(token),
                    false
                )
                .await,
                Err(api::ApiError::Authentication)
            ) {
                return Err(
                    "A Satisfactory server did not reject the other owned instance's API token."
                        .into(),
                );
            }
        }
        self.current(state, running).await?;
        other.current(state, other_running).await?;
        Ok(())
    }
}
