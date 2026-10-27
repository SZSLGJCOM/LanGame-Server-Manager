//! The native server waits for its selected mod.io Mods before completing setup.
//! Use its configured download limit plus the existing listener budget. Native
//! format strings alone do not prove a stable log protocol, so no inferred Mod
//! completion/failure marker resets or extends this one bounded allowance.
use std::time::{Duration, Instant};

const NATIVE_DOWNLOAD_TIMEOUT_SECONDS: u64 = 900;

fn assigned_values<'a>(args: &'a [String], option: &'a str) -> impl Iterator<Item = &'a str> {
    args.iter().filter_map(move |argument| {
        let (key, value) = argument.split_once('=')?;
        key.eq_ignore_ascii_case(option).then_some(value)
    })
}

pub(super) fn needs_download_allowance(module_id: &str, args: &[String]) -> bool {
    module_id == "abioticfactor"
        && !args
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("-SideloadMods"))
        && assigned_values(args, "-Mods").any(|ids| !ids.trim().is_empty())
}

/// `normal` is the existing module-specific listener limit, not a replacement
/// timeout. Read the actual argv so persisted defaults cannot disagree with the
/// launched native ModInstallTimeout value. Other modules and modes are unchanged.
pub(super) fn listener_timeout(
    module_id: &str,
    args: &[String],
    normal: Duration,
) -> Result<Duration, String> {
    if !needs_download_allowance(module_id, args) {
        return Ok(normal);
    }
    if assigned_values(args, "-Mods").count() != 1 {
        return Err("Abiotic Factor startup has an ambiguous official Mod list.".into());
    }
    let mut values = assigned_values(args, "-ModInstallTimeout");
    let seconds = match values.next() {
        None => NATIVE_DOWNLOAD_TIMEOUT_SECONDS,
        Some(value) => value.parse::<u64>()
            .ok()
            .filter(|seconds| *seconds > 0)
            .ok_or("Abiotic Factor Mod installation timeout must be a positive whole number of seconds.")?,
    };
    if values.next().is_some() {
        return Err("Abiotic Factor startup has more than one Mod installation timeout.".into());
    }
    let timeout = normal.checked_add(Duration::from_secs(seconds)).ok_or(
        "Abiotic Factor Mod installation timeout exceeds the supported startup clock range.",
    )?;
    if Instant::now().checked_add(timeout).is_none() {
        return Err(
            "Abiotic Factor Mod installation timeout exceeds the supported startup clock range."
                .into(),
        );
    }
    Ok(timeout)
}

#[cfg(test)]
#[path = "commands_abiotic_mod_startup_tests.rs"]
mod tests;
