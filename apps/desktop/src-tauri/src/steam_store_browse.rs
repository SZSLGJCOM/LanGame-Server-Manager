//! Full public store descriptions over verified regional Steam API origins.

use std::time::Duration;

use serde_json::{Value, json};

use super::{AboutError, description};

const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

fn request_url(app_id: u64, locale: Option<&str>) -> reqwest::Url {
    let chinese = app_network::SourcePreference::from_locale(locale)
        == app_network::SourcePreference::ChinaFirst;
    let input = json!({
        "ids": [{"appid": app_id}],
        "context": {
            "language": if chinese { "schinese" } else { "english" },
            "country_code": if chinese { "CN" } else { "US" },
        },
        "data_request": {"include_full_description": true},
    });
    let mut url =
        reqwest::Url::parse("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/")
            .expect("fixed official Steam API URL");
    url.query_pairs_mut()
        .append_pair("input_json", &input.to_string());
    url
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    app_id: u64,
    locale: Option<&str>,
    budget: Duration,
) -> Result<Option<String>, AboutError> {
    let request = request_url(app_id, locale);
    let candidates = app_network::official_url_candidates(
        request.as_str(),
        app_network::SourcePreference::from_locale(locale),
    );
    fetch_candidates(client, app_id, &candidates, budget).await
}

async fn fetch_candidates(
    client: &reqwest::Client,
    app_id: u64,
    candidates: &[String],
    budget: Duration,
) -> Result<Option<String>, AboutError> {
    let deadline = tokio::time::Instant::now() + budget;
    let mut last_error = None;
    for (index, url) in candidates.iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let source_budget = remaining / (candidates.len() - index) as u32;
        let request = client
            .get(url)
            .build()
            .map_err(|error| AboutError::Invalid(error.to_string()))?;
        let result = async {
            let response = app_network::read_public_bytes_from_source(
                client,
                request,
                source_budget,
                RESPONSE_LIMIT,
            )
            .await
            .map_err(AboutError::Network)?;
            parse_description(&response.bytes, app_id)
        }
        .await;
        match result {
            Ok(Some(html)) => {
                app_network::record_success(url);
                return Ok(Some(html));
            }
            Ok(None) => {}
            Err(error) => {
                if !error.permits_fallback() {
                    return Err(error);
                }
                app_network::record_failure(url);
                last_error = Some(error);
            }
        }
    }
    if let Some(error) = last_error {
        return Err(error);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(AboutError::Network(app_network::NetworkError::Deadline {
            attempts: candidates.len(),
            origin: "Steam public store description".into(),
        }));
    }
    Ok(None)
}

fn parse_description(bytes: &[u8], app_id: u64) -> Result<Option<String>, AboutError> {
    let payload: Value = serde_json::from_slice(bytes).map_err(|error| {
        AboutError::Invalid(format!("Invalid Steam store browse response: {error}"))
    })?;
    let invalid = || {
        AboutError::Invalid(format!(
            "Steam store browse response did not uniquely identify requested app {app_id}"
        ))
    };
    let items = payload
        .get("response")
        .and_then(|response| response.get("store_items"))
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if items.len() != 1 {
        return Err(invalid());
    }
    let item = &items[0];
    if item.get("appid").and_then(Value::as_u64) != Some(app_id)
        || item.get("id").and_then(Value::as_u64) != Some(app_id)
        || item.get("item_type").and_then(Value::as_u64) != Some(0)
    {
        return Err(invalid());
    }
    if item.get("success").and_then(Value::as_u64) != Some(1)
        || item.get("visible").and_then(Value::as_bool) != Some(true)
    {
        return Err(AboutError::Unavailable(format!(
            "Steam store description for app {app_id} is unavailable"
        )));
    }
    let Some(value) = item.get("full_description_bbcode") else {
        return Ok(None);
    };
    let text = value
        .as_str()
        .ok_or_else(|| AboutError::Invalid("Steam full description is not text".into()))?;
    description::render(text, app_id).map_err(AboutError::Invalid)
}

#[cfg(test)]
#[path = "steam_store_browse_tests.rs"]
mod tests;
