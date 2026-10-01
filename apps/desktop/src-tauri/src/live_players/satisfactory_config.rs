use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

const MAX_CONFIG_BYTES: u64 = 256 * 1024;
const SETTINGS_SECTION: &str = "/Script/FactoryGame.FGGameUserSettings";
const PORT_KEY: &str = "FicsitRemoteMonitoring.Server.uWS.Port";
const AUTOSTART_KEY: &str = "FicsitRemoteMonitoring.Server.uWS.Autostart";

pub(super) fn read_web_port(instance_config: &str) -> Result<Option<u16>, &'static str> {
    let root = Path::new(instance_config)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance configuration has no owning directory.")?;
    let path = root.join("data/Saved/Config/WindowsServer/GameUserSettings.ini");
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("The instance's FRM settings could not be read."),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The instance's FRM settings could not be read.")?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err("The instance's FRM settings exceed the configuration size limit.");
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "The instance's FRM settings are not valid UTF-8.")?;
    parse_web_port(text)
}

fn parse_web_port(text: &str) -> Result<Option<u16>, &'static str> {
    let mut in_settings = false;
    let mut port = None;
    let mut autostart = None;
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') && line.ends_with(']') {
            in_settings = &line[1..line.len() - 1] == SETTINGS_SECTION;
            continue;
        }
        if !in_settings || line.starts_with([';', '#']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(key.trim(), "mIntValues" | "mStringValues") {
            continue;
        }
        // Native settings are Unreal maps of scalar tuples. Parse tuple boundaries
        // so a key mentioned inside another option's string cannot select a port.
        for (key, value) in scalar_pairs(value)? {
            let target = match key {
                PORT_KEY => &mut port,
                AUTOSTART_KEY => &mut autostart,
                _ => continue,
            };
            if target.replace(value).is_some() {
                return Err("The instance's FRM settings contain duplicate endpoint options.");
            }
        }
    }
    match autostart {
        None | Some("0" | "False" | "false") => return Ok(None),
        Some("1" | "True" | "true") => {}
        _ => return Err("The instance's FRM HTTP autostart option is invalid."),
    }
    // FRM's native default is 8080 when no port override is persisted.
    let port = port.unwrap_or("8080");
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("The instance's FRM HTTP port is invalid.");
    }
    port.parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .map(Some)
        .ok_or("The instance's FRM HTTP port is invalid.")
}

fn scalar_pairs(mut input: &str) -> Result<Vec<(&str, &str)>, &'static str> {
    const INVALID: &str = "The instance's FRM settings contain a malformed native option map.";
    fn consume(input: &mut &str, prefix: char) -> Result<(), &'static str> {
        *input = input.trim_start().strip_prefix(prefix).ok_or(INVALID)?;
        Ok(())
    }
    fn quoted<'a>(input: &mut &'a str) -> Result<&'a str, &'static str> {
        consume(input, '"')?;
        let mut escaped = false;
        for (index, ch) in input.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                let value = &input[..index];
                *input = &input[index + 1..];
                return Ok(value);
            }
        }
        Err(INVALID)
    }

    consume(&mut input, '(')?;
    let mut pairs = Vec::new();
    if input.trim() == ")" {
        return Ok(pairs);
    }
    loop {
        consume(&mut input, '(')?;
        let key = quoted(&mut input)?;
        consume(&mut input, ',')?;
        input = input.trim_start();
        let value = if input.starts_with('"') {
            quoted(&mut input)?
        } else {
            let end = input.find(')').ok_or(INVALID)?;
            let value = input[..end].trim();
            if value.is_empty() || value.contains([',', '(', '"']) {
                return Err(INVALID);
            }
            input = &input[end..];
            value
        };
        consume(&mut input, ')')?;
        pairs.push((key, value));
        input = input.trim_start();
        if input == ")" {
            return Ok(pairs);
        }
        consume(&mut input, ',')?;
    }
}

#[cfg(test)]
#[path = "satisfactory_config_tests.rs"]
mod tests;
