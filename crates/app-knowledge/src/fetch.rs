use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::sources::{Source, public_url};
use crate::{ContentUse, KnowledgeError, Result, check_cancel};
use reqwest::{Client, StatusCode, Url};

pub(crate) const MAX_BODY: usize = 8 * 1024 * 1024;
const USER_AGENT: &str = "LanGameDocs/0.1";

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;

pub(crate) struct Fetcher {
    robots: HashMap<String, String>,
}

pub(crate) struct PageResponse {
    pub content_use: ContentUse,
    pub url: Url,
    pub status: StatusCode,
    pub body: Vec<u8>,
    pub content_type: String,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

impl Fetcher {
    pub fn new() -> Self {
        Self {
            robots: HashMap::new(),
        }
    }

    pub async fn page(
        &mut self,
        source: &Source,
        initial: &Url,
        etag: Option<&str>,
        modified: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<PageResponse> {
        let mut url = initial.clone();
        let (mut etag, mut modified) = (etag, modified);
        let mut content_use = ContentUse::Full;
        for _ in 0..6 {
            check_cancel(cancel)?;
            if !source.allows(&url) {
                return Err(KnowledgeError::Network(format!(
                    "URL is outside the reviewed source scope: {url}"
                )));
            }
            content_use = content_use.restrict(self.check_robots(&url, cancel).await?);
            let response = request(&url, etag, modified, cancel).await?;
            check_article_access(source, &url, response.status())?;
            content_use = content_use.restrict(response_content_use(&response)?);
            if response.status().is_redirection() && response.status() != StatusCode::NOT_MODIFIED {
                url = redirect(&url, &response)?;
                // Validators identify the requested resource, not another URL
                // reached through a redirect. Never forward them to that URL.
                etag = None;
                modified = None;
                continue;
            }
            let mut page = read_response(url, response, MAX_BODY, true, cancel).await?;
            page.content_use = content_use.restrict(page.content_use);
            return Ok(page);
        }
        Err(KnowledgeError::Network(
            "Documentation redirect limit exceeded".into(),
        ))
    }

    async fn check_robots(&mut self, url: &Url, cancel: &AtomicBool) -> Result<ContentUse> {
        let origin = url.origin().ascii_serialization();
        if !self.robots.contains_key(&origin) {
            let mut robots_url = url
                .join("/robots.txt")
                .map_err(|e| KnowledgeError::Network(e.to_string()))?;
            let mut resolved = None;
            for _ in 0..4 {
                let response = request(&robots_url, None, None, cancel).await?;
                if response.status().is_redirection() {
                    let next = redirect(&robots_url, &response)?;
                    if next.origin() != url.origin() {
                        return Err(KnowledgeError::Network(
                            "robots.txt redirected outside its origin".into(),
                        ));
                    }
                    robots_url = next;
                    continue;
                }
                if robots_unavailable(response.status()) {
                    resolved = Some(String::new());
                } else {
                    let page =
                        read_response(robots_url.clone(), response, 512 * 1024, false, cancel)
                            .await?;
                    resolved = Some(String::from_utf8_lossy(&page.body).into_owned());
                }
                break;
            }
            self.robots.insert(
                origin.clone(),
                resolved.ok_or_else(|| {
                    KnowledgeError::Network("robots.txt redirect limit exceeded".into())
                })?,
            );
        }
        let policy = self.robots.get(&origin).expect("robots policy inserted");
        let mut path = url.path().to_string();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        robots_policy(policy, &path)
            .map_err(|error| KnowledgeError::Policy(format!("{error}; source: {url}")))
    }
}

pub(crate) fn check_article_access(source: &Source, url: &Url, status: StatusCode) -> Result<()> {
    // Unlike a transient fetch failure, denied/deleted anonymous API articles
    // must hide retained evidence until public access is established again.
    if crate::zendesk::is_article_url(source, url)
        && matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::NOT_FOUND
                | StatusCode::GONE
                | StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
        )
    {
        return Err(KnowledgeError::Policy(format!(
            "Publisher article is no longer publicly accessible (HTTP {}): {}",
            status.as_u16(),
            source.citation_url(url.as_str())
        )));
    }
    Ok(())
}

fn robots_unavailable(status: StatusCode) -> bool {
    // RFC 9309 §2.3.1.3: a missing/unavailable robots resource (4xx) does not
    // prohibit public documents. In particular, object CDNs often return 403
    // for a nonexistent robots key. Still stop for rate limits/legal refusal;
    // document requests retain their own strict status and policy checks.
    status.is_client_error()
        && status != StatusCode::TOO_MANY_REQUESTS
        && status != StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS
}

fn redirect(url: &Url, response: &reqwest::Response) -> Result<Url> {
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| KnowledgeError::Network("Redirect has no valid Location".into()))?;
    let mut next = url
        .join(location)
        .map_err(|e| KnowledgeError::Network(e.to_string()))?;
    next.set_fragment(None);
    public_url(next.as_str())
}

