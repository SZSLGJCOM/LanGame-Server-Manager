//! Reviewed publisher article APIs with explicit source and module bindings.
//! Public visibility and identity are checked before extracting the HTML body.
use reqwest::Url;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::sources::{Source, public_url};
use crate::{KnowledgeError, Result};

const LOCALE: &str = "en-us";
const API_PATH: &str = "/api/v2/help_center/en-us/articles/";
const HTML_PATH: &str = "/hc/en-us/articles/";

struct PublisherArticlePolicy {
    module_id: &'static str,
    source_id: &'static str,
    api_host: &'static str,
    api_path: &'static str,
    api_suffix: &'static str,
    response_host: &'static str,
    citation_host: &'static str,
    article_ids: &'static [u64],
}

// Exact reviewed publishers and article IDs only. Minecraft's public website
// uses a same-origin proxy; its envelope retains the publisher's Zendesk host.
const PUBLISHERS: &[PublisherArticlePolicy] = &[
    PublisherArticlePolicy {
        module_id: "enshrouded",
        source_id: "keen-server-help",
        api_host: "enshrouded.zendesk.com",
        api_path: API_PATH,
        api_suffix: ".json",
        response_host: "enshrouded.zendesk.com",
        citation_host: "enshrouded.zendesk.com",
        article_ids: &[
            16051370691485,
            16055628734109,
            16055441447709,
            20453241249821,
            19191581489309,
            16056312924957,
            16454683440541,
        ],
    },
    PublisherArticlePolicy {
        module_id: "dontstarve",
        source_id: "klei-command-line",
        api_host: "support.klei.com",
        api_path: API_PATH,
        api_suffix: ".json",
        response_host: "support.klei.com",
        citation_host: "support.klei.com",
        article_ids: &[360029556192],
    },
    PublisherArticlePolicy {
        module_id: "sevendaystodie",
        source_id: "fun-pimps-server-migration",
        api_host: "7-days-to-die.zendesk.com",
        api_path: API_PATH,
        api_suffix: ".json",
        response_host: "7-days-to-die.zendesk.com",
        citation_host: "7-days-to-die.zendesk.com",
        article_ids: &[50318172509972],
    },
    PublisherArticlePolicy {
        module_id: "minecraft",
        source_id: "mojang-java-help",
        api_host: "help.minecraft.net",
        api_path: "/help_center/en-us/articles/",
        api_suffix: "",
        response_host: "minecrafthelp.zendesk.com",
        citation_host: "help.minecraft.net",
        article_ids: &[360058525452],
    },
];

fn publisher(source: &Source) -> Option<&'static PublisherArticlePolicy> {
    PUBLISHERS
        .iter()
        .find(|policy| policy.source_id == source.id)
}

