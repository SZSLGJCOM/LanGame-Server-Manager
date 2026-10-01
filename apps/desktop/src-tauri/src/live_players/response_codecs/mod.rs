use std::collections::{HashMap, HashSet};

use app_core::{
    ModulePlayerListCodec, RuntimeLivePlayerAttribute, RuntimeLivePlayerEntry,
    RuntimeLivePlayerIdentifier, RuntimePlayerIdentityKind,
};

mod ark;
mod conan;
pub(crate) mod humanitz;
mod rust;
mod seven_days;
mod squad;
mod text_lists;

#[cfg(test)]
mod seven_days_tests;
#[cfg(test)]
mod tests;

const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_RESPONSE_ROWS: usize = 4096;
const MAX_VISIBLE_ROWS: usize = 256;
const MAX_FIELD_CHARS: usize = 256;

#[derive(Debug)]
pub(crate) struct ParsedPlayerList {
    pub entries: Vec<RuntimeLivePlayerEntry>,
    pub bindings: HashMap<(String, String), String>,
    pub complete: bool,
    pub truncated: bool,
    pub current_players: Option<usize>,
    pub max_players: Option<usize>,
}

pub(crate) fn parse(
    codec: ModulePlayerListCodec,
    text: &str,
    request_id: &str,
    now: u64,
    action_ids: &[String],
) -> Result<ParsedPlayerList, String> {
    if text.len() > MAX_RESPONSE_BYTES || text.contains('\0') {
        return Err(String::from(
            "Player response exceeds the capture limit or contains NUL bytes.",
        ));
    }
    let text = text.trim_matches(['\r', '\n']);
    if text.trim().is_empty() {
        return Err(String::from(
            "The server did not return a player-list response.",
        ));
    }
    let mut output = ListBuilder::new(request_id, action_ids);
    match codec {
        ModulePlayerListCodec::RustPlayerList => rust::parse(text, now, &mut output)?,
        ModulePlayerListCodec::ArkListPlayers => ark::parse(text, &mut output)?,
        ModulePlayerListCodec::ConanListPlayers => conan::parse(text, &mut output)?,
        ModulePlayerListCodec::HumanitzPlayers => humanitz::parse(text, &mut output)?,
        ModulePlayerListCodec::SquadListPlayers => squad::parse(text, &mut output)?,
        ModulePlayerListCodec::ZomboidPlayers => text_lists::zomboid(text, &mut output)?,
        ModulePlayerListCodec::SevenDaysPlayers => seven_days::parse(text, &mut output)?,
        _ => {
            return Err(String::from(
                "This response codec has no direct-response parser.",
            ));
        }
    }
    Ok(output.finish())
}

struct ListBuilder<'a> {
    request_id: &'a str,
    action_ids: &'a [String],
    entries: Vec<RuntimeLivePlayerEntry>,
    bindings: HashMap<(String, String), String>,
    identities: HashSet<String>,
    total: usize,
}

impl<'a> ListBuilder<'a> {
    fn new(request_id: &'a str, action_ids: &'a [String]) -> Self {
        Self {
            request_id,
            action_ids,
            entries: Vec::new(),
            bindings: HashMap::new(),
            identities: HashSet::new(),
            total: 0,
        }
    }

    fn push(
        &mut self,
        mut entry: RuntimeLivePlayerEntry,
        targets: &[(&str, &str)],
    ) -> Result<(), String> {
        self.total += 1;
        if self.total > MAX_RESPONSE_ROWS {
            return Err(String::from("Player response exceeds the row limit."));
        }
        for identifier in &entry.identifiers {
            let key = format!("{:?}:{}", identifier.kind, identifier.value);
            if !self.identities.insert(key) {
                return Err(String::from(
                    "Player response contains a duplicate identity.",
                ));
            }
        }
        if self.entries.len() == MAX_VISIBLE_ROWS {
            return Ok(());
        }
        entry.player_key = format!("response:{}:{}", self.request_id, self.total);
        for (action_id, target) in targets {
            if self
                .action_ids
                .iter()
                .any(|declared| declared.as_str() == *action_id)
            {
                entry.available_action_ids.push((*action_id).to_owned());
                self.bindings.insert(
                    (entry.player_key.clone(), (*action_id).to_owned()),
                    (*target).to_owned(),
                );
            }
        }
        self.entries.push(entry);
        Ok(())
    }

    fn finish(mut self) -> ParsedPlayerList {
        let truncated = self.total > self.entries.len();
        if truncated {
            self.bindings.clear();
            for entry in &mut self.entries {
                entry.available_action_ids.clear();
            }
        }
        ParsedPlayerList {
            entries: self.entries,
            bindings: self.bindings,
            complete: !truncated,
            truncated,
            current_players: Some(self.total),
            max_players: None,
        }
    }
}

fn player(
    name: &str,
    identifiers: Vec<RuntimeLivePlayerIdentifier>,
) -> Result<RuntimeLivePlayerEntry, String> {
    validate_field(name)?;
    Ok(RuntimeLivePlayerEntry {
        player_key: String::new(),
        display_name: name.to_owned(),
        identifiers,
        available_action_ids: Vec::new(),
        ping_ms: None,
        session_started_at_unix_ms: None,
        role: None,
        attributes: Vec::new(),
    })
}

fn validate_field(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.chars().count() > MAX_FIELD_CHARS
        || value.chars().any(char::is_control)
    {
        return Err(String::from("Player response contains an invalid field."));
    }
    Ok(())
}

fn identifier(
    kind: RuntimePlayerIdentityKind,
    value: &str,
    stable: bool,
) -> RuntimeLivePlayerIdentifier {
    RuntimeLivePlayerIdentifier {
        kind,
        value: value.to_owned(),
        stable,
    }
}

fn decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn steam_id(value: &str) -> bool {
    value.len() == 17 && decimal(value)
}

fn eos_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn attribute(key: &str, value: &str) -> RuntimeLivePlayerAttribute {
    RuntimeLivePlayerAttribute {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}
