use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::{
    SteamWorkshopLookupItem, build_lookup_item, extract_text_field, extract_u32_field,
    extract_u64_field, extract_value_id, normalize_lookup_ids, read_public_workshop_item_details,
    read_workshop_response, summarize_description, workshop_client, workshop_language,
};

const WORKSHOP_BROWSE_URL: &str = "https://steamcommunity.com/workshop/browse/";
const SEARCH_PAGE_SIZE: u32 = 30;

#[derive(Debug, Clone, Serialize)]
pub struct SteamWorkshopSearchResult {
    pub app_id: u32,
    pub browse_kind: String,
    pub query: String,
    pub sort: String,
    pub page: u32,
    pub page_size: u32,
    pub total_count: Option<u64>,
    pub has_more: bool,
    pub source_url: String,
    pub items: Vec<SteamWorkshopLookupItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SteamWorkshopSearchSort {
    Relevance,
    Trend,
    Popular,
    Recent,
    Subscribers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SteamWorkshopBrowseKind {
    Item,
    Collection,
}

impl SteamWorkshopBrowseKind {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).unwrap_or("item") {
            "item" => Ok(Self::Item),
            "collection" => Ok(Self::Collection),
            _ => Err("Steam Workshop browse kind must be item or collection.".into()),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Collection => "collection",
        }
    }

    fn section(self) -> &'static str {
        match self {
            Self::Item => "readytouseitems",
            Self::Collection => "collections",
        }
    }
}

pub async fn search_public_workshop_items(
    app_id: u32,
    query: Option<String>,
    sort: Option<String>,
    page: Option<u32>,
    locale: Option<String>,
    browse_kind: Option<String>,
) -> Result<SteamWorkshopSearchResult, String> {
    let browse_kind = SteamWorkshopBrowseKind::parse(browse_kind.as_deref())?;
    if app_id == 0 {
        return Err(String::from(
            "Steam Workshop search requires a valid app id.",
        ));
    }
    let raw_query = query.unwrap_or_default();
    let raw_query = raw_query.trim();
    let query = raw_query.chars().take(120).collect::<String>();
    let sort = normalize_search_sort(sort.as_deref(), query.is_empty());
    validate_search_sort(sort, browse_kind)?;
    let page = page.unwrap_or(1).max(1);
    let preference = app_network::SourcePreference::from_locale(locale.as_deref());
    if let Some(id) = exact_workshop_id(raw_query)? {
        let items = std::iter::once(
            read_public_workshop_item_details(id.clone(), locale.as_deref()).await?,
        )
        .filter(|item| matches_exact_item(item, app_id, browse_kind))
        .collect::<Vec<_>>();
        return Ok(SteamWorkshopSearchResult {
            app_id,
            browse_kind: browse_kind.as_str().to_string(),
            query,
            sort: sort.as_str().to_string(),
            page,
            page_size: SEARCH_PAGE_SIZE,
            total_count: Some(items.len() as u64),
            has_more: false,
            source_url: format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"),
            items: if page == 1 { items } else { Vec::new() },
        });
    }

    let client = workshop_client()?;
    let request = build_search_request(
        &client,
        app_id,
        &query,
        sort,
        page,
        workshop_language(locale.as_deref()),
        browse_kind,
    )?;
    let source_url = request.url().to_string();
    let response = read_workshop_response(&client, request, preference).await?;
    let html = std::str::from_utf8(&response.bytes)
        .map_err(|_| unrecognized_browse_response("response is not UTF-8"))?;
    let (items, total_count, has_more) =
        parse_browse_page(html, app_id, &query, sort, page, browse_kind)?;
    app_network::record_success(response.url.as_str());
    Ok(SteamWorkshopSearchResult {
        app_id,
        browse_kind: browse_kind.as_str().to_string(),
        query,
        sort: sort.as_str().to_string(),
        page,
        page_size: SEARCH_PAGE_SIZE,
        total_count: Some(total_count),
        has_more,
        source_url,
        items,
    })
}

fn matches_exact_item(
    item: &SteamWorkshopLookupItem,
    app_id: u32,
    browse_kind: SteamWorkshopBrowseKind,
) -> bool {
    item.status != "not_found"
        && item.consumer_app_id == Some(app_id)
        && (item.item_kind == browse_kind.as_str()
            // An explicit ID can show verified identity with pending type evidence.
            // Do not disguise an unavailable Community lookup as zero search results.
            || (item.status == "unverified" && item.item_kind == "unknown"))
}

fn build_search_request(
    client: &reqwest::Client,
    app_id: u32,
    query: &str,
    sort: SteamWorkshopSearchSort,
    page: u32,
    language: &str,
    browse_kind: SteamWorkshopBrowseKind,
) -> Result<reqwest::Request, String> {
    let mut params = vec![
        ("appid", app_id.to_string()),
        ("section", browse_kind.section().to_string()),
        ("browsesort", sort.browse_sort().to_string()),
        ("actualsort", sort.browse_sort().to_string()),
        ("p", page.to_string()),
        ("numperpage", SEARCH_PAGE_SIZE.to_string()),
        ("l", language.to_string()),
    ];
    if sort == SteamWorkshopSearchSort::Trend {
        params.push(("days", String::from("7")));
    }
    if !query.is_empty() {
        params.push(("searchtext", query.to_string()));
    }
    client
        .get(WORKSHOP_BROWSE_URL)
        .query(&params)
        .build()
        .map_err(|error| format!("failed to build Steam Workshop browse request: {error}"))
}

