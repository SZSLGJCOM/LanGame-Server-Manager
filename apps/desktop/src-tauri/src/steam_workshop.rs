use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

use app_network::SourcePreference;
use serde::Serialize;
use serde_json::Value;

mod community;
mod download_validation;
mod file_types;
mod localized_details;
mod network_error;
mod search;

pub use download_validation::validate_workshop_download_items;
pub use localized_details::read_public_workshop_item_details;
pub use search::{SteamWorkshopSearchResult, search_public_workshop_items};

const COLLECTION_DETAILS_URL: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetCollectionDetails/v1/";
const PUBLISHED_FILE_DETAILS_URL: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
const LOOKUP_CHUNK_SIZE: usize = 50;
const MAX_LOOKUP_IDS: usize = 64;
const MAX_CHILD_TITLE_LOOKUPS: usize = 256;
const USER_AGENT: &str = concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION"));
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct SteamWorkshopLookupChild {
    pub id: String,
    pub title: Option<String>,
    pub preview_url: Option<String>,
    pub consumer_app_id: Option<u32>,
    pub item_kind: String,
    pub status: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SteamWorkshopLookupItem {
    pub id: String,
    pub title: Option<String>,
    pub preview_url: Option<String>,
    pub description: Option<String>,
    pub description_excerpt: Option<String>,
    pub detail_url: String,
    pub item_kind: String,
    pub status: String,
    pub message: Option<String>,
    pub localization_warning: Option<String>,
    pub consumer_app_id: Option<u32>,
    pub creator_app_id: Option<u32>,
    pub creator_id: Option<String>,
    pub file_size: Option<u64>,
    pub created_at_unix: Option<u64>,
    pub updated_at_unix: Option<u64>,
    pub subscriptions: Option<u64>,
    pub favorites: Option<u64>,
    pub views: Option<u64>,
    pub tags: Vec<String>,
    pub child_count: usize,
    pub children: Vec<SteamWorkshopLookupChild>,
}

pub async fn lookup_public_workshop_items(
    ids: Vec<String>,
    preference: SourcePreference,
) -> Result<Vec<SteamWorkshopLookupItem>, String> {
    let ids = normalize_lookup_ids(ids)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    lookup_workshop_items(&ids, preference).await
}

async fn lookup_workshop_items(
    ids: &[String],
    preference: SourcePreference,
) -> Result<Vec<SteamWorkshopLookupItem>, String> {
    let client = workshop_client()?;
    // Reserve the final 15 seconds for best-effort type verification, so a cold
    // library cannot lose its fetched metadata to an outer whole-request timeout.
    let (published_file_map, collection_children_map) = tokio::time::timeout(
        Duration::from_secs(30),
        fetch_lookup_metadata(&client, ids, preference),
    )
    .await
    .map_err(|_| String::from("Steam Workshop metadata lookup timed out. Please retry."))??;
    finish_lookup_items(
        &client,
        ids,
        preference,
        published_file_map,
        collection_children_map,
        None,
    )
    .await
}

async fn finish_lookup_items(
    client: &reqwest::Client,
    ids: &[String],
    preference: SourcePreference,
    mut published_file_map: HashMap<String, Value>,
    collection_children_map: HashMap<String, Vec<String>>,
    html_error: Option<&str>,
) -> Result<Vec<SteamWorkshopLookupItem>, String> {
    let type_errors = file_types::populate_file_types(
        client,
        &mut published_file_map,
        &collection_children_map,
        preference,
        html_error,
    )
    .await?;
    for (id, error) in type_errors {
        if let Some(detail) = published_file_map.get_mut(&id) {
            detail["type_error"] = Value::from(error);
        }
    }
    ids.iter()
        .map(|id| {
            build_lookup_item(
                id,
                &published_file_map,
                &collection_children_map,
                &published_file_map,
            )
        })
        .collect()
}

fn workshop_language(locale: Option<&str>) -> &'static str {
    if locale
        .unwrap_or("zh-CN")
        .trim()
        .to_ascii_lowercase()
        .starts_with("zh")
    {
        "schinese"
    } else {
        "english"
    }
}

