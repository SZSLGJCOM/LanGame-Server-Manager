use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::context::Endpoint;

#[cfg(test)]
#[path = "satisfactory_world_fixture.rs"]
pub(super) mod fixture;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(45);

fn request_budget(function: &str, remaining: Duration) -> Duration {
    // Native map travel and save serialization run on the game thread. A cold
    // CreateNewGame measured 5.8s before its empty 202 response, beyond a safe
    // margin for short reads; these operations use the bounded remaining budget.
    if matches!(function, "CreateNewGame" | "LoadGame" | "SaveGame") {
        remaining
    } else {
        remaining.min(REQUEST_TIMEOUT)
    }
}

pub(super) struct Api {
    pub(super) endpoint: Endpoint,
    client: reqwest::Client,
    started: Instant,
    #[cfg(test)]
    pub(super) fixture: Option<fixture::Fixture>,
    #[cfg(test)]
    pub(super) test_credentials:
        Option<std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>>,
}

pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) data: Value,
}

#[derive(Debug)]
pub(super) enum ApiError {
    Authentication,
    Rejected(String),
    Unavailable,
    OutcomeUnknown,
    InvalidResponse,
    TooLarge,
    Ownership(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Authentication => "Authorize Satisfactory server management before continuing.",
            Self::Rejected(code) => {
                return write!(
                    formatter,
                    "The Satisfactory server rejected the request ({code})."
                );
            }
            Self::Unavailable => {
                "The Satisfactory HTTPS API is unavailable or still loading. Refresh when the server is ready."
            }
            Self::OutcomeUnknown => {
                "The Satisfactory request outcome could not be confirmed. Refresh or reconnect before retrying; the request was not resent."
            }
            Self::InvalidResponse => "The Satisfactory HTTPS API returned an incomplete response.",
            Self::TooLarge => "The Satisfactory HTTPS API response exceeded its size limit.",
            Self::Ownership(message) => message,
        };
        formatter.write_str(message)
    }
}