async fn request(
    url: &Url,
    etag: Option<&str>,
    modified: Option<&str>,
    cancel: &AtomicBool,
) -> Result<reqwest::Response> {
    public_url(url.as_str())?;
    let proxy_matcher = hyper_util::client::proxy::matcher::Matcher::from_system();
    let destination: http::Uri = url
        .as_str()
        .parse()
        .map_err(|_| KnowledgeError::Network("Invalid documentation request URI".into()))?;
    if let Some(proxy) = proxy_matcher.intercept(&destination) {
        // Explicit operator-configured proxies own destination DNS resolution.
        // Do not resolve a second time on the host: TUN clients may intentionally
        // return reserved fake IPs. The proxy receives only reviewed HTTPS hosts;
        // the same source scope and redirect checks still apply on every hop.
        let mut configured = reqwest::Proxy::https(proxy.uri().to_string()).map_err(network)?;
        if let Some(auth) = proxy.basic_auth() {
            configured = configured.custom_http_auth(auth.clone());
        }
        let client = client_builder()
            .proxy(configured)
            .build()
            .map_err(network)?;
        return send(client, url, etag, modified, cancel).await;
    }
    let host = url
        .host_str()
        .ok_or_else(|| KnowledgeError::Network("URL has no host".into()))?;
    let addresses: Vec<SocketAddr> = tokio::select! {
        _ = cancelled(cancel) => return Err(KnowledgeError::Cancelled),
        result = tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host((host, 443))) => {
            result.map_err(|_| KnowledgeError::Network("DNS lookup timed out".into()))?.map_err(|e| KnowledgeError::Network(e.to_string()))?.collect()
        }
    };
    if addresses.is_empty() || addresses.iter().any(|a| !public_address(a.ip())) {
        return Err(KnowledgeError::Network(
            "Documentation DNS resolved to a non-public address".into(),
        ));
    }
    // Pin the checked addresses: a second resolver lookup must not reopen a DNS
    // rebinding window. Ambient proxies cannot redirect this public-only client.
    let client = client_builder()
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(network)?;
    send(client, url, etag, modified, cancel).await
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .no_proxy()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(40))
}

async fn send(
    client: Client,
    url: &Url,
    etag: Option<&str>,
    modified: Option<&str>,
    cancel: &AtomicBool,
) -> Result<reqwest::Response> {
    let mut request = client.get(url.clone());
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    if let Some(modified) = modified {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, modified);
    }
    tokio::select! { _ = cancelled(cancel) => Err(KnowledgeError::Cancelled), response = request.send() => response.map_err(network) }
}

