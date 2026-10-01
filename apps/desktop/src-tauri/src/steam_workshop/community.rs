use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use app_network::{NetworkError, PublicBytes, SourcePreference};
use reqwest::{Client, Method, Request};
use tokio::time::Instant;

#[path = "community_cooldown.rs"]
mod cooldown;
static COMMUNITY_GATE: cooldown::CommunityGate = cooldown::CommunityGate::new();

const COMMUNITY_HOST: &str = "steamcommunity.com";
const COMMUNITY_CDN: &str = "steamcommunity-a.akamaihd.net";
const CONNECTION_FAILURE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
enum Connection {
    Origin,
    Cdn,
}

impl Connection {
    fn index(self) -> usize {
        match self {
            Self::Origin => 0,
            Self::Cdn => 1,
        }
    }
}

struct Route {
    request: Request,
    connection: Option<Connection>,
}

#[derive(Default)]
struct ConnectionHealth {
    // Fixed identities bound memory and keep CDN + Host routing separate from
    // the ordinary equivalent-URL policy. No caller-selected URL becomes a key.
    failed_at: Mutex<[Option<Instant>; 2]>,
}

impl ConnectionHealth {
    fn order(&self, routes: &mut [Route], now: Instant) {
        let Ok(failures) = self.failed_at.lock() else {
            return;
        };
        // Stable sorting keeps this request's locale order when both connections
        // have the same health, including after the cooldown expires.
        routes.sort_by_key(|route| {
            route
                .connection
                .and_then(|connection| failures[connection.index()])
                .is_some_and(|failed_at| {
                    now.saturating_duration_since(failed_at) < CONNECTION_FAILURE_TTL
                })
        });
    }

    fn record_failure(&self, connection: Option<Connection>, now: Instant) {
        if let Some(connection) = connection
            && let Ok(mut failures) = self.failed_at.lock()
        {
            failures[connection.index()] = Some(now);
        }
    }
}

static CONNECTION_HEALTH: ConnectionHealth = ConnectionHealth {
    failed_at: Mutex::new([None, None]),
};

/// Only anonymous Workshop HTML reads may use the CDN's Community virtual host.
/// Account pages, credentials, and arbitrary caller-selected destinations never do.
pub(super) fn supports(request: &Request) -> bool {
    let url = request.url();
    if request.method() != Method::GET
        || request.body().is_some()
        || url.scheme() != "https"
        || url.host_str() != Some(COMMUNITY_HOST)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !request
            .headers()
            .keys()
            .all(|name| matches!(name.as_str(), "accept" | "accept-language" | "user-agent"))
    {
        return false;
    }
    let (required, allowed): (&str, &[&str]) = match url.path() {
        "/workshop/browse/" => (
            "appid",
            &[
                "appid",
                "section",
                "browsesort",
                "actualsort",
                "p",
                "numperpage",
                "l",
                "days",
                "searchtext",
            ],
        ),
        "/sharedfiles/filedetails/" => ("id", &["id", "l"]),
        _ => return false,
    };
    let mut keys = HashSet::new();
    let mut valid_id = false;
    for (key, value) in url.query_pairs() {
        if !allowed.contains(&key.as_ref()) || !keys.insert(key.clone().into_owned()) {
            return false;
        }
        if key == required {
            valid_id = value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok_and(|id| id > 0);
        }
    }
    valid_id
}

pub(super) async fn read(
    client: &Client,
    request: Request,
    budget: Duration,
    max_bytes: usize,
    preference: SourcePreference,
) -> Result<PublicBytes, NetworkError> {
    let is_community = supports(&request);
    let routes = routes(request, preference)?;
    if is_community {
        return COMMUNITY_GATE
            .read(budget, |remaining| {
                read_routes(
                    client,
                    routes,
                    remaining,
                    max_bytes,
                    preference,
                    &CONNECTION_HEALTH,
                )
            })
            .await;
    }
    read_routes(
        client,
        routes,
        budget,
        max_bytes,
        preference,
        &CONNECTION_HEALTH,
    )
    .await
}

fn routes(request: Request, preference: SourcePreference) -> Result<Vec<Route>, NetworkError> {
    if !supports(&request) {
        return Ok(vec![Route {
            request,
            connection: None,
        }]);
    }
    let mut cdn = request.try_clone().ok_or(NetworkError::Unrepeatable)?;
    cdn.url_mut()
        .set_host(Some(COMMUNITY_CDN))
        .map_err(|_| NetworkError::InvalidSource(COMMUNITY_CDN.to_owned()))?;
    // TLS authenticates Steam's CDN hostname normally. Host selects Community's
    // public virtual host at that CDN; no local CA, fixed IP or TLS bypass is used.
    cdn.headers_mut().insert(
        reqwest::header::HOST,
        reqwest::header::HeaderValue::from_static(COMMUNITY_HOST),
    );
    let origin = Route {
        request,
        connection: Some(Connection::Origin),
    };
    let cdn = Route {
        request: cdn,
        connection: Some(Connection::Cdn),
    };
    Ok(match preference {
        SourcePreference::ChinaFirst => vec![cdn, origin],
        SourcePreference::InternationalFirst => vec![origin, cdn],
    })
}

async fn read_routes(
    client: &Client,
    mut routes: Vec<Route>,
    budget: Duration,
    max_bytes: usize,
    preference: SourcePreference,
    health: &ConnectionHealth,
) -> Result<PublicBytes, NetworkError> {
    let deadline = Instant::now() + budget;
    health.order(&mut routes, Instant::now());
    let count = routes.len();
    let mut last_error = NetworkError::Deadline {
        attempts: 0,
        origin: format!("https://{COMMUNITY_HOST}"),
    };
    for (index, route) in routes.into_iter().enumerate() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let route_budget = remaining / u32::try_from(count - index).unwrap_or(u32::MAX);
        match app_network::read_public_bytes(
            client,
            route.request,
            route_budget,
            max_bytes,
            preference,
        )
        .await
        {
            // HTTP 200 is not business validation. Do not clear an existing
            // cooldown for a challenge page or malformed Workshop response.
            Ok(response) => return Ok(response),
            Err(error) if error.permits_source_fallback() => {
                health.record_failure(route.connection, Instant::now());
                last_error = error;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error)
}

#[cfg(test)]
#[path = "community_tests.rs"]
mod tests;