impl Api {
    pub(super) fn new(endpoint: Endpoint) -> Result<Self, String> {
        // The target is always numeric loopback and is checked against the owned
        // process before each request. The native server uses a self-signed cert.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .danger_accept_invalid_certs(true)
            .connect_timeout(Duration::from_secs(2))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| "The Satisfactory HTTPS client could not be created.".to_string())?;
        Ok(Self {
            endpoint,
            client,
            started: Instant::now(),
            #[cfg(test)]
            fixture: None,
            #[cfg(test)]
            test_credentials: None,
        })
    }

    pub(super) async fn read_credential(&self, kind: &str) -> Result<Option<String>, String> {
        #[cfg(test)]
        if let Some(credentials) = &self.test_credentials {
            return Ok(credentials
                .lock()
                .map_err(|_| "Fixture credential store lock failed.".to_string())?
                .get(kind)
                .cloned());
        }
        super::credentials::read(&self.endpoint.credential_identity, kind).await
    }

    pub(super) async fn store_credential(&self, kind: &str, secret: String) -> Result<(), String> {
        #[cfg(test)]
        if let Some(credentials) = &self.test_credentials {
            credentials
                .lock()
                .map_err(|_| "Fixture credential store lock failed.".to_string())?
                .insert(kind.to_owned(), secret);
            return Ok(());
        }
        super::credentials::write(&self.endpoint.credential_identity, kind, secret).await
    }

    async fn check_listener(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture.check(false);
        }
        self.endpoint.check().await
    }

    async fn check_process(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            return fixture.check(true);
        }
        self.endpoint.check_process().await
    }

    pub(super) async fn call(
        &self,
        function: &'static str,
        data: Value,
        token: Option<&str>,
        mutation: bool,
    ) -> Result<Reply, ApiError> {
        if !matches!(
            function,
            "PasswordlessLogin"
                | "PasswordLogin"
                | "ClaimServer"
                | "VerifyAuthenticationToken"
                | "QueryServerState"
                | "GetServerOptions"
                | "GetAdvancedGameSettings"
                | "EnumerateSessions"
                | "ApplyAdvancedGameSettings"
                | "CreateNewGame"
                | "LoadGame"
                | "RenameServer"
                | "SetClientPassword"
                | "SetAutoLoadSessionName"
                | "SaveGame"
                | "RunCommand"
        ) || (function == "RunCommand" && data != json!({"Command": "server.GenerateAPIToken"}))
        {
            return Err(ApiError::InvalidResponse);
        }
        self.check_listener().await.map_err(ApiError::Ownership)?;
        let remaining = OPERATION_TIMEOUT
            .checked_sub(self.started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(ApiError::Unavailable)?;
        let timeout = request_budget(function, remaining);
        let mut request = self
            .client
            .post(format!("https://127.0.0.1:{}/api/v1", self.endpoint.port))
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&json!({"function": function, "data": data}))
            .timeout(timeout);
        if let Some(token) = token {
            let mut authorization =
                reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| ApiError::Authentication)?;
            authorization.set_sensitive(true);
            request = request.header(reqwest::header::AUTHORIZATION, authorization);
        }
        let request = request.build().map_err(|_| ApiError::InvalidResponse)?;
        #[cfg(test)]
        let result = if let Some(fixture) = &self.fixture {
            fixture.reply(function, &data)
        } else {
            execute(&self.client, request, mutation).await
        };
        #[cfg(not(test))]
        let result = execute(&self.client, request, mutation).await;
        // A confirmed map change deliberately suspends HTTPS availability.
        // Its process must remain owned, while its listening socket can close.
        let check = if matches!(function, "CreateNewGame" | "LoadGame")
            && result.as_ref().is_ok_and(|reply| reply.status == 202)
        {
            self.check_process().await
        } else {
            self.check_listener().await
        };
        if let Err(error) = check {
            return Err(if mutation {
                ApiError::OutcomeUnknown
            } else {
                ApiError::Ownership(error)
            });
        }
        result
    }

    pub(super) async fn server_name(&self) -> Result<String, String> {
        self.check_listener().await?;
        #[cfg(test)]
        if let Some(fixture) = &self.fixture {
            let result = fixture.name();
            self.check_listener().await?;
            return result;
        }
        let port = self.endpoint.port;
        let name = tokio::time::timeout(Duration::from_secs(2), async move {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0")
                .await
                .map_err(|_| "The local server-name query could not be opened.".to_string())?;
            socket
                .connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|_| "The local server-name query could not be connected.".to_string())?;
            let cookie = uuid::Uuid::new_v4();
            let mut request = vec![0xd5, 0xf6, 0, 1];
            request.extend_from_slice(&cookie.as_bytes()[..8]);
            request.push(1);
            socket
                .send(&request)
                .await
                .map_err(|_| "The local server-name query failed.".to_string())?;
            let mut buffer = [0u8; 8192];
            let size = socket
                .recv(&mut buffer)
                .await
                .map_err(|_| "The local server-name query failed.".to_string())?;
            parse_server_name(&buffer[..size], &cookie.as_bytes()[..8])
        })
        .await
        .map_err(|_| "The Satisfactory server-name query timed out.".to_string())??;
        self.check_listener().await?;
        Ok(name)
    }
}