async fn read_response(
    url: Url,
    mut response: reqwest::Response,
    limit: usize,
    indexing: bool,
    cancel: &AtomicBool,
) -> Result<PageResponse> {
    let status = response.status();
    if !status.is_success() && status != StatusCode::NOT_MODIFIED {
        return Err(KnowledgeError::Network(format!(
            "HTTP {} from {url}",
            status.as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(KnowledgeError::Network(format!(
            "Documentation response exceeds {limit} bytes: {url}"
        )));
    }
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = header(reqwest::header::CONTENT_TYPE).unwrap_or_default();
    let etag = header(reqwest::header::ETAG);
    let modified = header(reqwest::header::LAST_MODIFIED);
    let content_use = if indexing {
        response_content_use(&response)?
    } else {
        ContentUse::Full
    };
    // robots.txt is read as access policy, never indexed. Its own noindex header
    // does not prohibit indexing a different resource on the same origin.
    if indexing
        && response
            .headers()
            .get_all("x-robots-tag")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|v| {
                v.split([',', ' ']).any(|part| {
                    matches!(
                        part.trim().to_ascii_lowercase().as_str(),
                        "noindex" | "noarchive" | "none"
                    )
                })
            })
    {
        return Err(KnowledgeError::Policy(format!(
            "Publisher X-Robots-Tag disallows indexing: {url}"
        )));
    }
    let mut body = Vec::new();
    loop {
        let chunk = tokio::select! { _ = cancelled(cancel) => return Err(KnowledgeError::Cancelled), result = response.chunk() => result.map_err(network)? };
        let Some(chunk) = chunk else {
            break;
        };
        if body.len() + chunk.len() > limit {
            return Err(KnowledgeError::Network(format!(
                "Documentation body exceeds {limit} bytes: {url}"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(PageResponse {
        content_use,
        url,
        status,
        body,
        content_type,
        etag,
        modified,
    })
}

fn response_content_use(response: &reqwest::Response) -> Result<ContentUse> {
    response
        .headers()
        .get_all("content-signal")
        .iter()
        .try_fold(ContentUse::Full, |policy, value| {
            policy.apply(value.to_str().map_err(|_| {
                KnowledgeError::Policy("Invalid publisher Content-Signal header".into())
            })?)
        })
}

fn network(error: reqwest::Error) -> KnowledgeError {
    KnowledgeError::Network(error.without_url().to_string())
}

pub(crate) async fn cancelled(cancel: &AtomicBool) {
    while check_cancel(cancel).is_ok() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(crate) fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_multicast()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0 && c == 0)
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 192 && b == 88 && c == 99)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
        }
    }
}

// RFC 9309 group selection, longest rule and Allow on a tie. We additionally
// honor explicit publisher restrictions on using page contents as AI input.
#[cfg(test)]
pub(crate) fn robots_allowed(text: &str, path: &str) -> bool {
    robots_policy(text, path).is_ok()
}

fn robots_policy(text: &str, path: &str) -> Result<ContentUse> {
    type RobotsRule = (bool, String);
    type RobotsGroup = (Vec<String>, Vec<RobotsRule>, Vec<String>);
    let mut groups: Vec<RobotsGroup> = Vec::new();
    let mut agents = Vec::new();
    let mut rules = Vec::new();
    let mut signals = Vec::new();
    let mut global_signals = Vec::new();
    let mut seen_rule = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "user-agent" => {
                if seen_rule {
                    groups.push((
                        std::mem::take(&mut agents),
                        std::mem::take(&mut rules),
                        std::mem::take(&mut signals),
                    ));
                    seen_rule = false;
                }
                agents.push(value.to_ascii_lowercase());
            }
            "allow" | "disallow" if !agents.is_empty() => {
                seen_rule = true;
                if !value.is_empty() {
                    rules.push((key.trim().eq_ignore_ascii_case("allow"), value.to_string()));
                }
            }
            "content-signal" => {
                if agents.is_empty() {
                    global_signals.push(value.to_owned());
                } else {
                    signals.push(value.to_owned());
                    seen_rule = true;
                }
            }
            _ => {}
        }
    }
    groups.push((agents, rules, signals));
    let specificity = |agents: &[String]| {
        agents
            .iter()
            .filter_map(|agent| {
                if agent == "*" {
                    Some(0)
                } else if USER_AGENT.to_ascii_lowercase().contains(agent.as_str()) {
                    Some(agent.len())
                } else {
                    None
                }
            })
            .max()
    };
    let best_group = groups
        .iter()
        .filter_map(|(agents, _, _)| specificity(agents))
        .max();
    let mut best: Option<(usize, bool)> = None;
    let mut content_use = ContentUse::Full;
    for signal in global_signals {
        content_use = content_use.apply(&signal)?;
    }
    for (agents, rules, signals) in &groups {
        if specificity(agents) != best_group || best_group.is_none() {
            continue;
        }
        for signal in signals {
            content_use = content_use.apply(signal)?;
        }
        for (allow, pattern) in rules {
            if rule_matches(pattern, path) {
                let normalized = robots_octets(pattern, true);
                let candidate = (
                    normalized
                        .iter()
                        .filter(|c| **c != b'*' && **c != b'$')
                        .count(),
                    *allow,
                );
                if best.is_none_or(|old| candidate > old) {
                    best = Some(candidate);
                }
            }
        }
    }
    if best.is_none_or(|(_, allow)| allow) {
        Ok(content_use)
    } else {
        Err(KnowledgeError::Policy(
            "Publisher robots.txt disallows this URL".into(),
        ))
    }
}

fn rule_matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.ends_with('$');
    let pattern = robots_octets(pattern.strip_suffix('$').unwrap_or(pattern), true);
    let path = robots_octets(path, false);
    let (mut pattern_index, mut path_index) = (0, 0);
    let mut wildcard = None;
    while path_index < path.len() {
        if pattern_index == pattern.len() && !anchored {
            return true;
        }
        if pattern.get(pattern_index) == Some(&b'*') {
            wildcard = Some((pattern_index, path_index));
            pattern_index += 1;
        } else if pattern.get(pattern_index) == path.get(path_index) {
            pattern_index += 1;
            path_index += 1;
        } else if let Some((star, consumed)) = wildcard {
            let next = consumed + 1;
            wildcard = Some((star, next));
            pattern_index = star + 1;
            path_index = next;
        } else {
            return false;
        }
    }
    while pattern.get(pattern_index) == Some(&b'*') {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

// RFC 9309: compare canonical URI octets, decode escaped unreserved ASCII,
// retain escaped separators, and distinguish wildcard syntax from literal '*'.
fn robots_octets(value: &str, pattern: bool) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%'
            && index + 2 < bytes.len()
            && let (Some(a), Some(b)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            let decoded = a * 16 + b;
            if decoded.is_ascii_alphanumeric() || b"-._~".contains(&decoded) {
                result.push(decoded);
            } else {
                push_percent(&mut result, decoded);
            }
            index += 3;
            continue;
        }
        if !byte.is_ascii() || (!pattern && matches!(byte, b'*' | b'$')) {
            push_percent(&mut result, byte);
        } else {
            result.push(byte);
        }
        index += 1;
    }
    result
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn push_percent(bytes: &mut Vec<u8>, value: u8) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    bytes.extend([b'%', HEX[(value >> 4) as usize], HEX[(value & 15) as usize]]);
}
