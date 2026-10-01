use std::collections::HashSet;
use std::fs;
use std::path::Path;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::{KnowledgeError, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u8,
    pub module_id: String,
    pub scope: String,
    pub gaps: Vec<String>,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub authority: String,
    pub kind: String,
    pub seeds: Vec<String>,
    pub allowed_prefixes: Vec<String>,
    pub discover_links: bool,
    /// Reviewed publisher terms permit linking but not background AI ingestion.
    #[serde(default)]
    pub reference_only: bool,
    pub max_pages: usize,
    pub content_selector: Option<String>,
    /// Optional HTML container for discovering links, separate from body text.
    #[serde(default)]
    pub discovery_selector: Option<String>,
    #[serde(default)]
    pub sitemaps: Vec<String>,
    pub authority_evidence: String,
    pub license_note: String,
    pub license_url: Option<String>,
    pub reviewed_on: String,
}

pub fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(KnowledgeError::Invalid(
            "Invalid knowledge identifier".into(),
        ));
    }
    Ok(())
}

pub fn public_url(value: &str) -> Result<Url> {
    if !text_valid(value, 2048)
        || !value.starts_with("https://")
        || value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\\' | '"' | '<' | '>' | '`' | '#'))
    {
        return Err(KnowledgeError::Invalid(
            "Invalid public documentation URL".into(),
        ));
    }
    let url =
        Url::parse(value).map_err(|_| KnowledgeError::Invalid("Invalid source URL".into()))?;
    let authority = value
        .strip_prefix("https://")
        .unwrap_or("")
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || authority.contains(':')
    {
        return Err(KnowledgeError::Invalid(
            "Documentation sources must use public HTTPS without credentials".into(),
        ));
    }
    if let Some(host) = url.host_str()
        && (host != authority
            || !host.contains('.')
            || [".local", ".localhost", ".internal", ".test", ".invalid"]
                .iter()
                .any(|suffix| host.ends_with(suffix))
            || host.parse::<std::net::IpAddr>().is_ok()
            || host.starts_with('[')
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            }))
    {
        return Err(KnowledgeError::Invalid(
            "Documentation sources must use public DNS names".into(),
        ));
    }
    let raw_path = value
        .strip_prefix("https://")
        .unwrap_or("")
        .strip_prefix(authority)
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    let decoded = decode_percent(value)?;
    if decoded.chars().any(|c| c.is_control() || c == '\\')
        || ["%2f", "%5c", "%25"]
            .iter()
            .any(|part| raw_path.to_ascii_lowercase().contains(part))
        || decode_percent(raw_path)?
            .split('/')
            .any(|part| part == "." || part == "..")
    {
        return Err(KnowledgeError::Invalid(
            "Documentation URL contains ambiguous path encoding".into(),
        ));
    }
    Ok(url)
}

impl Source {
    /// Keep API URLs as HTTP cache keys, but cite the reviewed public article.
    pub(crate) fn citation_url(&self, url: &str) -> String {
        Url::parse(url)
            .ok()
            .and_then(|url| crate::zendesk::citation_url(self, &url))
            .map(String::from)
            .unwrap_or_else(|| url.to_owned())
    }

    /// Exact seeds may have fixed query parameters. Discovery is restricted to
    /// explicit directory boundaries and never inherits a seed's entire host.
    pub fn allows(&self, url: &Url) -> bool {
        if self.reference_only {
            return false;
        }
        let mut clean = url.clone();
        clean.set_fragment(None);
        if public_url(clean.as_str()).is_err() {
            return false;
        }
        if self.seeds.iter().chain(self.sitemaps.iter()).any(|seed| {
            Url::parse(seed).is_ok_and(|mut u| {
                u.set_fragment(None);
                u == clean
            })
        }) {
            return true;
        }
        // IPS topics retain their numeric identity when publishers rename a
        // title. This exception is only for an explicitly reviewed seed topic.
        if let Some(topic_id) = forum_topic_id(&clean)
            && self.seeds.iter().any(|seed| {
                Url::parse(seed).is_ok_and(|seed| {
                    seed.origin() == clean.origin()
                        && seed.path() == clean.path()
                        && forum_topic_id(&seed) == Some(topic_id)
                })
            })
        {
            return true;
        }
        clean.query().is_none()
            && self.allowed_prefixes.iter().any(|prefix| {
                Url::parse(prefix).is_ok_and(|p| {
                    p.origin() == clean.origin() && clean.path().starts_with(p.path())
                })
            })
    }

    pub fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        if !matches!(
            self.kind.as_str(),
            "official" | "official_community" | "community"
        ) || self.seeds.is_empty()
            || self.seeds.len() > 256
            || self.allowed_prefixes.len() > 64
            || self.sitemaps.len() > 16
            || self.max_pages == 0
            || self.max_pages > 512
            || !text_valid(&self.title, 512)
            || !text_valid(&self.authority, 512)
            || !text_valid(&self.license_note, 4096)
            || !valid_review_date(&self.reviewed_on)
            || (self.discover_links && self.allowed_prefixes.is_empty())
            || (self.discovery_selector.is_some() && !self.discover_links)
        {
            return Err(KnowledgeError::Invalid(format!(
                "Invalid source policy: {}",
                self.id
            )));
        }
        for (name, selector) in [
            ("content_selector", &self.content_selector),
            ("discovery_selector", &self.discovery_selector),
        ] {
            if let Some(selector) = selector
                && (!text_valid(selector, 1024) || dom_query::Matcher::new(selector).is_err())
            {
                return Err(KnowledgeError::Invalid(format!(
                    "Invalid {name} in source policy: {}",
                    self.id
                )));
            }
        }
        for url in self
            .seeds
            .iter()
            .chain(self.sitemaps.iter())
            .chain(std::iter::once(&self.authority_evidence))
            .chain(self.license_url.iter())
        {
            public_url(url)?;
        }
        for list in [&self.seeds, &self.sitemaps, &self.allowed_prefixes] {
            if list.iter().collect::<HashSet<_>>().len() != list.len() {
                return Err(KnowledgeError::Invalid("Duplicate source URL".into()));
            }
        }
        for prefix in &self.allowed_prefixes {
            let url = public_url(prefix)?;
            if !url.path().ends_with('/')
                || url.query().is_some()
                || url.fragment().is_some()
                || !self
                    .seeds
                    .iter()
                    .any(|seed| Url::parse(seed).is_ok_and(|seed| seed.origin() == url.origin()))
            {
                return Err(KnowledgeError::Invalid(format!(
                    "Source prefix must be a directory: {prefix}"
                )));
            }
        }
        Ok(())
    }
}

