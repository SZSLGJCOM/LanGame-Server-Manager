//! Locale-specific presentation for the one Workshop item being viewed.
//! The anonymous batch API ignores language; never expand a library into HTML reads.

use std::collections::HashMap;
use std::time::Duration;

use dom_query::Document;
use serde_json::Value;

use super::{
    MAX_RESPONSE_BYTES, SourcePreference, SteamWorkshopLookupItem, extract_u64_field,
    fetch_lookup_metadata, file_types, finish_lookup_items, normalize_lookup_ids, workshop_client,
    workshop_language,
};

struct LocalizedText {
    title: String,
    description: Option<String>,
    child_titles: HashMap<String, String>,
    file_type: Option<u32>,
}

pub async fn read_public_workshop_item_details(
    id: String,
    locale: Option<&str>,
) -> Result<SteamWorkshopLookupItem, String> {
    let ids = normalize_lookup_ids(vec![id])?;
    let id = ids.first().ok_or("A Workshop item ID is required.")?;
    let client = workshop_client()?;
    let preference = SourcePreference::from_locale(locale);
    tokio::time::timeout(Duration::from_secs(45), async {
        // The page also supplies type evidence, so the root does not need a second
        // Community request during the existing best-effort type verification phase.
        let (metadata, text) = tokio::join!(
            tokio::time::timeout(
                Duration::from_secs(30),
                fetch_lookup_metadata(&client, &ids, preference)
            ),
            fetch_localized_text(&client, id, locale, preference)
        );
        let (details, collections) = metadata.map_err(|_| {
            String::from("Steam Workshop metadata lookup timed out. Please retry.")
        })??;
        finish_localized_details(&client, id, preference, details, collections, text).await
    })
    .await
    .map_err(|_| String::from("Steam Workshop details lookup timed out. Please retry."))?
}

async fn finish_localized_details(
    client: &reqwest::Client,
    id: &str,
    preference: SourcePreference,
    mut details: HashMap<String, Value>,
    collections: HashMap<String, Vec<String>>,
    text: Result<LocalizedText, String>,
) -> Result<SteamWorkshopLookupItem, String> {
    let localization_warning = if details
        .get(id)
        .and_then(|detail| extract_u64_field(detail, "result"))
        == Some(1)
    {
        match text {
            Ok(text) => match apply_localized_text(&mut details, id, &text) {
                Ok(()) => {
                    if let Some(file_type) = text.file_type {
                        file_types::remember_file_type(id, file_type)?;
                    }
                    None
                }
                Err(error) => Some(error),
            },
            Err(error) => Some(error),
        }
    } else {
        None
    };
    // The public API remains authoritative for existence and identity. HTML
    // failures retain its original text with an explicit presentation warning;
    // unknown types stay unverified and never gain download permission.
    let mut item = finish_lookup_items(
        client,
        &[id.to_string()],
        preference,
        details,
        collections,
        localization_warning.as_deref(),
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| invalid_page("metadata omitted the requested item"))?;
    item.localization_warning = localization_warning;
    Ok(item)
}

async fn fetch_localized_text(
    client: &reqwest::Client,
    id: &str,
    locale: Option<&str>,
    preference: SourcePreference,
) -> Result<LocalizedText, String> {
    let request = client
        .get("https://steamcommunity.com/sharedfiles/filedetails/")
        .query(&[("id", id), ("l", workshop_language(locale))])
        .build()
        .map_err(|error| format!("failed to prepare localized Workshop details: {error}"))?;
    let response = super::community::read(
        client,
        request,
        Duration::from_secs(20),
        MAX_RESPONSE_BYTES,
        preference,
    )
    .await
    .map_err(|error| super::network_error::workshop_network_error("details", &error))?;
    let requested_id = id.to_string();
    let response_url = response.url.to_string();
    let text = tokio::task::spawn_blocking(move || {
        let html =
            std::str::from_utf8(&response.bytes).map_err(|_| invalid_page("page is not UTF-8"))?;
        parse_localized_text(html, &requested_id)
    })
    .await
    .map_err(|error| format!("Workshop text parser failed: {error}"))??;
    app_network::record_success(&response_url);
    Ok(text)
}