async fn fetch_lookup_metadata(
    client: &reqwest::Client,
    ids: &[String],
    preference: SourcePreference,
) -> Result<(HashMap<String, Value>, HashMap<String, Vec<String>>), String> {
    let (mut published_file_map, collection_children_map) = tokio::try_join!(
        fetch_published_file_map(client, ids, preference),
        fetch_collection_children_map(client, ids, preference)
    )?;

    let mut seen_children = HashSet::new();
    let child_lookup_ids = ids
        .iter()
        .filter_map(|id| collection_children_map.get(id))
        .flatten()
        .filter(|id| !published_file_map.contains_key(*id) && seen_children.insert((*id).clone()))
        .take(MAX_CHILD_TITLE_LOOKUPS)
        .cloned()
        .collect::<Vec<_>>();
    published_file_map
        .extend(fetch_published_file_map(client, &child_lookup_ids, preference).await?);
    Ok((published_file_map, collection_children_map))
}

fn workshop_client() -> Result<reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                // In particular, never carry the Community CDN's Host to a redirect.
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|error| format!("failed to prepare Workshop client: {error}"))
        })
        .clone()
}

async fn read_workshop_response(
    client: &reqwest::Client,
    request: reqwest::Request,
    preference: SourcePreference,
) -> Result<app_network::PublicBytes, String> {
    let stage = network_error::request_stage(&request);
    community::read(
        client,
        request,
        Duration::from_secs(20),
        MAX_RESPONSE_BYTES,
        preference,
    )
    .await
    .map_err(|error| network_error::workshop_network_error(stage, &error))
}

async fn fetch_published_file_map(
    client: &reqwest::Client,
    ids: &[String],
    preference: SourcePreference,
) -> Result<HashMap<String, Value>, String> {
    let mut detail_map = HashMap::new();
    for chunk in ids.chunks(LOOKUP_CHUNK_SIZE) {
        let mut form = vec![(String::from("itemcount"), chunk.len().to_string())];
        for (index, id) in chunk.iter().enumerate() {
            form.push((format!("publishedfileids[{index}]"), id.clone()));
        }

        let request = client
            .post(PUBLISHED_FILE_DETAILS_URL)
            .form(&form)
            .build()
            .map_err(|error| format!("failed to prepare Workshop item details request: {error}"))?;
        let response = read_workshop_response(client, request, preference).await?;
        let payload: Value = serde_json::from_slice(&response.bytes)
            .map_err(|error| format!("failed to decode Steam item lookup response: {error}"))?;

        let Some(items) = payload
            .get("response")
            .and_then(|value| value.get("publishedfiledetails"))
            .and_then(Value::as_array)
        else {
            return Err(String::from(
                "Steam item lookup response did not contain publishedfiledetails.",
            ));
        };

        app_network::record_success(response.url.as_str());
        for item in items {
            if let Some(id) = extract_value_id(item, "publishedfileid") {
                detail_map.insert(id, item.clone());
            }
        }
    }

    Ok(detail_map)
}

