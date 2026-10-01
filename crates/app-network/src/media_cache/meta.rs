use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const DEFAULT_LIFETIME: u64 = 7 * 86400;
const MAX_LIFETIME: u64 = 30 * 86400;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Headers {
    pub content_type: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
    pub expires: Option<String>,
    pub age: u64,
    pub vary_star: bool,
}

impl Headers {
    pub fn read(headers: &reqwest::header::HeaderMap) -> Self {
        let value = |key: &str| {
            headers
                .get(key)
                .and_then(|value| value.to_str().ok())
                .filter(|value| value.len() <= 2048)
                .map(str::to_string)
        };
        let controls = headers
            .get_all("cache-control")
            .iter()
            .map(|value| value.to_str())
            .collect::<Result<Vec<_>, _>>();
        let cache_control = match controls {
            Ok(values) if values.is_empty() => None,
            Ok(values) if values.iter().map(|value| value.len()).sum::<usize>() <= 4096 => {
                Some(values.join(","))
            }
            _ => Some("no-store".into()),
        };
        Self {
            content_type: value("content-type")
                .unwrap_or_else(|| "application/octet-stream".into()),
            etag: value("etag"),
            last_modified: value("last-modified"),
            cache_control,
            expires: value("expires"),
            age: value("age")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            vary_star: headers.get_all("vary").iter().any(|value| {
                value.to_str().map_or(true, |value| {
                    value.split(',').any(|value| value.trim() == "*")
                })
            }),
        }
    }
    pub fn validator(&self) -> Option<Validator> {
        // A bare Last-Modified (and especially a weak ETag) does not prove
        // byte equivalence for If-Range. Only strong entity tags pin blocks.
        self.etag
            .as_ref()
            .filter(|etag| {
                etag.len() >= 2
                    && etag.starts_with('"')
                    && etag.ends_with('"')
                    && etag.as_bytes()[1..etag.len() - 1]
                        .iter()
                        .all(|byte| *byte == 0x21 || (0x23..=0x7e).contains(byte))
            })
            .map(|etag| Validator::Etag(etag.clone()))
    }
    pub fn merge_revalidation(&mut self, value: Self) {
        if let Some(etag) = value.etag {
            self.etag = Some(etag);
        }
        if let Some(modified) = value.last_modified {
            self.last_modified = Some(modified);
        }
        if let Some(control) = value.cache_control {
            self.cache_control = Some(control);
        }
        if let Some(expires) = value.expires {
            self.expires = Some(expires);
        }
        self.age = value.age;
        self.vary_star |= value.vary_star;
    }
    pub fn freshness(&self, now: u64) -> Freshness {
        let mut no_store = self.vary_star;
        let mut revalidate = false;
        let mut must_revalidate = false;
        let mut max_age = None;
        for part in self.cache_control.as_deref().unwrap_or_default().split(',') {
            let (key, value) = part.trim().split_once('=').unwrap_or((part.trim(), ""));
            match key.trim().to_ascii_lowercase().as_str() {
                "no-store" | "private" => no_store = true,
                "no-cache" => revalidate = true,
                "must-revalidate" => must_revalidate = true,
                "max-age" => {
                    max_age = Some(value.trim().trim_matches('"').parse::<u64>().unwrap_or(0))
                }
                _ => {}
            }
        }
        let seconds = if revalidate {
            0
        } else if let Some(age) = max_age {
            age.saturating_sub(self.age)
        } else if let Some(expires) = &self.expires {
            http_date(expires).unwrap_or(0).saturating_sub(now)
        } else {
            DEFAULT_LIFETIME.saturating_sub(self.age)
        };
        Freshness {
            expires_at: now.saturating_add(seconds.min(MAX_LIFETIME)),
            no_store,
            stale_allowed: !no_store && !must_revalidate && !revalidate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Validator {
    Etag(String),
}
impl Validator {
    pub fn value(&self) -> &str {
        match self {
            Self::Etag(value) => value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Freshness {
    pub expires_at: u64,
    pub no_store: bool,
    pub stale_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Blob {
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Entry {
    pub source: String,
    pub headers: Headers,
    pub freshness: Freshness,
    pub total: u64,
    pub accessed: u64,
    pub full: Option<Blob>,
    pub blocks: BTreeMap<u64, Blob>,
}
impl Entry {
    pub fn new(source: String, headers: Headers, total: u64, now: u64) -> Self {
        let freshness = headers.freshness(now);
        Self {
            source,
            headers,
            freshness,
            total,
            accessed: now,
            full: None,
            blocks: BTreeMap::new(),
        }
    }
    pub fn fresh(&self, now: u64) -> bool {
        now < self.freshness.expires_at
    }
    pub fn blobs(&self) -> impl Iterator<Item = &Blob> {
        self.full.iter().chain(self.blocks.values())
    }
    pub fn revalidate(&mut self, headers: Headers, now: u64) {
        self.headers.merge_revalidation(headers);
        self.freshness = self.headers.freshness(now);
    }
}

// HTTP recipients also accept obsolete RFC850 and asctime dates. Invalid dates
// are treated as already expired instead of silently gaining a seven-day TTL.
fn http_date(value: &str) -> Option<u64> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    let (day, month, year, time) = if words.len() == 6 && words[5] == "GMT" {
        (
            words[1].parse::<u32>().ok()?,
            words[2],
            words[3].parse::<u32>().ok()?,
            words[4],
        )
    } else if words.len() == 4 && words[1].contains('-') && words[3] == "GMT" {
        let date = words[1].split('-').collect::<Vec<_>>();
        if date.len() != 3 {
            return None;
        }
        let short = date[2].parse::<u32>().ok()?;
        (
            date[0].parse().ok()?,
            date[1],
            if short >= 70 {
                1900 + short
            } else {
                2000 + short
            },
            words[2],
        )
    } else if words.len() == 5 {
        (
            words[2].parse().ok()?,
            words[1],
            words[4].parse().ok()?,
            words[3],
        )
    } else {
        return None;
    };
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|candidate| candidate.eq_ignore_ascii_case(month))?;
    if !(1970..=9999).contains(&year) {
        return None;
    }
    let leap = |year: u32| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > months[month] {
        return None;
    }
    let time = time
        .split(':')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if time.len() != 3 || time[0] > 23 || time[1] > 59 || time[2] > 59 {
        return None;
    }
    let days = (1970..year)
        .map(|year| if leap(year) { 366_u64 } else { 365 })
        .sum::<u64>()
        + months[..month]
            .iter()
            .map(|days| u64::from(*days))
            .sum::<u64>()
        + u64::from(day - 1);
    Some(days * 86400 + time[0] * 3600 + time[1] * 60 + time[2])
}