fn exact_workshop_id(query: &str) -> Result<Option<String>, String> {
    let id = if !query.is_empty() && query.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(query.to_string())
    } else {
        reqwest::Url::parse(query).ok().and_then(|url| {
            if !matches!(url.scheme(), "http" | "https")
                || !matches!(
                    url.host_str(),
                    Some("steamcommunity.com" | "www.steamcommunity.com")
                )
                || url.path().trim_end_matches('/') != "/sharedfiles/filedetails"
            {
                return None;
            }
            url.query_pairs()
                .find(|(key, _)| key == "id")
                .map(|(_, id)| id.into_owned())
        })
    };
    id.map(|id| normalize_lookup_ids(vec![id]).map(|mut ids| ids.pop()))
        .transpose()
        .map(Option::flatten)
}

fn parse_browse_page(
    html: &str,
    app_id: u32,
    query: &str,
    sort: SteamWorkshopSearchSort,
    page: u32,
    browse_kind: SteamWorkshopBrowseKind,
) -> Result<(Vec<SteamWorkshopLookupItem>, u64, bool), String> {
    let context = parse_browse_context(html)?;
    let query_data: Value = serde_json::from_str(
        context
            .get("queryData")
            .and_then(Value::as_str)
            .ok_or_else(|| unrecognized_browse_response("SSR query data is missing"))?,
    )
    .map_err(|_| unrecognized_browse_response("SSR query data is invalid JSON"))?;
    let catalog_query = query_data
        .get("queries")
        .and_then(Value::as_array)
        .and_then(|queries| {
            queries.iter().find(|entry| {
                let Some(key) = entry.get("queryKey").and_then(Value::as_array) else {
                    return false;
                };
                let Some(params) = key.get(1) else {
                    return false;
                };
                key.first().and_then(Value::as_str) == Some("workshop_browse")
                    && extract_u32_field(params, "appid") == Some(app_id)
                    && extract_u32_field(params, "page") == Some(page)
                    && extract_u32_field(params, "num_per_page") == Some(SEARCH_PAGE_SIZE)
                    && params.get("search_text").and_then(Value::as_str) == Some(query)
                    && params.get("browse_sort").and_then(Value::as_str) == Some(sort.browse_sort())
                    && params.get("section").and_then(Value::as_str) == Some(browse_kind.section())
            })
        })
        .ok_or_else(|| {
            unrecognized_browse_response(
                "catalog query does not match the requested app, section, sort, search or page",
            )
        })?;
    let data = catalog_query
        .pointer("/state/data")
        .ok_or_else(|| unrecognized_browse_response("catalog data is missing"))?;
    if extract_u64_field(data, "eresult") != Some(1) {
        return Err(unrecognized_browse_response(
            "catalog did not report a successful eresult",
        ));
    }
    let total_count = extract_u64_field(data, "total_count")
        .ok_or_else(|| unrecognized_browse_response("catalog total_count is missing"))?;
    let total_pages = extract_u64_field(data, "total_pages")
        .ok_or_else(|| unrecognized_browse_response("catalog total_pages is missing"))?;
    let results = data
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| unrecognized_browse_response("catalog results are missing"))?;
    if extract_u32_field(data, "current_page") != Some(page)
        || results.len() > SEARCH_PAGE_SIZE as usize
        || (total_count == 0 && !results.is_empty())
        || (total_count > 0 && u64::from(page) <= total_pages && results.is_empty())
    {
        return Err(unrecognized_browse_response(
            "catalog pagination or result count is inconsistent",
        ));
    }
    let mut items = results
        .iter()
        .map(|detail| build_browse_item(detail, app_id))
        .collect::<Result<Vec<_>, _>>()?;
    items.retain(|item| item.status == "resolved" && item.item_kind == browse_kind.as_str());
    Ok((items, total_count, u64::from(page) < total_pages))
}

