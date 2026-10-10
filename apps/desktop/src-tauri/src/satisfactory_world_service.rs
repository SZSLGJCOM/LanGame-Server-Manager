use app_core::satisfactory_world::*;
use serde_json::{Value, json};

use super::{
    api::{Api, ApiError},
    protocol,
};

async fn stored_authorization(api: &Api) -> Result<Option<String>, String> {
    let token = api.read_credential("api-token").await?;
    if let Some(token) = token {
        let fingerprint = protocol::application_token_fingerprint(&token)?;
        api.call(
            "VerifyAuthenticationToken",
            json!({"AuthenticationToken": fingerprint, "PrivilegeLevel": "APIToken"}),
            Some(&token),
            false,
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(Some(token));
    }
    if api.endpoint.insecure_local_access {
        return Ok(None);
    }
    Err(ApiError::Authentication.to_string())
}

pub(super) async fn read(api: &Api) -> Result<SatisfactoryWorldSnapshot, String> {
    let server_name = Some(api.server_name().await?);
    let token = api.read_credential("api-token").await?;
    if token.is_none() {
        match api
            .call(
                "PasswordlessLogin",
                json!({"MinimumPrivilegeLevel": "InitialAdmin"}),
                None,
                false,
            )
            .await
        {
            Ok(_) => {
                return protocol::empty_snapshot(
                    &api.endpoint.instance_id,
                    SatisfactoryConnectionStatus::Unclaimed,
                    server_name,
                );
            }
            Err(ApiError::Rejected(code)) if code == "passwordless_login_not_possible" => {}
            Err(ApiError::Authentication) => {}
            Err(error) => return Err(error.to_string()),
        }
        if !api.endpoint.insecure_local_access {
            return protocol::empty_snapshot(
                &api.endpoint.instance_id,
                SatisfactoryConnectionStatus::AuthorizationRequired,
                server_name,
            );
        }
    }
    match snapshot(api, token.as_deref(), server_name.clone()).await {
        Ok(snapshot) => Ok(snapshot),
        Err(ApiError::Authentication) => protocol::empty_snapshot(
            &api.endpoint.instance_id,
            SatisfactoryConnectionStatus::AuthorizationRequired,
            server_name,
        ),
        Err(error) => Err(error.to_string()),
    }
}

async fn snapshot(
    api: &Api,
    token: Option<&str>,
    server_name: Option<String>,
) -> Result<SatisfactoryWorldSnapshot, ApiError> {
    let state = api
        .call("QueryServerState", json!({}), token, false)
        .await?
        .data;
    let state: protocol::ServerState = serde_json::from_value(
        state
            .get("serverGameState")
            .cloned()
            .ok_or(ApiError::InvalidResponse)?,
    )
    .map_err(|_| ApiError::InvalidResponse)?;
    let options: protocol::ServerOptions = serde_json::from_value(
        api.call("GetServerOptions", json!({}), token, false)
            .await?
            .data,
    )
    .map_err(|_| ApiError::InvalidResponse)?;
    let advanced: protocol::AdvancedSettings = serde_json::from_value(
        api.call("GetAdvancedGameSettings", json!({}), token, false)
            .await?
            .data,
    )
    .map_err(|_| ApiError::InvalidResponse)?;
    let sessions = protocol::sessions(
        api.call("EnumerateSessions", json!({}), token, false)
            .await?
            .data,
    )
    .map_err(|_| ApiError::InvalidResponse)?;
    let mut snapshot = protocol::empty_snapshot(
        &api.endpoint.instance_id,
        SatisfactoryConnectionStatus::Ready,
        server_name,
    )
    .map_err(|_| ApiError::InvalidResponse)?;
    snapshot.active_session_name = state.active_session_name;
    snapshot.auto_load_session_name = state.auto_load_session_name;
    snapshot.is_game_running = state.is_game_running;
    snapshot.connected_players = state.num_connected_players;
    snapshot.creative_mode_enabled = advanced.creative_mode_enabled;
    snapshot.advanced_game_settings = advanced.advanced_game_settings;
    snapshot.server_options = options.server_options;
    snapshot.pending_server_options = options.pending_server_options;
    snapshot.sessions = sessions;
    snapshot.revision = protocol::revision(&snapshot).map_err(|_| ApiError::InvalidResponse)?;
    Ok(snapshot)
}

async fn current(
    api: &Api,
    token: Option<&str>,
    expected: &str,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let before = snapshot(api, token, Some(api.server_name().await?))
        .await
        .map_err(|error| error.to_string())?;
    protocol::require_revision(&before, expected)?;
    Ok(before)
}

fn response_token(data: Value) -> Result<String, String> {
    data.get("authenticationToken")
        .and_then(Value::as_str)
        .filter(|token| {
            !token.is_empty() && token.len() <= 2048 && !token.chars().any(char::is_whitespace)
        })
        .map(str::to_owned)
        .ok_or_else(|| "The Satisfactory server did not return a valid authorization token.".into())
}

async fn permanent_token(api: &Api, token: Option<&str>) -> Result<String, String> {
    let data = api
        .call(
            "RunCommand",
            json!({"Command": "server.GenerateAPIToken"}),
            token,
            true,
        )
        .await
        .map_err(|error| error.to_string())?
        .data;
    let output = data
        .get("commandResult")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "The Satisfactory token command did not return its native output.".to_string()
        })?;
    let application = protocol::parse_api_token(output)?;
    let fingerprint = protocol::application_token_fingerprint(&application)?;
    api.call(
        "VerifyAuthenticationToken",
        json!({"AuthenticationToken": fingerprint, "PrivilegeLevel": "APIToken"}),
        Some(&application),
        false,
    )
    .await
    .map_err(|error| error.to_string())?;
    api.store_credential("api-token", application.clone())
        .await?;
    Ok(application)
}

