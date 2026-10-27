//! Abiotic's mod.io credential may be echoed in the native command-line log.
//! These helpers affect application output only; launch plans used to spawn the
//! game must retain their original arguments.

use std::ops::Range;

pub const REDACTED_MODIO_TOKEN: &str = "[REDACTED]";
const FLAG: &[u8] = b"-ModioToken";

fn horizontal_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn token_ranges(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut consumed = 0;
    for (start, candidate) in bytes.windows(FLAG.len()).enumerate() {
        if start < consumed || !candidate.eq_ignore_ascii_case(FLAG) {
            continue;
        }
        let mut position = start + FLAG.len();
        if position == bytes.len()
            || !(bytes[position] == b'=' || horizontal_space(bytes[position]))
        {
            continue;
        }
        while position < bytes.len() && horizontal_space(bytes[position]) {
            position += 1;
        }
        if bytes.get(position) == Some(&b'=') {
            position += 1;
        }
        while position < bytes.len() && horizontal_space(bytes[position]) {
            position += 1;
        }
        if position == bytes.len() || matches!(bytes[position], b'\r' | b'\n') {
            continue;
        }
        let delimiter = if bytes[position..].starts_with(b"\\\"") {
            Some(&b"\\\""[..])
        } else if matches!(bytes[position], b'\'' | b'"') {
            Some(&bytes[position..position + 1])
        } else {
            None
        };
        let (value_start, value_end) = if let Some(delimiter) = delimiter {
            let value_start = position + delimiter.len();
            let value_end = bytes[value_start..]
                .windows(delimiter.len())
                .position(|window| window == delimiter)
                .map_or(bytes.len(), |offset| value_start + offset);
            (value_start, value_end)
        } else {
            let outer_single_quote = start > 0 && bytes[start - 1] == b'\'';
            let value_end = bytes[position..]
                .iter()
                .position(|byte| {
                    byte.is_ascii_whitespace()
                        || *byte == b'"'
                        || (outer_single_quote && *byte == b'\'')
                })
                .map_or(bytes.len(), |offset| position + offset);
            (position, value_end)
        };
        if value_start < value_end {
            ranges.push(value_start..value_end);
            consumed = value_end;
        }
    }
    ranges
}

/// Hide marked token values, including partial native log lines and historical
/// credentials no longer present in the current instance configuration.
pub fn redact_modio_token_text(text: &str) -> String {
    let ranges = token_ranges(text);
    if ranges.is_empty() {
        return text.to_owned();
    }
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        result.push_str(&text[cursor..range.start]);
        result.push_str(REDACTED_MODIO_TOKEN);
        cursor = range.end;
    }
    result.push_str(&text[cursor..]);
    result
}

/// Extract only this launch option's values for the application-owned stdout
/// filter. Do not log or format the returned credentials.
pub fn modio_tokens_from_args(arguments: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for (index, argument) in arguments.iter().enumerate() {
        for range in token_ranges(argument) {
            tokens.push(argument[range].to_owned());
        }
        if argument.as_bytes().eq_ignore_ascii_case(FLAG)
            && let Some(value) = arguments.get(index + 1)
            && !value.is_empty()
        {
            tokens.push(value.trim_matches(['\'', '"']).to_owned());
        }
    }
    tokens.retain(|token| !token.is_empty());
    tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
    tokens.dedup();
    tokens
}

/// Apply only to a public preview copy, never the internal or spawned plan.
pub fn redact_abiotic_launch_preview(plan: &mut crate::LaunchPlan) {
    if plan.module_id != "abioticfactor" {
        return;
    }
    let tokens = modio_tokens_from_args(&plan.args);
    let redact = |text: &str| {
        let mut result = redact_modio_token_text(text);
        for token in &tokens {
            result = result.replace(token, REDACTED_MODIO_TOKEN);
        }
        result
    };
    for argument in &mut plan.args {
        *argument = redact(argument);
    }
    plan.command_line = redact(&plan.command_line);
}

/// The successful start response also contains public copies of every plan.
/// Call only after process creation, validation and registration are complete.
pub fn redact_abiotic_start_result(result: &mut crate::StartInstanceResult) {
    redact_abiotic_launch_preview(&mut result.launch_plan);
    for process in &mut result.launch_plans {
        redact_abiotic_launch_preview(&mut process.launch_plan);
    }
}

#[cfg(test)]
#[path = "abiotic_output_tests.rs"]
mod tests;