fn parse_localized_text(html: &str, id: &str) -> Result<LocalizedText, String> {
    let document = Document::from(html);
    // User descriptions are never an identity source, and hidden/executable media
    // must not become visible plain text. The DOM parser decodes HTML entities.
    document.select("#highlightContent script, #highlightContent style, #highlightContent iframe, #highlightContent noscript").remove();
    let identities = document
        .select("script")
        .iter()
        .flat_map(|script| script_item_identities(&script.text()))
        .collect::<Vec<_>>();
    if identities.len() != 1 || identities[0] != id {
        return Err(invalid_page(
            "page does not uniquely identify the requested item",
        ));
    }
    let title = document.select(".workshopItemDetailsHeader > .workshopItemTitle");
    let description = document.select(".workshopItemDescription#highlightContent");
    if title.length() != 1 || description.length() > 1 {
        return Err(invalid_page(
            "item title or description container is missing or ambiguous",
        ));
    }
    let title = title.text().trim().to_string();
    let description = (description.length() == 1).then(|| description.formatted_text().to_string());
    if title.len() > 16 * 1024
        || description
            .as_ref()
            .is_some_and(|text| text.len() > 1024 * 1024)
    {
        return Err(invalid_page("item text exceeds its supported limit"));
    }
    let mut child_titles = HashMap::new();
    let children = document.select("#mainContentsCollection .collectionChildren > .collectionItem");
    if children.length() > 8192 {
        return Err(invalid_page(
            "collection exceeds its supported member limit",
        ));
    }
    for child in children.iter() {
        let Some(child_id) = child
            .attr("id")
            .and_then(|value| value.strip_prefix("sharedfile_").map(str::to_string))
        else {
            continue;
        };
        if normalize_lookup_ids(vec![child_id.clone()]).is_err() {
            continue;
        }
        let title = child.select(".collectionItemDetails > a > .workshopItemTitle");
        if title.length() == 1 {
            let title = title.text().trim().to_string();
            if title.len() > 16 * 1024 {
                return Err(invalid_page("member title exceeds its supported limit"));
            }
            child_titles.insert(child_id, title);
        }
    }
    Ok(LocalizedText {
        title: if title.is_empty() {
            id.to_string()
        } else {
            title
        },
        description,
        child_titles,
        file_type: file_types::parse_file_type(html, id).ok(),
    })
}

fn script_item_identities(script: &str) -> Vec<String> {
    // Item pages put bSkipVideos and SESSION_ID before publishedfileid, whereas
    // collection pages can begin with publishedfileid. Read only the leading
    // static declarations, never matches inside comments, strings or functions.
    let mut source = script.trim();
    let mut identities = Vec::new();
    for _ in 0..16 {
        let Some(rest) = source
            .strip_prefix("var")
            .filter(|rest| rest.starts_with(char::is_whitespace))
        else {
            break;
        };
        let rest = rest.trim_start();
        let name_end = rest
            .find(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_' && character != '$'
            })
            .unwrap_or(rest.len());
        let (name, rest) = rest.split_at(name_end);
        let Some(value) = rest.trim_start().strip_prefix('=').map(str::trim_start) else {
            break;
        };
        let (literal, remaining, quoted) = if let Some(quote @ ('\'' | '"')) = value.chars().next()
        {
            let Some(end) = value[1..].find(quote).map(|index| index + 1) else {
                break;
            };
            let literal = &value[1..end];
            if literal.contains(['\\', '\n', '\r']) {
                break;
            }
            (literal, &value[end + 1..], true)
        } else {
            let Some((literal, _)) = value.split_once(';') else {
                break;
            };
            let literal = literal.trim_end();
            if !matches!(literal, "true" | "false")
                && (literal.is_empty() || !literal.bytes().all(|byte| byte.is_ascii_digit()))
            {
                break;
            }
            (literal, &value[literal.len()..], false)
        };
        let Some(rest) = remaining.trim_start().strip_prefix(';') else {
            break;
        };
        if name == "publishedfileid" {
            if !quoted {
                return Vec::new();
            }
            identities.push(literal.to_string());
        }
        source = rest.trim_start();
    }
    identities
}

fn apply_localized_text(
    details: &mut HashMap<String, Value>,
    id: &str,
    text: &LocalizedText,
) -> Result<(), String> {
    let root = details
        .get_mut(id)
        .ok_or_else(|| invalid_page("metadata omitted the requested item"))?;
    if text.description.is_none()
        && root
            .get("description")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    {
        return Err(invalid_page(
            "the nonempty item description container is missing",
        ));
    }
    root["title"] = Value::from(text.title.clone());
    root["description"] = Value::from(text.description.clone().unwrap_or_default());
    for (child_id, title) in &text.child_titles {
        if child_id != id
            && let Some(child) = details.get_mut(child_id)
            && extract_u64_field(child, "result") == Some(1)
        {
            child["title"] = Value::from(title.clone());
        }
    }
    Ok(())
}

fn invalid_page(reason: &str) -> String {
    serde_json::json!({
        "code": "steam_workshop_details_unrecognized_response",
        "message": format!("Steam Workshop returned unrecognized item details: {reason}.")
    })
    .to_string()
}

#[cfg(test)]
#[path = "localized_details_tests.rs"]
mod tests;
