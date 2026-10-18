//! Workshop references accepted by the existing native configuration editors.
pub fn normalize_reference(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("--") {
        return None;
    }
    let normalized = trimmed.strip_prefix("workshop-").unwrap_or(trimmed);
    if normalized.len() >= 6 && normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(normalized.into());
    }
    if let Some(index) = trimmed.find("id=") {
        let digits = trimmed[index + 3..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        if digits.len() >= 6 {
            return Some(digits);
        }
    }
    trimmed
        .split(|character: char| !character.is_ascii_digit())
        .find(|digits| digits.len() >= 6)
        .map(String::from)
}