async fn read_after_mutation(
    api: &Api,
    token: Option<&str>,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let name = api
        .server_name()
        .await
        .map_err(|_| ApiError::OutcomeUnknown.to_string())?;
    snapshot(api, token, Some(name))
        .await
        .map_err(|_| ApiError::OutcomeUnknown.to_string())
}

pub(super) async fn setup(
    api: &Api,
    input: SetupSatisfactoryServerInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    protocol::validate_text(&input.server_name, false)?;
    let initial = response_token(
        api.call(
            "PasswordlessLogin",
            json!({"MinimumPrivilegeLevel": "InitialAdmin"}),
            None,
            false,
        )
        .await
        .map_err(|error| error.to_string())?
        .data,
    )?;
    let password = match input.admin_password {
        Some(password) => password,
        None => api
            .read_credential("admin-password")
            .await?
            .unwrap_or_else(|| {
                format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                )
            }),
    };
    protocol::validate_text(&password, false)?;
    // Claim can succeed before its response is lost. Store the exact password
    // first so an explicit reconnect can recover without guessing or reclaiming.
    api.store_credential("admin-password", password.clone())
        .await?;
    let claim = api
        .call(
            "ClaimServer",
            json!({"ServerName": input.server_name, "AdminPassword": password}),
            Some(&initial),
            true,
        )
        .await
        .map_err(|error| error.to_string())?;
    let admin = response_token(claim.data).map_err(|error| format!(
        "The claim request was accepted and the administrator password is stored, but its authorization token could not be read. Reconnect to recover. {error}"
    ))?;
    let token = permanent_token(api, Some(&admin)).await.map_err(|error| format!(
        "The server was claimed and its administrator password is stored. Reconnect to complete management authorization. {error}"
    ))?;
    read_after_mutation(api, Some(&token)).await.map_err(|error| format!(
        "The server was claimed and management authorization is stored. Refresh to read its current state. {error}"
    ))
}

pub(super) async fn authorize(
    api: &Api,
    input: AuthorizeSatisfactoryServerInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let password = match input.admin_password {
        Some(password) => Some(password),
        None => api.read_credential("admin-password").await?,
    };
    let admin = if let Some(password) = password {
        protocol::validate_text(&password, false)?;
        let token = response_token(
            api.call(
                "PasswordLogin",
                json!({"MinimumPrivilegeLevel": "Administrator", "Password": password}),
                None,
                false,
            )
            .await
            .map_err(|error| error.to_string())?
            .data,
        )?;
        api.store_credential("admin-password", password).await?;
        Some(token)
    } else if api.endpoint.insecure_local_access {
        None
    } else {
        return Err(ApiError::Authentication.to_string());
    };
    let token = permanent_token(api, admin.as_deref()).await?;
    read_after_mutation(api, Some(&token))
        .await
        .map_err(|error| {
            format!("Management authorization is stored. Refresh to read the server state. {error}")
        })
}

async fn save_before_change(
    api: &Api,
    token: Option<&str>,
    before: &SatisfactoryWorldSnapshot,
) -> Result<(), String> {
    if !before.is_game_running {
        return Ok(());
    }
    save_world_snapshot(api, token, &before.active_session_name, "LGSM_before_world_change").await
        .map_err(|error| format!("Saving the current world could not be confirmed. No world change was requested. {error}"))
}

