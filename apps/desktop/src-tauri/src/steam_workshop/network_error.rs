use app_network::NetworkError;
use serde_json::json;

pub(super) fn request_stage(request: &reqwest::Request) -> &'static str {
    match request.url().path() {
        "/workshop/browse/" => "browse",
        "/sharedfiles/filedetails/" => "item_type",
        "/ISteamRemoteStorage/GetCollectionDetails/v1/" => "collection",
        _ => "details",
    }
}

pub(super) fn workshop_network_error(stage: &str, error: &NetworkError) -> String {
    let (origin, reason, status) = match error {
        NetworkError::Deadline { origin, .. } => (origin.as_str(), "timeout", None),
        NetworkError::Transport { origin, source } => (
            origin.as_str(),
            if source.is_timeout() {
                "timeout"
            } else if source.is_connect() {
                "connection"
            } else if source.is_body() || source.is_decode() {
                "response"
            } else {
                "request"
            },
            None,
        ),
        NetworkError::Status { origin, status }
        | NetworkError::RetryDeferred { origin, status, .. } => {
            (origin.as_str(), "http", Some(status.as_u16()))
        }
        NetworkError::BodyTooLarge { origin, .. } => (origin.as_str(), "response", None),
        NetworkError::Unrepeatable | NetworkError::InvalidSource(_) => {
            return format!("failed to read Steam Workshop response: {error}");
        }
    };
    json!({
        "code": "steam_workshop_network_failed",
        "message": format!("failed to read Steam Workshop response: {error}"),
        "stage": stage,
        "origin": origin,
        "reason": reason,
        "status": status,
        "retry_after_seconds": match error {
            NetworkError::RetryDeferred { retry_after: Some(delay), .. } => Some(delay.as_secs().saturating_add(u64::from(delay.subsec_nanos() > 0))),
            _ => None,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn workshop_failures_identify_the_actual_service_and_stage() {
        let client = reqwest::Client::new();
        for (url, stage) in [
            (
                "https://steamcommunity.com/workshop/browse/?searchtext=example",
                "browse",
            ),
            (
                "https://steamcommunity.com/sharedfiles/filedetails/?id=123456",
                "item_type",
            ),
            (
                "https://api.steamchina.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/",
                "details",
            ),
            (
                "https://api.steamchina.com/ISteamRemoteStorage/GetCollectionDetails/v1/",
                "collection",
            ),
        ] {
            let request = client.get(url).build().expect("request");
            assert_eq!(request_stage(&request), stage);
            let error = NetworkError::Deadline {
                attempts: 2,
                origin: request.url().origin().ascii_serialization(),
            };
            let payload: serde_json::Value =
                serde_json::from_str(&workshop_network_error(stage, &error)).expect("diagnostic");
            assert_eq!(payload["reason"], "timeout");
            assert_eq!(payload["stage"], stage);
            assert_eq!(
                payload["origin"],
                request.url().origin().ascii_serialization()
            );
            assert!(!payload.to_string().contains("searchtext"));
        }
    }

    #[test]
    fn throttling_retains_status_without_claiming_a_connection_outage() {
        let error = NetworkError::RetryDeferred {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            origin: "https://steamcommunity.com".to_string(),
            retry_after: Some(Duration::from_secs(120)),
        };
        let payload: serde_json::Value =
            serde_json::from_str(&workshop_network_error("browse", &error)).expect("diagnostic");
        assert_eq!(payload["reason"], "http");
        assert_eq!(payload["status"], 429);
        assert_eq!(payload["retry_after_seconds"], 120);
    }

    #[tokio::test]
    async fn refused_connection_is_classified_without_exposing_request_parameters() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture");
        let address = listener.local_addr().expect("address");
        drop(listener);
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(8))
            .build()
            .expect("client");
        let request = client
            .get(format!("http://{address}/?key=private-fixture-value"))
            .build()
            .expect("request");
        let origin = request.url().origin().ascii_serialization();
        // Classify one transport failure. A retry budget can correctly turn a
        // refused Windows connection into an overall deadline instead.
        let source = client
            .execute(request)
            .await
            .expect_err("refused connection");
        let error = NetworkError::Transport {
            origin,
            source: source.without_url(),
        };
        let diagnostic = workshop_network_error("browse", &error);
        let payload: serde_json::Value = serde_json::from_str(&diagnostic).expect("diagnostic");
        assert_eq!(payload["reason"], "connection");
        assert!(!diagnostic.contains("private-fixture-value"));
    }
}
