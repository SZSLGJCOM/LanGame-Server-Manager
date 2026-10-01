use std::net::SocketAddr;
use std::time::Duration;

pub(super) const MAX_HTTP_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HttpApiError {
    Authentication,
    Timeout,
    TooLarge,
    Transport,
    HttpStatus(u16),
}

/// Call a code-selected path on a numeric local instance endpoint. The caller
/// owns endpoint selection; proxy routing and redirects cannot move the request.
pub(super) async fn fetch_json(
    address: SocketAddr,
    path: &'static str,
    admin_password: Option<&str>,
    timeout: Duration,
) -> Result<Vec<u8>, HttpApiError> {
    request_json(
        address,
        path,
        admin_password,
        timeout,
        reqwest::Method::GET,
        None,
    )
    .await
}

pub(super) async fn request_json(
    address: SocketAddr,
    path: &'static str,
    admin_password: Option<&str>,
    timeout: Duration,
    method: reqwest::Method,
    body: Option<&serde_json::Value>,
) -> Result<Vec<u8>, HttpApiError> {
    let requires_ok = method == reqwest::Method::POST;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(timeout.min(Duration::from_secs(1)))
        .timeout(timeout)
        .build()
        .map_err(|_| HttpApiError::Transport)?;
    let mut request = client
        .request(method, format!("http://{address}{path}"))
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(body) = body {
        request = request.json(body);
    } else if requires_ok {
        // Palworld requires an explicit length even for its bodyless Save request.
        request = request.header(reqwest::header::CONTENT_LENGTH, "0");
    }
    if let Some(password) = admin_password {
        request = request.basic_auth("admin", Some(password));
    }
    let mut response = request.send().await.map_err(fetch_error)?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(HttpApiError::Authentication);
    }
    if !response.status().is_success()
        || (requires_ok && response.status() != reqwest::StatusCode::OK)
    {
        return Err(HttpApiError::HttpStatus(response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_HTTP_RESPONSE_BYTES as u64)
    {
        return Err(HttpApiError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(fetch_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_HTTP_RESPONSE_BYTES {
            return Err(HttpApiError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn fetch_error(error: reqwest::Error) -> HttpApiError {
    if error.is_timeout() {
        HttpApiError::Timeout
    } else {
        HttpApiError::Transport
    }
}
