use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Deserialize;

mod store_description;

const SUCCESS_TTL: Duration = Duration::from_secs(15 * 60);
const FAILURE_TTL: Duration = Duration::from_secs(60);
const HEALTH_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePreference {
    ChinaFirst,
    InternationalFirst,
}

impl SourcePreference {
    pub fn from_locale(locale: Option<&str>) -> Self {
        let language = locale
            .and_then(|value| value.split('-').next())
            .unwrap_or_default();
        if language.eq_ignore_ascii_case("zh") {
            Self::ChinaFirst
        } else {
            Self::InternationalFirst
        }
    }

    fn prefers_china(self) -> bool {
        self == Self::ChinaFirst
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    schema_version: u32,
    china_origins: Vec<String>,
    groups: Vec<Group>,
    exact_resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Group {
    id: String,
    origins: Vec<String>,
    path_prefixes: Vec<String>,
    #[serde(default)]
    exact_paths: Vec<String>,
    #[serde(default)]
    allowed_query_keys: Vec<String>,
    #[serde(default)]
    purposes: Vec<String>,
    #[serde(default)]
    purpose_path_prefixes: HashMap<String, Vec<String>>,
    #[serde(default)]
    purpose_path_suffixes: HashMap<String, Vec<String>>,
    #[serde(default)]
    base_paths: HashMap<String, String>,
    #[serde(default)]
    filename_patterns: Vec<FilenamePattern>,
}

#[derive(Deserialize)]
struct FilenamePattern {
    prefix: String,
    extension: String,
}

impl Group {
    fn resource_path(&self, origin: &str, url: &Url, input: &str) -> Option<String> {
        if !self.origins.iter().any(|candidate| candidate == origin) {
            return None;
        }
        let path = if self.base_paths.is_empty() {
            url.path().to_owned()
        } else {
            // This narrow basename mapping must never authorize a path that
            // the URL parser normalized out of traversal or encoded input.
            let raw_path = input.split_once("://")?.1.split_once('/')?.1;
            if url.query().is_some() || format!("/{raw_path}") != url.path() {
                return None;
            }
            let name = url
                .path()
                .strip_prefix(self.base_paths.get(origin)?.as_str())?;
            if name.is_empty()
                || name.len() > 255
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                || name.split('.').any(str::is_empty)
            {
                return None;
            }
            format!("/{name}")
        };
        let matched = self.exact_paths.iter().any(|exact| exact == &path)
            || self
                .path_prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix))
            || self.filename_patterns.iter().any(|pattern| {
                let Some(name) = path
                    .strip_prefix('/')
                    .and_then(|name| name.strip_prefix(pattern.prefix.as_str()))
                else {
                    return false;
                };
                let Some((stem, suffix)) = name.split_once(pattern.extension.as_str()) else {
                    return false;
                };
                !stem.is_empty() && (suffix.is_empty() || suffix.starts_with('.'))
            });
        matched.then_some(path)
    }
}

pub(crate) fn media_identity(input: &str, purpose: &str) -> Option<String> {
    let (key, _) = candidates(policy(), input)?;
    let group = policy().groups.iter().find(|group| group.id == key)?;
    if !group.purposes.iter().any(|value| value == purpose) {
        return None;
    }
    let mut url = Url::parse(input).ok()?;
    if group
        .purpose_path_prefixes
        .get(purpose)
        .is_some_and(|prefixes| !prefixes.iter().any(|prefix| url.path().starts_with(prefix)))
        || group
            .purpose_path_suffixes
            .get(purpose)
            .is_some_and(|suffixes| !suffixes.iter().any(|suffix| url.path().ends_with(suffix)))
    {
        return None;
    }
    let mut query = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    query.sort();
    // Reordering duplicate keys can change which value an origin uses.
    if query.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return None;
    }
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Some(format!(
        "{}:{}{}",
        group.id,
        url.path(),
        url.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
    ))
}

#[derive(Deserialize)]
struct Resource {
    id: String,
    urls: Vec<String>,
}

struct Health {
    key: String,
    origin: String,
    at: Instant,
    success: bool,
}

fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        // This is a source-controlled build input, verified by policy tests.
        let policy: Policy = serde_json::from_str(include_str!("../official-sources.json"))
            .expect("bundled official source policy must match its checked schema");
        assert_eq!(
            policy.schema_version, 1,
            "unsupported bundled source policy"
        );
        policy
    })
}