async fn fetch_collection_children_map(
    client: &reqwest::Client,
    ids: &[String],
    preference: SourcePreference,
) -> Result<HashMap<String, Vec<String>>, String> {
    let mut collection_map = HashMap::new();
    for chunk in ids.chunks(LOOKUP_CHUNK_SIZE) {
        let mut form = vec![(String::from("collectioncount"), chunk.len().to_string())];
        for (index, id) in chunk.iter().enumerate() {
            form.push((format!("publishedfileids[{index}]"), id.clone()));
        }

        let request = client
            .post(COLLECTION_DETAILS_URL)
            .form(&form)
            .build()
            .map_err(|error| format!("failed to prepare Workshop collection request: {error}"))?;
        let response = read_workshop_response(client, request, preference).await?;
        let payload: Value = serde_json::from_slice(&response.bytes).map_err(|error| {
            format!("failed to decode Steam collection lookup response: {error}")
        })?;

        let Some(collections) = payload
            .get("response")
            .and_then(|value| value.get("collectiondetails"))
            .and_then(Value::as_array)
        else {
            return Err(String::from(
                "Steam collection lookup response did not contain collectiondetails.",
            ));
        };

        app_network::record_success(response.url.as_str());
        for collection in collections {
            let Some(id) = extract_value_id(collection, "publishedfileid") else {
                continue;
            };
            if extract_u64_field(collection, "result") != Some(1) {
                continue;
            }
            if let Some(children) = collection.get("children").and_then(Value::as_array) {
                for child in children {
                    if let (Some(child_id), Some(file_type)) = (
                        extract_value_id(child, "publishedfileid"),
                        extract_u32_field(child, "filetype"),
                    ) {
                        file_types::remember_file_type(&child_id, file_type)?;
                    }
                }
            }

            let child_ids = collection
                .get("children")
                .and_then(Value::as_array)
                .map(|children| {
                    children
                        .iter()
                        .filter_map(|child| extract_value_id(child, "publishedfileid"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if !child_ids.is_empty() || is_collection_detail(collection) {
                collection_map.insert(id, child_ids);
            }
        }
    }

    Ok(collection_map)
}

fn build_lookup_item(
    id: &str,
    published_file_map: &HashMap<String, Value>,
    collection_children_map: &HashMap<String, Vec<String>>,
    child_lookup_map: &HashMap<String, Value>,
) -> Result<SteamWorkshopLookupItem, String> {
    let detail = published_file_map
        .get(id)
        .filter(|detail| extract_u64_field(detail, "result") == Some(1));
    let child_ids = collection_children_map.get(id).cloned().unwrap_or_default();
    // A normal mod may declare required items; children alone do not make it a collection.
    let file_type = if collection_children_map.contains_key(id) {
        Some(2)
    } else {
        detail.and_then(|detail| extract_u32_field(detail, "file_type"))
    };
    let item_kind = file_types::item_kind(file_type);

    let children = child_ids
        .iter()
        .map(|child_id| {
            let detail = child_lookup_map
                .get(child_id)
                .or_else(|| published_file_map.get(child_id));
            let child_type = detail
                .and_then(|detail| extract_u32_field(detail, "file_type"))
                .or(file_types::cached_file_type(child_id)?);
            let child_kind = file_types::item_kind(child_type);
            Ok(SteamWorkshopLookupChild {
                id: child_id.clone(),
                title: detail.and_then(|value| extract_text_field(value, "title")),
                preview_url: detail.and_then(|value| extract_text_field(value, "preview_url")),
                consumer_app_id: detail
                    .and_then(|value| extract_u32_field(value, "consumer_app_id")),
                item_kind: child_kind.to_string(),
                tags: workshop_tags(detail),
                status: if detail
                    .is_some_and(|detail| extract_u64_field(detail, "result") != Some(1))
                {
                    "not_found"
                } else if detail.is_none() || child_type.is_none() {
                    "unverified"
                } else if matches!(child_kind, "item" | "collection") {
                    "resolved"
                } else {
                    "unsupported"
                }
                .to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    // Children beyond the bounded metadata lookup remain unverified, but the
    // manifest reviewer must still be able to traverse their verified parent.
    let unverified_child = children.iter().find(|child| {
        child.status == "unverified"
            && (child_lookup_map.contains_key(&child.id)
                || published_file_map.contains_key(&child.id))
    });
    let message = match detail {
        None => Some(String::from(
            "Steam did not return public Workshop details for this ID.",
        )),
        Some(detail) if file_type.is_none() => Some(extract_text_field(detail, "type_error").unwrap_or_else(|| String::from(
            "Steam has not verified this item's content type. Retry before installing it."
        ))),
        Some(_) if !matches!(item_kind, "item" | "collection") => Some(String::from(
            "This Steam item is not installable game Workshop content. Open it in Steam to inspect its type.",
        )),
        Some(_) if unverified_child.is_some() => Some(unverified_child
            .and_then(|child| child_lookup_map.get(&child.id).or_else(|| published_file_map.get(&child.id)))
            .and_then(|detail| extract_text_field(detail, "type_error"))
            .unwrap_or_else(|| String::from("Some collection item types could not be verified. Retry before installing this collection."))),
        // Keep the metadata traversable by the bounded manifest reviewer. Ordinary
        // install controls must not mistake this one-level response for leaf IDs.
        Some(_) if children.iter().any(|child| child.item_kind == "collection") => {
            Some(String::from(
                "This collection contains nested collections. Inspect the complete collection through the Workshop list importer or add its individual Mods.",
            ))
        }
        Some(_) if child_ids.len() > MAX_CHILD_TITLE_LOOKUPS => Some(format!(
            "Expanded {0} collection entries. Titles were resolved for the first {1} children.",
            child_ids.len(),
            MAX_CHILD_TITLE_LOOKUPS
        )),
        _ => None,
    };

    let description = detail.and_then(|value| extract_text_field(value, "description"));
    let tags = workshop_tags(detail);

    Ok(SteamWorkshopLookupItem {
        id: id.to_string(),
        title: detail.and_then(|value| extract_text_field(value, "title")),
        preview_url: detail.and_then(|value| extract_text_field(value, "preview_url")),
        description_excerpt: description.as_deref().map(summarize_description),
        description,
        detail_url: format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"),
        item_kind: item_kind.to_string(),
        status: if detail.is_none() {
            "not_found"
        } else if file_type.is_none() || unverified_child.is_some() {
            "unverified"
        } else if matches!(item_kind, "item" | "collection") {
            "resolved"
        } else {
            "unsupported"
        }
        .to_string(),
        message,
        localization_warning: None,
        consumer_app_id: detail.and_then(|value| extract_u32_field(value, "consumer_app_id")),
        creator_app_id: detail.and_then(|value| extract_u32_field(value, "creator_app_id")),
        creator_id: detail.and_then(|value| extract_value_id(value, "creator")),
        file_size: detail.and_then(|value| extract_u64_field(value, "file_size")),
        created_at_unix: detail.and_then(|value| extract_u64_field(value, "time_created")),
        updated_at_unix: detail.and_then(|value| extract_u64_field(value, "time_updated")),
        subscriptions: detail.and_then(|value| extract_u64_field(value, "subscriptions")),
        favorites: detail.and_then(|value| extract_u64_field(value, "favorited")),
        views: detail.and_then(|value| extract_u64_field(value, "views")),
        tags,
        child_count: child_ids.len(),
        children,
    })
}

fn workshop_tags(detail: Option<&Value>) -> Vec<String> {
    detail
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| extract_text_field(tag, "tag"))
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_lookup_ids(ids: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw in ids {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.len() < 6
            || trimmed.len() > 20
            || !trimmed.bytes().all(|byte| byte.is_ascii_digit())
            || !trimmed.parse::<u64>().is_ok_and(|id| id > 0)
        {
            return Err(format!("`{trimmed}` is not a valid Workshop ID."));
        }
        if seen.insert(trimmed.to_string()) {
            normalized.push(trimmed.to_string());
        }
    }

    if normalized.len() > MAX_LOOKUP_IDS {
        return Err(format!(
            "Workshop lookup is limited to {MAX_LOOKUP_IDS} IDs per request."
        ));
    }

    Ok(normalized)
}

fn extract_text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn extract_u32_field(value: &Value, key: &str) -> Option<u32> {
    if let Some(number) = value.get(key).and_then(Value::as_u64) {
        return u32::try_from(number).ok();
    }

    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<u32>().ok())
}

fn extract_u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|field| {
        field
            .as_u64()
            .or_else(|| field.as_str().and_then(|text| text.parse::<u64>().ok()))
    })
}

fn extract_value_id(value: &Value, key: &str) -> Option<String> {
    if let Some(text) = value.get(key).and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    value
        .get(key)
        .and_then(Value::as_u64)
        .map(|number| number.to_string())
}

fn summarize_description(raw: &str) -> String {
    let collapsed = strip_workshop_markup(raw)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.chars().count() <= 180 {
        return collapsed;
    }

    let mut excerpt = collapsed.chars().take(177).collect::<String>();
    excerpt.push_str("...");
    excerpt
}

fn strip_workshop_markup(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut inside_tag = false;
    for character in raw.chars() {
        match character {
            '[' => inside_tag = true,
            ']' if inside_tag => inside_tag = false,
            _ if !inside_tag => result.push(character),
            _ => {}
        }
    }
    result
}

fn is_collection_detail(value: &Value) -> bool {
    value
        .get("children")
        .and_then(Value::as_array)
        .is_some_and(|children| !children.is_empty())
        || value
            .get("result")
            .and_then(Value::as_u64)
            .is_some_and(|result| result == 1)
            && value.get("children").is_some()
}

#[cfg(test)]
#[path = "steam_workshop/lookup_tests.rs"]
mod tests;