async fn save_world_snapshot(
    api: &Api,
    token: Option<&str>,
    session_name: &str,
    prefix: &str,
) -> Result<(), String> {
    let name = format!("{prefix}_{}", uuid::Uuid::new_v4().simple());
    api.call("SaveGame", json!({"SaveName": name}), token, true)
        .await
        .map_err(|error| error.to_string())?;
    let sessions = protocol::sessions(
        api.call("EnumerateSessions", json!({}), token, false)
            .await
            .map_err(|error| error.to_string())?
            .data,
    )?;
    if !sessions.iter().any(|session| {
        session.session_name == session_name
            && session.saves.iter().any(|save| save.save_name == name)
    }) {
        return Err("The Satisfactory world snapshot could not be verified.".into());
    }
    Ok(())
}

pub(super) async fn write_rules(
    api: &Api,
    input: WriteSatisfactoryWorldRulesInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let values = protocol::validate_rules(&input.advanced_game_settings, false, false)?;
    let token = stored_authorization(api).await?;
    let before = current(api, token.as_deref(), &input.expected_revision).await?;
    if !before.is_game_running {
        return Err("Load a Satisfactory world before changing its rules.".into());
    }
    if values.is_empty()
        || values
            .iter()
            .all(|(key, value)| before.advanced_game_settings.get(key) == Some(value))
    {
        return Ok(before);
    }
    if !before.creative_mode_enabled && !input.acknowledge_enable_advanced_settings {
        return Err("Confirm enabling Advanced Game Settings permanently for this world.".into());
    }
    if values
        .keys()
        .any(|key| !before.advanced_game_settings.contains_key(key))
    {
        return Err(
            "The running Satisfactory build does not provide one of the selected world rules."
                .into(),
        );
    }
    save_before_change(api, token.as_deref(), &before).await?;
    current(api, token.as_deref(), &before.revision).await?;
    api.call(
        "ApplyAdvancedGameSettings",
        json!({"AppliedAdvancedGameSettings": values}),
        token.as_deref(),
        true,
    )
    .await
    .map_err(|error| error.to_string())?;
    save_world_snapshot(api, token.as_deref(), &before.active_session_name, "LGSM_world_settings")
        .await.map_err(|error| format!("World rules were applied, but saving the updated world could not be confirmed. Refresh before retrying. {error}"))?;
    let after = read_after_mutation(api, token.as_deref()).await?;
    if after.active_session_name != before.active_session_name {
        return Err(ApiError::OutcomeUnknown.to_string());
    }
    if values
        .iter()
        .any(|(key, value)| after.advanced_game_settings.get(key) != Some(value))
    {
        return Err(
            "Satisfactory did not read back all requested world rules. Refresh before retrying."
                .into(),
        );
    }
    Ok(after)
}

#[cfg(test)]
#[path = "satisfactory_world_service_tests.rs"]
mod tests;

fn no_players(snapshot: &SatisfactoryWorldSnapshot) -> Result<(), String> {
    if snapshot.connected_players > 0 {
        return Err("Wait until all players disconnect before creating or loading a world.".into());
    }
    Ok(())
}

pub(super) async fn create(
    api: &Api,
    input: CreateSatisfactoryWorldInput,
) -> Result<SatisfactoryWorldOperationResult, String> {
    protocol::validate_session_name(&input.session_name)?;
    if !input.skip_onboarding {
        return Err("This dedicated-server workflow skips onboarding.".into());
    }
    if !protocol::catalog()?
        .starting_locations
        .iter()
        .any(|option| option.value == input.starting_location)
    {
        return Err("The Satisfactory starting location is unsupported.".into());
    }
    let game_mode = protocol::validate_rules(&input.game_mode_settings, true, true)?;
    let mut advanced = protocol::validate_rules(&input.advanced_game_settings, true, false)?;
    let catalog = protocol::catalog()?;
    advanced.retain(|key, value| {
        !catalog
            .settings
            .iter()
            .any(|rule| rule.key == *key && rule.default_value == *value)
    });
    if !advanced.is_empty() && !input.acknowledge_enable_advanced_settings {
        return Err(
            "Confirm enabling Advanced Game Settings permanently for the new world.".into(),
        );
    }
    let token = stored_authorization(api).await?;
    let before = current(api, token.as_deref(), &input.expected_revision).await?;
    no_players(&before)?;
    if before.sessions.iter().any(|session| {
        session
            .session_name
            .eq_ignore_ascii_case(&input.session_name)
    }) || (before.is_game_running
        && before
            .active_session_name
            .eq_ignore_ascii_case(&input.session_name))
    {
        return Err("A Satisfactory world already uses that name. Choose a new name.".into());
    }
    save_before_change(api, token.as_deref(), &before).await?;
    let fresh = current(api, token.as_deref(), &before.revision).await?;
    no_players(&fresh)?;
    if fresh.sessions.iter().any(|session| {
        session
            .session_name
            .eq_ignore_ascii_case(&input.session_name)
    }) {
        return Err("A Satisfactory world was created with that name before this request. Refresh and choose a new name.".into());
    }
    api.call("CreateNewGame", json!({"NewGameData": {
        "SessionName": input.session_name, "MapName": "", "StartingLocation": input.starting_location,
        "bSkipOnboarding": input.skip_onboarding, "GameModeSettings": game_mode,
        "AdvancedGameSettings": advanced, "CustomOptionsOnlyForModding": {}
    }}), token.as_deref(), true).await.map_err(|error| error.to_string())?;
    Ok(SatisfactoryWorldOperationResult {
        instance_id: input.instance_id,
        accepted: true,
        session_name: input.session_name,
    })
}