fn health() -> &'static Mutex<Vec<Health>> {
    static HEALTH: OnceLock<Mutex<Vec<Health>>> = OnceLock::new();
    HEALTH.get_or_init(|| Mutex::new(Vec::new()))
}

fn public_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
}

fn candidates(policy: &Policy, input: &str) -> Option<(String, Vec<String>)> {
    let url = Url::parse(input).ok()?;
    if !public_https(&url) {
        return None;
    }
    if let Some(resource) = policy
        .exact_resources
        .iter()
        .find(|entry| entry.urls.iter().any(|value| value == url.as_str()))
    {
        let mut values = vec![url.to_string()];
        values.extend(
            resource
                .urls
                .iter()
                .filter(|value| **value != url.as_str())
                .cloned(),
        );
        return Some((resource.id.clone(), values));
    }
    let origin = url.origin().ascii_serialization();
    let (group, path) = policy.groups.iter().find_map(|entry| {
        entry
            .resource_path(&origin, &url, input)
            .map(|path| (entry, path))
    })?;
    if !url.query_pairs().all(|(key, _)| {
        group
            .allowed_query_keys
            .iter()
            .any(|allowed| allowed == key.as_ref())
    }) {
        return None;
    }
    if group.id == "steam-store-description" && !store_description::allowed(&url, input) {
        return None;
    }
    let mut values = vec![url.to_string()];
    for target in &group.origins {
        if target == &origin {
            continue;
        }
        let target_url = Url::parse(target).ok()?;
        let mut candidate = url.clone();
        candidate.set_host(target_url.host_str()).ok()?;
        if !group.base_paths.is_empty() {
            candidate.set_path(&format!(
                "{}{}",
                group.base_paths.get(target)?,
                path.strip_prefix('/')?
            ));
        }
        values.push(candidate.to_string());
    }
    Some((group.id.clone(), values))
}

fn ordered_candidates(
    policy: &Policy,
    input: &str,
    preference: SourcePreference,
    entries: &[Health],
    now: Instant,
) -> Vec<String> {
    let Some((key, mut values)) = candidates(policy, input) else {
        return vec![input.to_string()];
    };
    // A healthy source wins over one in cooldown. Within that set, request
    // locale wins over cached success from a different region. Stable sorting
    // preserves the original source order when these preferences are equal.
    values.sort_by_key(|value| {
        let origin = Url::parse(value)
            .ok()
            .map(|url| url.origin().ascii_serialization());
        let is_china = origin
            .as_ref()
            .is_some_and(|origin| policy.china_origins.contains(origin));
        let entry = entries
            .iter()
            .rev()
            .find(|entry| entry.key == key && Some(&entry.origin) == origin.as_ref());
        let cooling_down = entry.is_some_and(|entry| {
            !entry.success && now.saturating_duration_since(entry.at) < FAILURE_TTL
        });
        let recent_success = entry.is_some_and(|entry| {
            entry.success && now.saturating_duration_since(entry.at) < SUCCESS_TTL
        });
        (
            cooling_down,
            is_china != preference.prefers_china(),
            !recent_success,
        )
    });
    values.dedup();
    values
}

/// Only explicitly verified equivalent resources can change origin. Unknown URLs
/// remain intact, including signed URLs and application-specific API endpoints.
/// Region preference belongs to this request, never to shared application state.
pub fn official_url_candidates(input: &str, preference: SourcePreference) -> Vec<String> {
    let now = Instant::now();
    match health().lock() {
        Ok(entries) => ordered_candidates(policy(), input, preference, &entries, now),
        Err(_) => ordered_candidates(policy(), input, preference, &[], now),
    }
}

fn record(input: &str, success: bool) {
    let Some((key, _)) = candidates(policy(), input) else {
        return;
    };
    let Ok(url) = Url::parse(input) else {
        return;
    };
    let origin = url.origin().ascii_serialization();
    let now = Instant::now();
    if let Ok(mut entries) = health().lock() {
        entries.retain(|entry| {
            now.duration_since(entry.at) < SUCCESS_TTL
                && !(entry.key == key && entry.origin == origin)
        });
        if entries.len() >= HEALTH_LIMIT {
            entries.remove(0);
        }
        entries.push(Health {
            key,
            origin,
            at: now,
            success,
        });
    }
}

pub fn record_success(url: &str) {
    record(url, true);
}
pub fn record_failure(url: &str) {
    record(url, false);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod store_description_tests;
