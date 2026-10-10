use std::collections::{BTreeMap, BTreeSet};

use app_core::satisfactory_world::{
    SatisfactoryConnectionStatus, SatisfactoryRuleDefinition, SatisfactoryRuleOption,
    SatisfactorySave, SatisfactorySession, SatisfactoryWorldSnapshot,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
pub(super) struct Catalog {
    pub(super) settings: Vec<SatisfactoryRuleDefinition>,
    pub(super) starting_locations: Vec<SatisfactoryRuleOption>,
}

pub(super) fn catalog() -> Result<Catalog, String> {
    serde_json::from_str(include_str!(
        "../../../../modules/satisfactory/world-settings.json"
    ))
    .map_err(|_| "The Satisfactory native settings catalog could not be read.".into())
}

pub(super) fn validate_rules(
    values: &BTreeMap<String, String>,
    creation: bool,
    game_mode: bool,
) -> Result<BTreeMap<String, String>, String> {
    let catalog = catalog()?;
    if values.len() > 32
        || values
            .iter()
            .any(|(key, value)| key.len() > 128 || value.len() > 64)
    {
        return Err("The Satisfactory settings patch exceeds its supported limits.".into());
    }
    let mut result = BTreeMap::new();
    for (key, value) in values {
        let rule = catalog
            .settings
            .iter()
            .find(|rule| rule.key == *key)
            .filter(|rule| {
                (creation || rule.scope != "creation")
                    && key.starts_with("FG.GameMode.") == game_mode
            })
            .ok_or_else(|| format!("The setting {key} is not supported by this operation."))?;
        let value = match rule.kind.as_str() {
            "boolean" if value.eq_ignore_ascii_case("true") => "True".into(),
            "boolean" if value.eq_ignore_ascii_case("false") => "False".into(),
            "select" if rule.options.iter().any(|option| option.value == *value) => value.clone(),
            "integer" => {
                let integer = value
                    .parse::<i64>()
                    .ok()
                    .filter(|integer| integer.to_string() == *value)
                    .filter(|integer| rule.minimum.is_none_or(|minimum| *integer >= minimum))
                    .filter(|integer| rule.maximum.is_none_or(|maximum| *integer <= maximum))
                    .ok_or_else(|| format!("The setting {key} has an invalid integer value."))?;
                integer.to_string()
            }
            _ => return Err(format!("The setting {key} has an unsupported value.")),
        };
        result.insert(key.clone(), value);
    }
    Ok(result)
}

pub(super) fn validate_text(value: &str, allow_empty: bool) -> Result<(), String> {
    if value.chars().count() > 128
        || value.chars().any(char::is_control)
        || (!allow_empty && value.trim().is_empty())
    {
        return Err(
            "A name or password is empty, too long, or contains control characters.".into(),
        );
    }
    Ok(())
}

pub(super) fn validate_session_name(value: &str) -> Result<(), String> {
    validate_text(value, false)?;
    // Native NewGameData becomes a map URL. Do not permit URL-option injection.
    if value.trim() != value || value.contains(['?', '#', '/', '\\', ':', '*', '"', '<', '>', '|'])
    {
        return Err(
            "The world name contains characters that cannot be used in a saved session.".into(),
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ServerState {
    pub(super) active_session_name: String,
    pub(super) auto_load_session_name: String,
    pub(super) is_game_running: bool,
    pub(super) num_connected_players: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AdvancedSettings {
    pub(super) creative_mode_enabled: bool,
    pub(super) advanced_game_settings: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ServerOptions {
    pub(super) server_options: BTreeMap<String, String>,
    pub(super) pending_server_options: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sessions {
    sessions: Vec<Session>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    session_name: String,
    save_headers: Vec<Save>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Save {
    save_name: String,
    save_date_time: String,
    play_duration_seconds: i64,
    is_creative_mode_enabled: bool,
}

pub(super) fn sessions(data: Value) -> Result<Vec<SatisfactorySession>, String> {
    let sessions: Sessions = serde_json::from_value(data)
        .map_err(|_| "The Satisfactory save collection response is incomplete.".to_string())?;
    let mut names = BTreeSet::new();
    let mut saves = BTreeSet::new();
    let mut result = Vec::new();
    if sessions.sessions.len() > 256 {
        return Err("The Satisfactory save collection exceeds the session limit.".into());
    }
    for session in sessions.sessions {
        validate_text(&session.session_name, false)?;
        if !names.insert(session.session_name.clone()) || session.save_headers.len() > 1024 {
            return Err(
                "The Satisfactory save collection contains duplicate or excessive entries.".into(),
            );
        }
        let mut headers = Vec::new();
        for save in session.save_headers {
            validate_text(&save.save_name, false)?;
            if save.save_name.contains(['/', '\\'])
                || save.play_duration_seconds < 0
                || save.save_date_time.len() > 64
                || !saves.insert(save.save_name.clone())
                || saves.len() > 4096
            {
                return Err(
                    "The Satisfactory save collection contains an invalid save header.".into(),
                );
            }
            headers.push(SatisfactorySave {
                save_name: save.save_name,
                save_date_time: save.save_date_time,
                play_duration_seconds: save.play_duration_seconds,
                is_creative_mode_enabled: save.is_creative_mode_enabled,
            });
        }
        result.push(SatisfactorySession {
            session_name: session.session_name,
            saves: headers,
        });
    }
    Ok(result)
}

pub(super) fn empty_snapshot(
    instance_id: &str,
    status: SatisfactoryConnectionStatus,
    server_name: Option<String>,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let catalog = catalog()?;
    Ok(SatisfactoryWorldSnapshot {
        instance_id: instance_id.into(),
        connection_status: status,
        revision: String::new(),
        server_name,
        active_session_name: String::new(),
        auto_load_session_name: String::new(),
        is_game_running: false,
        connected_players: 0,
        creative_mode_enabled: false,
        advanced_game_settings: BTreeMap::new(),
        server_options: BTreeMap::new(),
        pending_server_options: BTreeMap::new(),
        sessions: Vec::new(),
        rule_definitions: catalog.settings,
        starting_locations: catalog.starting_locations,
    })
}

pub(super) fn revision(snapshot: &SatisfactoryWorldSnapshot) -> Result<String, String> {
    // Saves and player counts change without a settings change. Bind edits to
    // the current world, native rules, name and startup selection instead.
    let bytes = serde_json::to_vec(&(
        &snapshot.instance_id,
        &snapshot.server_name,
        &snapshot.active_session_name,
        &snapshot.auto_load_session_name,
        snapshot.is_game_running,
        snapshot.creative_mode_enabled,
        &snapshot.advanced_game_settings,
    ))
    .map_err(|_| "The Satisfactory world revision could not be calculated.".to_string())?;
    Ok(sha256_hex(&bytes))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

pub(super) fn require_revision(
    snapshot: &SatisfactoryWorldSnapshot,
    expected: &str,
) -> Result<(), String> {
    if expected.is_empty() || snapshot.revision != expected {
        return Err("The Satisfactory world changed. Refresh before saving.".into());
    }
    Ok(())
}

pub(super) fn parse_api_token(output: &str) -> Result<String, String> {
    const PREFIX: &str = "New Server API Authentication Token: ";
    let mut matches = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix(PREFIX));
    let token = matches
        .next()
        .filter(|token| token.len() <= 2048 && !token.chars().any(char::is_whitespace))
        .ok_or_else(|| "The server did not return a supported application token.".to_string())?;
    if matches.next().is_some() {
        return Err("The server returned more than one application token.".into());
    }
    application_token_fingerprint(token)?;
    Ok(token.to_owned())
}

/// VerifyAuthenticationToken takes a fingerprint and privilege separately;
/// the HTTP Authorization header takes the complete encoded token.
pub(super) fn application_token_fingerprint(token: &str) -> Result<&str, String> {
    use base64::Engine;
    if token.len() > 2048 || token.chars().any(char::is_whitespace) {
        return Err("The server application token format is unsupported.".into());
    }
    let (payload, fingerprint) = token
        .split_once('.')
        .filter(|(_, fingerprint)| {
            fingerprint.len() == 128 && fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or_else(|| "The server application token format is unsupported.".to_string())?;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| "The server application token payload is malformed.".to_string())?;
    let payload: Value = serde_json::from_slice(&payload)
        .map_err(|_| "The server application token payload is malformed.".to_string())?;
    if payload.get("pl").and_then(Value::as_str) != Some("APIToken") {
        return Err("The server did not grant application-token privileges.".into());
    }
    Ok(fingerprint)
}

#[cfg(test)]
#[path = "satisfactory_world_protocol_tests.rs"]
mod tests;