/// Catalog loading supplies the module identity that an individual Source does
/// not carry. Reserved publisher source IDs cannot be rebound to another game.
pub(crate) fn validate_source_module(module_id: &str, source: &Source) -> Result<()> {
    if let Some(policy) = publisher(source)
        && (module_id != policy.module_id
            || source.kind != "official"
            || source.reference_only
            || source.discover_links
            || !source.allowed_prefixes.is_empty()
            || !source.sitemaps.is_empty()
            || source.seeds.iter().any(|seed| {
                Url::parse(seed).map_or(true, |url| article_id(policy, &url).is_none())
            }))
    {
        return Err(invalid(
            "Publisher article source changed its reviewed module or scope",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct PublicArticle {
    pub title: String,
    pub html: String,
    pub canonical_url: Url,
}

/// Only exact, reviewed seeds opt into this format. Prefix discovery cannot
/// turn an arbitrary Zendesk API endpoint into an authorized article source.
pub(crate) fn is_article_url(source: &Source, url: &Url) -> bool {
    !source.reference_only
        && source.kind == "official"
        && publisher(source).is_some_and(|policy| article_id(policy, url).is_some())
        && source.seeds.iter().any(|seed| seed == url.as_str())
}

/// Keep the request URL as the cache identity while presenting the publisher's
/// stable article route to readers. The article ID survives title/slug changes.
pub(crate) fn citation_url(source: &Source, url: &Url) -> Option<Url> {
    if !is_article_url(source, url) {
        return None;
    }
    let policy = publisher(source)?;
    Url::parse(&format!(
        "https://{}{HTML_PATH}{}",
        policy.citation_host,
        article_id(policy, url)?
    ))
    .ok()
}

pub(crate) fn extract_article(
    source: &Source,
    url: &Url,
    content_type: &str,
    bytes: &[u8],
    cancel: &std::sync::atomic::AtomicBool,
    deadline: std::time::Instant,
) -> Result<crate::extract::Extracted> {
    crate::check_cancel(cancel)?;
    let article = parse(source, url, content_type, bytes)?;
    let html = format!("<article>{}</article>", article.html);
    let mut body_source = source.clone();
    body_source.content_selector = Some("article".into());
    // The JSON envelope already selects one reviewed article; its body cannot
    // become a discovery directory, even if a catalog also has HTML seeds.
    body_source.discovery_selector = None;
    body_source.discover_links = false;
    let mut extracted = crate::extract::extract_with_context(
        &body_source,
        &article.canonical_url,
        "text/html; charset=utf-8",
        html.as_bytes(),
        cancel,
        deadline,
    )?;
    extracted.title = article.title;
    extracted.links.clear();
    Ok(extracted)
}

pub(crate) fn parse(
    source: &Source,
    url: &Url,
    content_type: &str,
    bytes: &[u8],
) -> Result<PublicArticle> {
    if !is_article_url(source, url) {
        return Err(invalid("Unreviewed Zendesk article API URL"));
    }
    let content_type = content_type.split(';').next().unwrap_or("").trim();
    if !content_type.eq_ignore_ascii_case("application/json") {
        return Err(invalid("Zendesk article response must be application/json"));
    }
    if bytes.len() > crate::extract::MAX_TEXT {
        return Err(invalid("Zendesk article response exceeds 2 MiB"));
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| invalid("Invalid Zendesk article response JSON"))?;
    let article = envelope.article;
    if article.draft || !public_segments(&article.user_segment_id, &article.user_segment_ids) {
        return Err(KnowledgeError::Policy(
            "Zendesk article is not explicitly published for anonymous readers".into(),
        ));
    }
    let policy = publisher(source).ok_or_else(|| invalid("Unreviewed publisher article source"))?;
    if Some(article.id) != article_id(policy, url) || article.locale != LOCALE {
        return Err(invalid("Zendesk article identity or locale changed"));
    }
    let mut canonical_url = public_url(&article.html_url)?;
    if canonical_url.host_str() != Some(policy.response_host)
        || canonical_url.query().is_some()
        || canonical_url.fragment().is_some()
        || !canonical_path(&canonical_url, article.id)
    {
        return Err(invalid("Zendesk article canonical URL changed identity"));
    }
    canonical_url
        .set_host(Some(policy.citation_host))
        .map_err(|_| invalid("Invalid publisher citation host"))?;
    let title = article.title.trim();
    if title.is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
        return Err(invalid("Invalid Zendesk article title"));
    }
    if article.body.trim().is_empty() || article.body.len() > crate::extract::MAX_TEXT {
        return Err(invalid("Missing or oversized Zendesk article body"));
    }
    Ok(PublicArticle {
        title: title.to_owned(),
        html: article.body,
        canonical_url,
    })
}

fn article_id(policy: &PublisherArticlePolicy, url: &Url) -> Option<u64> {
    if public_url(url.as_str()).is_err()
        || url.host_str() != Some(policy.api_host)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let id = url
        .path()
        .strip_prefix(policy.api_path)?
        .strip_suffix(policy.api_suffix)?;
    if id.starts_with('0') || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = id.parse().ok()?;
    policy.article_ids.contains(&id).then_some(id)
}

fn canonical_path(url: &Url, id: u64) -> bool {
    let Some(article) = url.path().strip_prefix(HTML_PATH) else {
        return false;
    };
    let (actual, slug) = article.split_once('-').unwrap_or((article, ""));
    actual == id.to_string()
        && (!article.contains('-') || !slug.is_empty())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(Deserialize)]
struct Envelope {
    article: Article,
}

#[derive(Deserialize)]
struct Article {
    id: u64,
    locale: String,
    draft: bool,
    title: String,
    body: String,
    html_url: String,
    #[serde(default)]
    user_segment_id: SegmentField,
    #[serde(default)]
    user_segment_ids: SegmentField,
}

/// Keep absent distinct from explicit public metadata. Derived Article decoding
/// also rejects duplicate fields instead of accepting a last-wins permission.
#[derive(Default)]
enum SegmentField {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> Deserialize<'de> for SegmentField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Value::deserialize(deserializer).map(Self::Present)
    }
}

fn public_segments(single: &SegmentField, multiple: &SegmentField) -> bool {
    let single_public = matches!(single, SegmentField::Present(Value::Null));
    let multiple_public =
        matches!(multiple, SegmentField::Present(Value::Array(ids)) if ids.is_empty());
    let single_safe = single_public || matches!(single, SegmentField::Missing);
    let multiple_safe = multiple_public
        || matches!(
            multiple,
            SegmentField::Missing | SegmentField::Present(Value::Null)
        );
    single_safe && multiple_safe && (single_public || multiple_public)
}

fn invalid(message: &str) -> KnowledgeError {
    KnowledgeError::Invalid(message.into())
}

#[cfg(test)]
#[path = "zendesk_input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "zendesk_tests.rs"]
mod persistence_tests;

#[cfg(test)]
#[path = "zendesk_publisher_tests.rs"]
mod publisher_tests;