pub(super) async fn load(
    api: &Api,
    input: LoadSatisfactorySaveInput,
) -> Result<SatisfactoryWorldOperationResult, String> {
    let token = stored_authorization(api).await?;
    let before = current(api, token.as_deref(), &input.expected_revision).await?;
    no_players(&before)?;
    let session = before
        .sessions
        .iter()
        .find(|session| {
            session
                .saves
                .iter()
                .any(|save| save.save_name == input.save_name)
        })
        .ok_or_else(|| {
            "The selected Satisfactory save is no longer in the native save collection.".to_string()
        })?;
    let session_name = session.session_name.clone();
    save_before_change(api, token.as_deref(), &before).await?;
    let fresh = current(api, token.as_deref(), &before.revision).await?;
    no_players(&fresh)?;
    if !fresh.sessions.iter().any(|session| {
        session.session_name == session_name
            && session
                .saves
                .iter()
                .any(|save| save.save_name == input.save_name)
    }) {
        return Err("The selected Satisfactory save disappeared before loading. Refresh the save collection.".into());
    }
    api.call(
        "LoadGame",
        json!({"SaveName": input.save_name, "EnableAdvancedGameSettings": false}),
        token.as_deref(),
        true,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(SatisfactoryWorldOperationResult {
        instance_id: input.instance_id,
        accepted: true,
        session_name,
    })
}

pub(super) async fn room(
    api: &Api,
    input: WriteSatisfactoryRoomInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    if let Some(name) = &input.server_name {
        protocol::validate_text(name, false)?;
    }
    if let Some(password) = &input.client_password {
        protocol::validate_text(password, true)?;
    }
    let token = stored_authorization(api).await?;
    let before = current(api, token.as_deref(), &input.expected_revision).await?;
    if input.auto_load_session_name.as_ref().is_some_and(|name| {
        !name.is_empty()
            && !before
                .sessions
                .iter()
                .any(|session| session.session_name == *name)
    }) {
        return Err(
            "The selected startup world is no longer in the native save collection.".into(),
        );
    }
    // These are separate native operations, with no atomic transaction. Once
    // one succeeds, a later rejection must report partial success explicitly.
    let mut changes = Vec::new();
    if let Some(name) = &input.server_name {
        changes.push(("RenameServer", json!({"ServerName": name})));
    }
    if let Some(password) = &input.client_password {
        changes.push(("SetClientPassword", json!({"Password": password})));
    }
    if let Some(name) = &input.auto_load_session_name {
        changes.push(("SetAutoLoadSessionName", json!({"SessionName": name})));
    }
    for (applied, (function, data)) in changes.into_iter().enumerate() {
        if let Err(error) = api.call(function, data, token.as_deref(), true).await {
            return Err(if applied > 0 {
                format!(
                    "Some Satisfactory room changes were applied. Refresh before retrying. {error}"
                )
            } else {
                error.to_string()
            });
        }
    }
    let after = read_after_mutation(api, token.as_deref()).await?;
    if input
        .server_name
        .as_ref()
        .is_some_and(|name| after.server_name.as_ref() != Some(name))
        || input
            .auto_load_session_name
            .as_ref()
            .is_some_and(|name| after.auto_load_session_name != *name)
    {
        return Err(
            "Satisfactory did not read back the requested room changes. Refresh before retrying."
                .into(),
        );
    }
    Ok(after)
}
