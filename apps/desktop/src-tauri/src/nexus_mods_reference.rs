use super::*;

pub(super) struct NexusAuthorization {
    pub(super) key: String,
    pub(super) expires: u64,
    pub(super) user_id: u64,
}

pub(super) struct NexusReference {
    pub(super) domain: String,
    pub(super) mod_id: String,
    pub(super) file_id: Option<String>,
    pub(super) authorization: Option<NexusAuthorization>,
    pub(super) is_nxm: bool,
}

impl NexusReference {
    pub(super) fn page_url(&self) -> String {
        format!(
            "https://www.nexusmods.com/{}/mods/{}",
            self.domain, self.mod_id
        )
    }

    pub(super) fn validate_authorization(
        &self,
        account: &api::NexusAccount,
        now: u64,
    ) -> Result<(), String> {
        if let Some(authorization) = &self.authorization {
            if authorization.user_id != account.user_id {
                return Err(failure(
                    "nexus_auth_invalid",
                    "This nxm download link belongs to a different Nexus account.",
                ));
            }
            if authorization.expires <= now {
                return Err(failure(
                    "nexus_download_expired",
                    "This Nexus download link has expired. Generate a new Download with Manager link on the website.",
                ));
            }
        }
        Ok(())
    }
}

fn invalid() -> String {
    failure(
        "nexus_reference_invalid",
        "Enter a Nexus Mod ID, an official Mod/file page, or an nxm link.",
    )
}

pub(super) fn canonical_id(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|ch| ch.is_ascii_digit()) {
        return Err(invalid());
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .ok_or_else(invalid)
}

fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 128
        && domain
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, b'_' | b'-'))
}

fn website_url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("www.nexusmods.com" | "nexusmods.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid());
    }
    Ok(url)
}

pub(super) fn source_domain(source: &ModuleModSourceSpec) -> Result<String, String> {
    if source.provider != "nexus" {
        return Err(invalid());
    }
    let url = website_url(&source.url)?;
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    let domain = match parts.as_slice() {
        [domain] | ["games", domain] => *domain,
        _ => return Err(invalid()),
    };
    if !valid_domain(domain) {
        return Err(invalid());
    }
    Ok(domain.into())
}

pub(super) fn parse_reference(
    value: &str,
    expected_domain: &str,
) -> Result<NexusReference, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 8192
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err(invalid());
    }
    if value.bytes().all(|ch| ch.is_ascii_digit()) {
        return Ok(NexusReference {
            domain: expected_domain.into(),
            mod_id: canonical_id(value)?,
            file_id: None,
            authorization: None,
            is_nxm: false,
        });
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| invalid())?;
    let mut authorization = None;
    let (domain, mod_id, mut file_id) = if parsed.scheme() == "nxm" {
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid());
        }
        let domain = parsed.host_str().ok_or_else(invalid)?;
        let parts = parsed
            .path()
            .trim_matches('/')
            .split('/')
            .collect::<Vec<_>>();
        let ["mods", mod_id, "files", file_id] = parts.as_slice() else {
            return Err(invalid());
        };
        let key = unique_query(&parsed, "key")?;
        let expires = unique_query(&parsed, "expires")?;
        let user_id = unique_query(&parsed, "user_id")?;
        match (key, expires, user_id) {
            (None, None, None) => {}
            (Some(key), Some(expires), Some(user_id))
                if !key.is_empty() && key.len() <= 1024 && !key.chars().any(char::is_control) =>
            {
                authorization = Some(NexusAuthorization {
                    key,
                    expires: canonical_id(&expires)?.parse().map_err(|_| invalid())?,
                    user_id: canonical_id(&user_id)?.parse().map_err(|_| invalid())?,
                });
            }
            _ => return Err(invalid()),
        }
        (
            domain.to_string(),
            canonical_id(mod_id)?,
            Some(canonical_id(file_id)?),
        )
    } else {
        let url = website_url(value)?;
        let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
        let (domain, mod_id, path_file) = match parts.as_slice() {
            [domain, "mods", mod_id] | ["games", domain, "mods", mod_id] => {
                (*domain, *mod_id, None)
            }
            [domain, "mods", mod_id, "files", file_id]
            | ["games", domain, "mods", mod_id, "files", file_id] => {
                (*domain, *mod_id, Some(canonical_id(file_id)?))
            }
            _ => return Err(invalid()),
        };
        if unique_query(&url, "key")?.is_some()
            || unique_query(&url, "expires")?.is_some()
            || unique_query(&url, "user_id")?.is_some()
        {
            return Err(invalid());
        }
        (domain.to_string(), canonical_id(mod_id)?, path_file)
    };
    if let Some(query_file) = unique_query(&parsed, "file_id")? {
        let query_file = canonical_id(&query_file)?;
        if file_id.as_ref().is_some_and(|id| id != &query_file) {
            return Err(invalid());
        }
        file_id = Some(query_file);
    }
    if !valid_domain(&domain) {
        return Err(invalid());
    }
    if domain != expected_domain {
        return Err(failure(
            "nexus_game_mismatch",
            "This Nexus Mod belongs to a different game from this server.",
        ));
    }
    Ok(NexusReference {
        domain,
        mod_id,
        file_id,
        authorization,
        is_nxm: parsed.scheme() == "nxm",
    })
}

fn unique_query(url: &reqwest::Url, field: &str) -> Result<Option<String>, String> {
    let mut values = url
        .query_pairs()
        .filter(|(key, _)| key == field)
        .map(|(_, value)| value.into_owned());
    let first = values.next();
    if values.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}