async fn execute(
    client: &reqwest::Client,
    request: reqwest::Request,
    mutation: bool,
) -> Result<Reply, ApiError> {
    let uncertain = || {
        if mutation {
            ApiError::OutcomeUnknown
        } else {
            ApiError::Unavailable
        }
    };
    let mut response = client.execute(request).await.map_err(|_| uncertain())?;
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(if mutation {
            ApiError::OutcomeUnknown
        } else {
            ApiError::TooLarge
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| uncertain())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(if mutation {
                ApiError::OutcomeUnknown
            } else {
                ApiError::TooLarge
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    let reply = parse_reply(status, &bytes);
    if mutation
        && matches!(
            reply,
            Err(ApiError::InvalidResponse | ApiError::Unavailable | ApiError::TooLarge)
        )
    {
        return Err(ApiError::OutcomeUnknown);
    }
    reply
}

fn parse_reply(status: u16, bytes: &[u8]) -> Result<Reply, ApiError> {
    if matches!(status, 401 | 403) {
        return Err(ApiError::Authentication);
    }
    let value = if bytes.is_empty() && matches!(status, 202 | 204) {
        json!({})
    } else {
        serde_json::from_slice::<Value>(bytes).map_err(|_| ApiError::InvalidResponse)?
    };
    if !value.is_object() {
        return Err(ApiError::InvalidResponse);
    }
    if let Some(code) = value.get("errorCode").and_then(Value::as_str) {
        // Native messages/data may contain caller values, paths or credentials.
        // Only its bounded machine code is suitable for the UI error boundary.
        if code.len() > 64
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(ApiError::InvalidResponse);
        }
        if matches!(
            code,
            "invalid_token" | "wrong_password" | "not_authenticated" | "insufficient_privileges"
        ) {
            return Err(ApiError::Authentication);
        }
        let mut detail = code.to_owned();
        if code == "missing_params" {
            // Report only declared protocol field names, never a server message,
            // invalid field value, credential, or caller-provided unknown key.
            const FIELDS: &[&str] = &[
                "authenticationToken",
                "privilegeLevel",
                "minimumPrivilegeLevel",
                "command",
                "serverName",
                "adminPassword",
                "password",
                "newGameData",
                "saveName",
                "enableAdvancedGameSettings",
                "SessionName",
                "MapName",
                "StartingLocation",
                "bSkipOnboarding",
                "GameModeSettings",
                "AdvancedGameSettings",
                "CustomOptionsOnlyForModding",
                "AppliedAdvancedGameSettings",
            ];
            let fields = value
                .get("errorData")
                .and_then(|data| data.get("missingParameters"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|field| FIELDS.contains(field))
                .take(8)
                .collect::<Vec<_>>();
            if !fields.is_empty() {
                detail.push_str(&format!("; missing fields: {}", fields.join(", ")));
            }
        }
        return Err(ApiError::Rejected(detail));
    }
    if !matches!(status, 200 | 202 | 204) {
        return Err(ApiError::Unavailable);
    }
    let data = value.get("data").cloned().unwrap_or_else(|| json!({}));
    if !data.is_object() {
        return Err(ApiError::InvalidResponse);
    }
    Ok(Reply { status, data })
}

fn parse_server_name(bytes: &[u8], cookie: &[u8]) -> Result<String, String> {
    const ERROR: &str = "The Satisfactory server-name query response is incomplete.";
    if bytes.len() < 29
        || bytes[..4] != [0xd5, 0xf6, 1, 1]
        || bytes[4..12] != *cookie
        || bytes.last() != Some(&1)
    {
        return Err(ERROR.into());
    }
    let count = usize::from(bytes[25]);
    if count > 32 {
        return Err(ERROR.into());
    }
    let cursor = 26 + count * 3;
    let size = bytes
        .get(cursor..cursor + 2)
        .ok_or_else(|| ERROR.to_string())?;
    let size = usize::from(u16::from_le_bytes([size[0], size[1]]));
    if size > 4096 || cursor + 2 + size + 1 != bytes.len() {
        return Err(ERROR.into());
    }
    let name = std::str::from_utf8(&bytes[cursor + 2..cursor + 2 + size])
        .map_err(|_| ERROR.to_string())?;
    if name.chars().any(char::is_control) {
        return Err(ERROR.into());
    }
    Ok(name.into())
}

#[cfg(test)]
#[path = "satisfactory_world_api_tests.rs"]
mod tests;