fn forum_topic_id(url: &Url) -> Option<&str> {
    if url.path() != "/index.php" {
        return None;
    }
    let topic = url
        .query()?
        .strip_prefix("/forums/topic/")?
        .strip_suffix('/')?;
    let (id, slug) = topic.split_once('-')?;
    if !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && !slug.is_empty()
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Some(id)
    } else {
        None
    }
}

pub fn load(root: &Path, module_id: &str) -> Result<Catalog> {
    validate_id(module_id)?;
    let directory = root.join(module_id);
    let path = directory.join("knowledge-sources.toml");
    for entry in [root, directory.as_path(), path.as_path()] {
        let metadata = fs::symlink_metadata(entry)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(KnowledgeError::Invalid(
                "Source catalogs cannot be links or reparse points".into(),
            ));
        }
    }
    if fs::metadata(&path)?.len() > 256 * 1024 {
        return Err(KnowledgeError::Invalid(
            "Source catalog exceeds 256 KiB".into(),
        ));
    }
    let catalog: Catalog = toml::from_str(&fs::read_to_string(path)?).map_err(|e| {
        KnowledgeError::Invalid(format!("Invalid source catalog for {module_id}: {e}"))
    })?;
    if catalog.schema_version != 1
        || catalog.module_id != module_id
        || catalog.sources.is_empty()
        || catalog.sources.len() > 16
        || !text_valid(&catalog.scope, 2048)
        || catalog.gaps.len() > 32
        || catalog.gaps.iter().any(|gap| !text_valid(gap, 2048))
    {
        return Err(KnowledgeError::Invalid(format!(
            "Invalid source catalog scope for {module_id}"
        )));
    }
    let mut ids = HashSet::new();
    for source in &catalog.sources {
        source.validate()?;
        crate::zendesk::validate_source_module(module_id, source)?;
        if !ids.insert(&source.id) {
            return Err(KnowledgeError::Invalid("Duplicate source ID".into()));
        }
    }
    Ok(catalog)
}

pub fn load_all(root: &Path) -> Result<Vec<Catalog>> {
    let mut catalogs = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.path().join("module.toml").exists()
            || entry.path().join("knowledge-sources.toml").exists()
        {
            let module = entry.file_name().to_string_lossy().into_owned();
            catalogs.push(load(root, &module)?);
        }
    }
    catalogs.sort_by(|a, b| a.module_id.cmp(&b.module_id));
    if catalogs.is_empty() {
        return Err(KnowledgeError::Unavailable(
            "No documentation source catalogs were packaged".into(),
        ));
    }
    Ok(catalogs)
}

fn text_valid(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn decode_percent(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .and_then(|v| std::str::from_utf8(v).ok())
                .and_then(|v| u8::from_str_radix(v, 16).ok())
                .ok_or_else(|| KnowledgeError::Invalid("Invalid URL percent encoding".into()))?;
            decoded.push(hex);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| KnowledgeError::Invalid("Invalid UTF-8 URL encoding".into()))
}

fn valid_review_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let parsed = parts
        .iter()
        .map(|v| v.parse::<i64>())
        .collect::<std::result::Result<Vec<_>, _>>();
    let Ok(date) = parsed else {
        return false;
    };
    let (year, month, day) = (date[0], date[1], date[2]);
    if year < 1970 || !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > days {
        return false;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year / 400;
    let yoe = adjusted_year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let since_epoch = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    since_epoch <= (crate::unix_seconds() / 86400) as i64
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_optional_bounded_and_validated_before_fetching() {
        let source: Source = toml::from_str(
            r#"
            id = "publisher"
            title = "Publisher manual"
            authority = "Publisher"
            kind = "official"
            seeds = ["https://example.com/manual/index"]
            allowed_prefixes = ["https://example.com/manual/"]
            discover_links = true
            max_pages = 32
            authority_evidence = "https://example.com/about"
            license_note = "Public reference"
            reviewed_on = "2026-09-28"
            "#,
        )
        .unwrap();
        assert!(source.discovery_selector.is_none());
        source.validate().unwrap();
        let mut bounded = source.clone();
        bounded.content_selector = Some("article > .body".into());
        bounded.discovery_selector = Some("nav.manual a[href], .official-guides".into());
        bounded.validate().unwrap();
        let mut disabled = bounded.clone();
        disabled.discover_links = false;
        assert!(disabled.validate().is_err());
        for invalid in [
            "".to_owned(),
            "a[".to_owned(),
            "a\n[href]".to_owned(),
            "a".repeat(1025),
            "界".repeat(342),
        ] {
            let mut discovery = source.clone();
            discovery.discovery_selector = Some(invalid.clone());
            assert!(discovery.validate().is_err(), "{invalid:?}");
            let mut content = source.clone();
            content.content_selector = Some(invalid.clone());
            assert!(content.validate().is_err(), "{invalid:?}");
        }
    }
}
