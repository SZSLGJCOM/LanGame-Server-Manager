//! Only the verified, public full-description subset of StoreBrowse can change origin.

use reqwest::Url;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescriptionRequest {
    ids: [AppId; 1],
    context: Context,
    data_request: DataRequest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppId {
    appid: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    language: String,
    country_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataRequest {
    include_full_description: bool,
}

pub(super) fn allowed(url: &Url, input: &str) -> bool {
    let Some(raw_path) = input
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, path)| path.split('?').next().unwrap_or_default())
    else {
        return false;
    };
    // A normalized traversal must not acquire an equivalent-origin mapping.
    if format!("/{raw_path}") != url.path() {
        return false;
    }
    let Some(query) = url.query() else {
        return false;
    };
    if query.len() > 4096 || !query.starts_with("input_json=") || query.contains('&') {
        return false;
    }
    let mut pairs = url.query_pairs();
    let Some((key, value)) = pairs.next() else {
        return false;
    };
    if key != "input_json" || value.len() > 4096 || pairs.next().is_some() {
        return false;
    }
    // Derived structs reject both unknown and duplicate JSON fields. Parsing
    // through Value would silently retain the last duplicate instead.
    let Ok(request) = serde_json::from_str::<DescriptionRequest>(&value) else {
        return false;
    };
    request.ids[0].appid > 0
        && matches!(request.context.language.as_str(), "schinese" | "english")
        && matches!(request.context.country_code.as_str(), "CN" | "US")
        && request.data_request.include_full_description
}
