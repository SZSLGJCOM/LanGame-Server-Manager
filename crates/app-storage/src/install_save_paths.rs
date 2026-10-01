use std::path::{Path, PathBuf};

/// Resolve the static install-relative save subtree, including every dynamic
/// instance or world leaf. None means the template cannot safely isolate data.
pub fn install_save_directory_prefix(template: &str, install_root: &Path) -> Option<PathBuf> {
    let template = template.trim();
    let after_open = template.strip_prefix("{{")?;
    let (token, suffix) = after_open.split_once("}}")?;
    if token.trim() != "paths.install_root" || !suffix.starts_with(['/', '\\']) {
        return None;
    }

    let mut path = install_root.to_path_buf();
    let mut static_segments = 0;
    let mut dynamic = false;
    for segment in suffix
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
    {
        if segment.contains(['{', '}']) {
            let token = segment.strip_prefix("{{")?.strip_suffix("}}")?.trim();
            if !known_dynamic_save_token(token) {
                return None;
            }
            dynamic = true;
            continue;
        }
        // Validate even after a dynamic segment: an escape later in the
        // template must not turn an empty prefix into permission to delete.
        if segment == "."
            || segment == ".."
            || segment.trim() != segment
            || segment.ends_with('.')
            || segment.chars().any(|value| {
                value.is_control() || matches!(value, ':' | '<' | '>' | '"' | '|' | '?' | '*')
            })
        {
            return None;
        }
        if !dynamic {
            path.push(segment);
            static_segments += 1;
        }
    }
    (static_segments > 0).then_some(path)
}

fn known_dynamic_save_token(token: &str) -> bool {
    matches!(
        token,
        "instance.id"
            | "instance_id"
            | "instance.name"
            | "instance_name"
            | "instance.module_id"
            | "module.id"
            | "module_id"
    ) || token.strip_prefix("settings.").is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, '_' | '.' | '-'))
    })
}
