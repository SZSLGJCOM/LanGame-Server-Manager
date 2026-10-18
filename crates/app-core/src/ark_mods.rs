//! ASA numeric IDs and native reference forms shared by launch and configuration.
use std::collections::HashSet;

pub fn parse_evolved_mod_ids(raw: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.split(|character: char| {
        character.is_whitespace() || matches!(character, ',' | ';' | '\u{3001}' | '\u{ff0c}')
    })
    .filter_map(crate::workshop_ids::normalize_reference)
    .filter(|id| seen.insert(id.clone()))
    .collect()
}

pub fn parse_mod_ids(raw: &str) -> Vec<String> {
    let mut mod_ids = Vec::new();
    let mut seen = HashSet::new();

    for token in raw.split(is_ark_ascended_mod_id_separator) {
        for candidate in extract_ark_ascended_mod_id_candidates(token) {
            if seen.insert(candidate.clone()) {
                mod_ids.push(candidate);
            }
        }
    }

    mod_ids
}

fn is_ark_ascended_mod_id_separator(ch: char) -> bool {
    matches!(
        ch,
        ',' | ';' | '\n' | '\r' | '\t' | ' ' | '\u{3001}' | '\u{ff0c}' | '\u{ff1b}'
    )
}

fn extract_ark_ascended_mod_id_candidates(token: &str) -> Vec<String> {
    let token = trim_mod_id_candidate(token);
    if token.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    push_numeric_mod_id(token, &mut candidates);

    let lowercase = token.to_ascii_lowercase();
    for prefix in [
        "cf-",
        "cf:",
        "curseforge-",
        "curseforge:",
        "project-",
        "project:",
        "projectid=",
        "project_id=",
        "mod-",
        "mod:",
        "modid=",
        "mod_id=",
        "id=",
    ] {
        if lowercase.starts_with(prefix) {
            push_numeric_mod_id(&token[prefix.len()..], &mut candidates);
        }
    }

    if let Some(query) = token.split_once('?').map(|(_, query)| query) {
        for pair in query.split('&') {
            push_supported_mod_id_key_value(pair, &mut candidates);
        }
    } else {
        push_supported_mod_id_key_value(token, &mut candidates);
    }

    let path = token.split_once('?').map_or(token, |(path, _)| path);
    let mut previous_was_mod_container = false;
    for segment in path.split(['/', '\\']) {
        let segment = trim_mod_id_candidate(segment);
        if previous_was_mod_container {
            push_numeric_mod_id(segment, &mut candidates);
        }
        previous_was_mod_container = is_ark_ascended_mod_container_segment(segment);
    }

    candidates
}

fn push_supported_mod_id_key_value(pair: &str, candidates: &mut Vec<String>) {
    let Some((key, value)) = pair.split_once('=') else {
        return;
    };
    if is_supported_ark_ascended_mod_id_key(key) {
        push_numeric_mod_id(value, candidates);
    }
}

fn is_supported_ark_ascended_mod_id_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(normalized.as_str(), "id" | "modid" | "projectid")
}

fn is_ark_ascended_mod_container_segment(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "mod" | "mods" | "project" | "projects"
    )
}

fn push_numeric_mod_id(candidate: &str, candidates: &mut Vec<String>) {
    let candidate = trim_mod_id_candidate(candidate);
    if !candidate.is_empty() && candidate.chars().all(|ch| ch.is_ascii_digit()) {
        candidates.push(String::from(candidate));
    }
}

fn trim_mod_id_candidate(candidate: &str) -> &str {
    candidate.trim().trim_matches(|ch| {
        matches!(
            ch,
            '"' | '\'' | '`' | '[' | ']' | '(' | ')' | '<' | '>' | '{' | '}' | '.'
        )
    })
}
