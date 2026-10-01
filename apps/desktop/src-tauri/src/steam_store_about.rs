//! Steam store descriptions, including identity validation before rendering.

use std::{future::Future, time::Duration};

use serde_json::Value;

#[path = "steam_store_browse.rs"]
mod browse;
#[path = "steam_store_description.rs"]
mod description;

pub(super) async fn fetch(app_id: u64, locale: Option<&str>) -> Result<Option<String>, String> {
    if app_id == 0 || app_id > u32::MAX.into() {
        return Err("Steam store app ID must be a positive 32-bit integer".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!(
            "LanGame Server Manager/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| error.to_string())?;
    fetch_providers(Duration::from_secs(16), |primary, budget| {
        let client = &client;
        async move {
            if primary {
                browse::fetch(client, app_id, locale, budget).await
            } else {
                fetch_appdetails(client, app_id, locale, budget)
                    .await
                    .map_err(AboutError::Invalid)
            }
        }
    })
    .await
}

async fn fetch_appdetails(
    client: &reqwest::Client,
    app_id: u64,
    locale: Option<&str>,
    budget: Duration,
) -> Result<Option<String>, String> {
    fetch_languages(locale, budget, |country, language, budget| {
        let client = &client;
        async move {
            let request = client
                .get("https://store.steampowered.com/api/appdetails")
                .query(&[
                    ("appids", app_id.to_string()),
                    ("cc", country.into()),
                    ("l", language.into()),
                ])
                .build()
                .map_err(|error| AboutError::Invalid(error.to_string()))?;
            let response = app_network::read_public_bytes(
                client,
                request,
                budget,
                8 * 1024 * 1024,
                app_network::SourcePreference::from_locale(locale),
            )
            .await
            .map_err(AboutError::Network)?;
            let payload: Value = serde_json::from_slice(&response.bytes).map_err(|error| {
                AboutError::Invalid(format!(
                    "Invalid Steam store response for app {app_id}: {error}"
                ))
            })?;
            let about = parse_about(&payload, app_id, language == "schinese")
                .map_err(AboutError::Invalid)?;
            app_network::record_success(response.url.as_str());
            Ok(about)
        }
    })
    .await
}

#[derive(Debug)]
enum AboutError {
    Network(app_network::NetworkError),
    Invalid(String),
    Unavailable(String),
}

impl AboutError {
    fn permits_fallback(&self) -> bool {
        match self {
            Self::Network(error) => error.permits_source_fallback(),
            Self::Invalid(_) => true,
            Self::Unavailable(_) => false,
        }
    }

    fn message(self) -> String {
        match self {
            Self::Network(error) => error.to_string(),
            Self::Invalid(message) | Self::Unavailable(message) => message,
        }
    }
}

/// The region-aware full-description API owns the first half of the total
/// budget. The older store endpoint remains a bounded recovery path, never a
/// way around authorization, throttling, or an explicitly unavailable item.
async fn fetch_providers<F, Fut>(budget: Duration, mut read: F) -> Result<Option<String>, String>
where
    F: FnMut(bool, Duration) -> Fut,
    Fut: Future<Output = Result<Option<String>, AboutError>>,
{
    let deadline = tokio::time::Instant::now() + budget;
    let mut last_error = None;
    for primary in [true, false] {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let attempt_budget = if primary { remaining / 2 } else { remaining };
        if attempt_budget.is_zero() {
            break;
        }
        let result = tokio::time::timeout(attempt_budget, read(primary, attempt_budget))
            .await
            .unwrap_or_else(|_| {
                Err(AboutError::Network(app_network::NetworkError::Deadline {
                    attempts: usize::from(!primary) + 1,
                    origin: if primary {
                        "Steam public store description"
                    } else {
                        "https://store.steampowered.com"
                    }
                    .into(),
                }))
            });
        match result {
            Ok(Some(html)) => return Ok(Some(html)),
            Ok(None) => {}
            Err(error) => {
                let fallback = error.permits_fallback();
                let message = error.message();
                if !fallback {
                    return Err(message);
                }
                last_error = Some(message);
            }
        }
    }
    if tokio::time::Instant::now() >= deadline && last_error.is_none() {
        return Err("Steam store description request timed out".into());
    }
    last_error.map_or(Ok(None), Err)
}

/// Both content-language attempts share one deadline. An unavailable/invalid
/// localized response must not suppress a valid alternative, while a server's
/// authorization or retry restriction still applies to the whole operation.
async fn fetch_languages<F, Fut>(
    locale: Option<&str>,
    budget: Duration,
    mut read: F,
) -> Result<Option<String>, String>
where
    F: FnMut(&'static str, &'static str, Duration) -> Fut,
    Fut: Future<Output = Result<Option<String>, AboutError>>,
{
    let chinese = app_network::SourcePreference::from_locale(locale)
        == app_network::SourcePreference::ChinaFirst;
    let locales = if chinese {
        [("cn", "schinese"), ("us", "english")]
    } else {
        [("us", "english"), ("cn", "schinese")]
    };
    let deadline = tokio::time::Instant::now() + budget;
    let mut last_error = None;
    for (index, (country, language)) in locales.into_iter().enumerate() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let attempt_budget = (remaining / (2 - index) as u32).min(Duration::from_secs(8));
        if attempt_budget.is_zero() {
            return Err(
                last_error.unwrap_or_else(|| "Steam store description request timed out".into())
            );
        }
        let result = tokio::time::timeout(attempt_budget, read(country, language, attempt_budget))
            .await
            .unwrap_or_else(|_| {
                Err(AboutError::Network(app_network::NetworkError::Deadline {
                    attempts: index + 1,
                    origin: "https://store.steampowered.com".into(),
                }))
            });
        match result {
            Ok(Some(html)) => return Ok(Some(html)),
            Ok(None) => {}
            Err(error) => {
                let fallback = error.permits_fallback();
                let message = error.message();
                if !fallback {
                    return Err(message);
                }
                last_error = Some(message);
            }
        }
    }
    last_error.map_or(Ok(None), Err)
}

fn parse_about(payload: &Value, app_id: u64, chinese: bool) -> Result<Option<String>, String> {
    let entries = payload
        .as_object()
        .ok_or_else(|| invalid_identity(app_id))?;
    // Some live AppDetails responses use another ID as the envelope key while
    // retaining the requested game's steam_appid in data. Never trust the first
    // entry or accept another game's description just because its key matches.
    let mut matching = entries.values().filter(|entry| {
        entry.get("success").and_then(Value::as_bool) == Some(true)
            && entry
                .get("data")
                .and_then(|data| data.get("steam_appid"))
                .and_then(Value::as_u64)
                == Some(app_id)
    });
    let Some(entry) = matching.next() else {
        return if entries
            .get(&app_id.to_string())
            .and_then(|entry| entry.get("success"))
            .and_then(Value::as_bool)
            == Some(false)
        {
            Ok(None)
        } else {
            Err(invalid_identity(app_id))
        };
    };
    if matching.next().is_some() {
        return Err(invalid_identity(app_id));
    }
    let data = &entry["data"];
    let text = |key| {
        data.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let mut html = String::new();
    if let Some(reviews) = text("reviews") {
        html.push_str(if chinese {
            "<h2>媒体评价</h2>"
        } else {
            "<h2>Reviews</h2>"
        });
        html.push_str(r#"<div class="steam-review-copy">"#);
        html.push_str(reviews);
        html.push_str("</div>");
    }
    if let Some(about) = text("about_the_game") {
        if !html.is_empty() {
            html.push_str("<hr />");
        }
        html.push_str(about);
    }
    Ok((!html.is_empty()).then_some(html))
}

fn invalid_identity(app_id: u64) -> String {
    format!("Steam store response did not uniquely identify requested app {app_id}")
}

#[cfg(test)]
#[path = "steam_store_about_tests.rs"]
mod tests;
