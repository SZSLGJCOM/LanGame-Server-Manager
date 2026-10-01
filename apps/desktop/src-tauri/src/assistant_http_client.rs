use super::{ASSISTANT_CONNECT_TIMEOUT, USER_AGENT, normalize_service_url};
use reqwest::{Client, ClientBuilder, Url};
use std::net::IpAddr;
use std::time::Duration;

pub(super) fn is_loopback_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

pub(super) fn build_assistant_http_client(
    endpoint: &str,
    timeout: Duration,
) -> Result<Client, String> {
    let endpoint = Url::parse(&normalize_service_url(endpoint)?)
        .map_err(|error| format!("failed to parse assistant endpoint: {error}"))?;
    let builder = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(ASSISTANT_CONNECT_TIMEOUT)
        .timeout(timeout);
    apply_endpoint_policy(builder, &endpoint)
        .build()
        .map_err(|error| format!("failed to prepare assistant client: {error}"))
}

fn apply_endpoint_policy(builder: ClientBuilder, endpoint: &Url) -> ClientBuilder {
    let origin = endpoint.origin();
    // A 307/308 redirect replays diagnostic request bodies and custom API-key
    // headers. Authorization to one provider does not cover another origin,
    // including a different local port or an HTTPS-to-HTTP downgrade.
    let builder = builder.redirect(reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.url().origin() != origin
            || !attempt.url().username().is_empty()
            || attempt.url().password().is_some()
        {
            attempt.stop()
        } else if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else {
            attempt.follow()
        }
    }));
    // Windows proxy bypass entries such as `127.*` are not IP ranges in
    // reqwest's system matcher. Local AI must reach the listening local process.
    if is_loopback_url(endpoint) {
        builder.no_proxy()
    } else {
        builder
    }
}

#[cfg(test)]
#[path = "assistant_http_client_tests.rs"]
mod tests;
