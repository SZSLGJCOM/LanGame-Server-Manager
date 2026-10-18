//! Parse public CurseForge identities without trusting a project display name,
//! a download filename, a third-party API, or an arbitrary URL's query string.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands) struct CurseForgeReference {
    pub(in crate::commands) project_id: Option<String>,
    pub(in crate::commands) game_slug: Option<String>,
    pub(in crate::commands) mod_slug: Option<String>,
    pub(in crate::commands) file_id: Option<String>,
}

pub(in crate::commands) fn parse_reference(reference: &str) -> Option<CurseForgeReference> {
    let reference = reference.trim();
    if reference.len() > 2048 {
        return None;
    }
    if let Some(id) = positive_id(reference) {
        return Some(id_reference(id));
    }
    for prefix in ["cf:", "curseforge:"] {
        if reference
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        {
            return positive_id(reference.get(prefix.len()..)?.trim()).map(id_reference);
        }
    }

    let url = reqwest::Url::parse(reference).ok()?;
    // These URLs provide an identity only; they are never fetched. Legacy HTTP
    // bookmarks therefore retain their meaning while metadata uses its own
    // authenticated, fixed HTTPS API endpoint.
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return None;
    }
    let host = url.host_str()?;
    if !matches!(
        host,
        "curseforge.com" | "www.curseforge.com" | "legacy.curseforge.com" | "api.curseforge.com"
    ) {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    let segments = path.strip_prefix('/')?.split('/').collect::<Vec<_>>();
    let mut result = if host == "api.curseforge.com" {
        match segments.as_slice() {
            ["v1", "mods", id] => id_reference(positive_id(id)?),
            ["v1", "mods", id, "files", file] => {
                let mut item = id_reference(positive_id(id)?);
                item.file_id = Some(positive_id(file)?);
                item
            }
            _ => return None,
        }
    } else {
        match segments.as_slice() {
            ["projects", id] => id_reference(positive_id(id)?),
            [game, kind, slug, tail @ ..]
                if token(game, 64)
                    && token(slug, 128)
                    && (*kind == "mods" || (*game == "minecraft" && *kind == "mc-mods")) =>
            {
                let file_id = match tail {
                    [] | ["files"] => None,
                    ["files", file] => Some(positive_id(file)?),
                    _ => return None,
                };
                CurseForgeReference {
                    project_id: None,
                    game_slug: Some((*game).to_string()),
                    mod_slug: Some((*slug).to_string()),
                    file_id,
                }
            }
            _ => return None,
        }
    };
    for (key, value) in url.query_pairs() {
        if matches!(
            key.as_ref(),
            "projectId" | "projectID" | "project_id" | "modId" | "modID"
        ) {
            let id = positive_id(&value)?;
            if result
                .project_id
                .as_ref()
                .is_some_and(|current| *current != id)
            {
                return None;
            }
            result.project_id = Some(id);
        }
    }
    Some(result)
}

pub(in crate::commands) fn project_id_from_reference(reference: &str) -> Option<String> {
    parse_reference(reference)?.project_id
}

pub(in crate::commands) fn mod_slug_from_reference(reference: &str) -> Option<String> {
    parse_reference(reference)?.mod_slug
}

fn id_reference(project_id: String) -> CurseForgeReference {
    CurseForgeReference {
        project_id: Some(project_id),
        game_slug: None,
        mod_slug: None,
        file_id: None,
    }
}

fn positive_id(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = value.parse::<u32>().ok()?;
    (id > 0).then(|| id.to_string())
}

fn token(value: &str, max_length: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
#[path = "commands_mod_curseforge_reference_tests.rs"]
mod tests;