fn parse_browse_context(html: &str) -> Result<Value, String> {
    // Hydration supplies the ordered catalog; page anchors also include tutorials
    // and promotions. Parse JSON as data without evaluating any page scripts.
    let document = dom_query::Document::from(html);
    let hydration = document.select("script#valve-ssr-data");
    if !hydration.is_empty() {
        if hydration.length() != 1
            || !hydration
                .attr("type")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        {
            return Err(unrecognized_browse_response(
                "SSR hydration container is ambiguous or has an invalid type",
            ));
        }
        // Script text is raw JSON, so HTML entities must remain literal. A present
        // but damaged new container must not be hidden by an older assignment.
        let mut data: Value = serde_json::from_str(&hydration.text())
            .map_err(|_| unrecognized_browse_response("SSR hydration data is invalid JSON"))?;
        return data
            .get_mut("renderContext")
            .map(Value::take)
            .ok_or_else(|| unrecognized_browse_response("SSR hydration context is missing"));
    }

    // Retain the previous response format for pages served during CDN rollout.
    let encoded = html
        .split_once("window.SSR.renderContext")
        .and_then(|(_, rest)| rest.trim_start().strip_prefix('='))
        .and_then(|rest| rest.trim_start().strip_prefix("JSON.parse("))
        .ok_or_else(|| unrecognized_browse_response("SSR hydration assignment is missing"))?;
    let context_json = serde_json::Deserializer::from_str(encoded)
        .into_iter::<String>()
        .next()
        .ok_or_else(|| unrecognized_browse_response("SSR hydration string is missing"))?
        .map_err(|_| unrecognized_browse_response("SSR hydration string is invalid JSON"))?;
    serde_json::from_str(&context_json)
        .map_err(|_| unrecognized_browse_response("SSR hydration context is invalid JSON"))
}

fn build_browse_item(detail: &Value, app_id: u32) -> Result<SteamWorkshopLookupItem, String> {
    let id = extract_value_id(detail, "publishedfileid")
        .ok_or_else(|| unrecognized_browse_response("catalog item ID is missing"))?;
    normalize_lookup_ids(vec![id.clone()])
        .map_err(|_| unrecognized_browse_response("catalog item ID is invalid"))?;
    if extract_u32_field(detail, "consumer_appid") != Some(app_id)
        || !detail.get("title").is_some_and(Value::is_string)
    {
        return Err(unrecognized_browse_response(
            "catalog item has a mismatched app or invalid title field",
        ));
    }
    let file_type = extract_u32_field(detail, "file_type")
        .ok_or_else(|| unrecognized_browse_response("catalog item file_type is missing"))?;
    super::file_types::remember_file_type(&id, file_type)?;
    let mut normalized = detail.clone();
    normalized["result"] = Value::from(1);
    normalized["consumer_app_id"] = Value::from(app_id);
    let published = HashMap::from([(id.clone(), normalized)]);
    let mut item = build_lookup_item(&id, &published, &HashMap::new(), &HashMap::new())?;
    // Steam permits an empty published title; identity and type still establish
    // a real catalog item. Keep it in order instead of rejecting the whole page.
    if item.title.is_none() {
        item.title = Some(id);
    }
    item.description = None;
    item.description_excerpt = extract_text_field(detail, "short_description")
        .map(|description| summarize_description(&description));
    item.child_count = extract_u64_field(detail, "num_children")
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);
    Ok(item)
}

fn unrecognized_browse_response(reason: &str) -> String {
    serde_json::json!({
        "code": "steam_workshop_browse_unrecognized_response",
        "message": format!("Steam Workshop returned an unrecognized or incomplete browse response: {reason}."),
    }).to_string()
}

fn validate_search_sort(
    sort: SteamWorkshopSearchSort,
    kind: SteamWorkshopBrowseKind,
) -> Result<(), String> {
    // Collection subscriptions are not a catalog sort. Steam rewrites this to
    // `accepted` and returns an unrelated empty catalog instead of rejecting it.
    if kind == SteamWorkshopBrowseKind::Collection && sort == SteamWorkshopSearchSort::Subscribers {
        return Err(serde_json::json!({
            "code": "steam_workshop_browse_unsupported_sort",
            "message": "Steam Workshop collections do not support subscriber sorting.",
            "sort": sort.as_str(),
            "browse_kind": kind.as_str(),
        })
        .to_string());
    }
    Ok(())
}

fn normalize_search_sort(value: Option<&str>, empty_query: bool) -> SteamWorkshopSearchSort {
    let sort = match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "relevance" | "textsearch" => SteamWorkshopSearchSort::Relevance,
        "trend" => SteamWorkshopSearchSort::Trend,
        "popular" | "toprated" | "top_rated" => SteamWorkshopSearchSort::Popular,
        "recent" | "new" | "mostrecent" => SteamWorkshopSearchSort::Recent,
        "subscribers" | "subscribed" | "subscriptions" => SteamWorkshopSearchSort::Subscribers,
        _ if !empty_query => SteamWorkshopSearchSort::Relevance,
        _ => SteamWorkshopSearchSort::Trend,
    };
    if empty_query && sort == SteamWorkshopSearchSort::Relevance {
        SteamWorkshopSearchSort::Trend
    } else {
        sort
    }
}

impl SteamWorkshopSearchSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Trend => "trend",
            Self::Popular => "popular",
            Self::Recent => "recent",
            Self::Subscribers => "subscribers",
        }
    }

    fn browse_sort(self) -> &'static str {
        match self {
            Self::Relevance => "textsearch",
            Self::Trend => "trend",
            Self::Popular => "toprated",
            Self::Recent => "mostrecent",
            Self::Subscribers => "totaluniquesubscribers",
        }
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "search_hydration_tests.rs"]
mod hydration_tests;

#[cfg(test)]
#[path = "search_live_tests.rs"]
mod live_tests;
