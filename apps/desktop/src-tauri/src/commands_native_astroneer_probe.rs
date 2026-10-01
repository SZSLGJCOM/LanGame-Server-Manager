use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use app_core::InstanceDetails;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const COMMANDS: [(&str, [&str; 5]); 3] = [
    (
        "DSListPlayers",
        [
            "DSListPlayers: timeout",
            "DSListPlayers: transport failure",
            "DSListPlayers: closed before response",
            "DSListPlayers: response limit exceeded",
            "DSListPlayers: incomplete or invalid JSON",
        ],
    ),
    (
        "DSListGames",
        [
            "DSListGames: timeout",
            "DSListGames: transport failure",
            "DSListGames: closed before response",
            "DSListGames: response limit exceeded",
            "DSListGames: incomplete or invalid JSON",
        ],
    ),
    (
        "DSServerStatistics",
        [
            "DSServerStatistics: timeout",
            "DSServerStatistics: transport failure",
            "DSServerStatistics: closed before response",
            "DSServerStatistics: response limit exceeded",
            "DSServerStatistics: incomplete or invalid JSON",
        ],
    ),
];

enum FetchError {
    Timeout,
    Transport,
    Closed,
    TooLarge,
    Incomplete,
}

// The caller restricts this diagnostic to its disposable native fixture. These
// observations do not establish world readiness or successful save generation.
pub(super) async fn inspect(instance: &InstanceDetails) -> Result<Vec<String>, &'static str> {
    let (port, password, settings) = console_context(instance)?;
    let sensitive_values = settings
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter(|(key, _)| sensitive_key(key))
        .filter_map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let mut observations = Vec::with_capacity(COMMANDS.len());
    for (command, errors) in COMMANDS {
        let mut response = fetch(port, &password, command)
            .await
            .map_err(|error| errors[error as usize])?;
        sanitize(&mut response, &sensitive_values);
        observations.push(format!("{command} {response}"));
    }
    Ok(observations)
}

pub(super) async fn world_state(
    instance: &InstanceDetails,
) -> Result<(Value, Value), &'static str> {
    let (port, password, _) = console_context(instance)?;
    let mut responses = Vec::with_capacity(2);
    for (command, errors) in &COMMANDS[1..] {
        responses.push(
            fetch(port, &password, command)
                .await
                .map_err(|error| errors[error as usize])?,
        );
    }
    let statistics = responses.pop().ok_or("missing ASTRONEER statistics")?;
    let games = responses.pop().ok_or("missing ASTRONEER game list")?;
    Ok((games, statistics))
}

fn console_context(instance: &InstanceDetails) -> Result<(u16, String, Value), &'static str> {
    if instance.summary.module_id != "astroneer" {
        return Err("ASTRONEER diagnostic: incorrect module");
    }
    let settings: Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| "ASTRONEER diagnostic: invalid settings")?;
    let password = settings
        .get("console_password")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.trim().is_empty()
                && value.chars().count() <= 128
                && !value.chars().any(char::is_control)
        })
        .ok_or("ASTRONEER diagnostic: invalid console password configuration")?;
    let mut ports = instance.ports.iter().filter(|port| port.name == "console");
    let port = ports
        .next()
        .ok_or("ASTRONEER diagnostic: missing console port")?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
        return Err("ASTRONEER diagnostic: invalid console port");
    }
    Ok((port.port, password.to_owned(), settings))
}

async fn fetch(port: u16, password: &str, command: &str) -> Result<Value, FetchError> {
    // Only fixed commands from COMMANDS reach this private helper. One deadline
    // covers connect, authentication, submission and every response fragment.
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await
            .map_err(|_| FetchError::Transport)?;
        stream
            .write_all(format!("{password}\n{command}\n").as_bytes())
            .await
            .map_err(|_| FetchError::Transport)?;
        let mut body = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream
                .read(&mut buffer)
                .await
                .map_err(|_| FetchError::Transport)?;
            if count == 0 {
                return Err(if body.is_empty() {
                    FetchError::Closed
                } else {
                    FetchError::Incomplete
                });
            }
            if body.len().saturating_add(count) > MAX_RESPONSE_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&buffer[..count]);
            if body.ends_with(b"\r\n") {
                match serde_json::from_slice::<Value>(&body) {
                    Ok(value) if value.is_object() => return Ok(value),
                    Err(error) if error.is_eof() => {}
                    _ => return Err(FetchError::Incomplete),
                }
            }
        }
    })
    .await
    .map_err(|_| FetchError::Timeout)?
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    [
        "password",
        "secret",
        "token",
        "gameid",
        "guid",
        "ip",
        "address",
        "url",
        "owner",
        "servername",
        "playername",
        "account",
        "steamid",
        "epicid",
        "name",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[test]
fn astroneer_diagnostics_preserve_world_state_without_runtime_identity() {
    let mut value = serde_json::json!({
        "activeSaveName": "",
        "gameList": [],
        "serverName": "temporary-server-identity",
        "nested": { "playerGuid": "player-identity", "publicIp": "192.0.2.1" },
        "status": "contains configured-credential",
        "secondsInGame": 75
    });
    sanitize(&mut value, &["configured-credential"]);
    assert_eq!(value["activeWorldSelected"], false);
    assert!(value.get("activeSaveName").is_none());
    assert_eq!(value["gameList"], serde_json::json!([]));
    assert_eq!(value["secondsInGame"], 75);
    let output = value.to_string();
    for sensitive in [
        "temporary-server-identity",
        "player-identity",
        "192.0.2.1",
        "configured-credential",
    ] {
        assert!(!output.contains(sensitive));
    }
}

fn sanitize(value: &mut Value, sensitive_values: &[&str]) {
    match value {
        Value::Object(object) => {
            for (name, presence) in [
                ("activeSaveName", "activeWorldSelected"),
                ("saveGameName", "worldSelected"),
            ] {
                if let Some(value) = object.remove(name) {
                    object.insert(
                        presence.into(),
                        Value::Bool(value.as_str().is_some_and(|name| !name.trim().is_empty())),
                    );
                }
            }
            object.retain(|key, _| !sensitive_key(key));
            for child in object.values_mut() {
                sanitize(child, sensitive_values);
            }
        }
        Value::Array(array) => {
            for child in array {
                sanitize(child, sensitive_values);
            }
        }
        Value::String(text) => {
            for sensitive in sensitive_values {
                *text = text.replace(sensitive, "[redacted]");
            }
            if text.contains("://") || text.parse::<std::net::IpAddr>().is_ok() {
                *text = String::from("[redacted]");
            }
        }
        _ => {}
    }
}
