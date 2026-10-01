#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::HashSet;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::time::Duration;

use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
#[path = "assistant_mock.rs"]
mod mock;
#[cfg(test)]
use mock::*;

#[cfg(test)]
#[path = "assistant_models_tests.rs"]
mod models_tests;

#[path = "assistant_http_client.rs"]
mod http_client;
use http_client::{build_assistant_http_client, is_loopback_url};

#[path = "assistant_provider.rs"]
mod provider;
use provider::{ProviderProtocol, normalize_service_url};

#[path = "assistant_persona.rs"]
mod persona;

#[path = "assistant_tool_conversation.rs"]
mod tool_conversation;
#[cfg(test)]
pub(crate) use tool_conversation::run_assistant_tool_turn;
pub(crate) use tool_conversation::{
    AssistantProgressSnapshot, AssistantSessionProgress, AssistantToolCall,
    AssistantToolDefinition, AssistantToolMessage, AssistantToolReply,
    run_assistant_tool_turn_streaming,
};

#[path = "assistant_connection.rs"]
mod connection;
pub use connection::{
    AssistantConnectionCheckInput, AssistantConnectionCheckOutput,
    cancel_assistant_connection_check, check_assistant_connection,
};

const USER_AGENT: &str = concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION"));
const ASSISTANT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const ASSISTANT_REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const ASSISTANT_LOCAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const ASSISTANT_MAX_RESPONSE_BYTES: usize = 256 * 1024;
const SYSTEM_PROMPT: &str = "You are LAN, the AI operations assistant for LanGame Server Manager. Be friendly, concise, and operator-focused. Use only the supplied context and do not invent missing facts. Respond in the same language as the user's prompt when it is clear. Use natural sentences and precise operational terms. Lead with the conclusion, then include evidence and next steps when they are relevant.";
const SECRET_SERVICE_NAME: &str = "LanGame Server Manager AI";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantProviderSettings {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantRunInput {
    pub settings: AssistantProviderSettings,
    pub prompt_label: String,
    pub prompt: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantRunOutput {
    pub provider: String,
    pub model: String,
    pub endpoint_url: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSecretDescriptor {
    pub provider: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantSecretStatus {
    pub stored: bool,
}

#[derive(Debug, Clone)]
enum SecretStoreBackend {
    SystemKeyring,
    #[cfg(test)]
    File(PathBuf),
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
}

pub async fn list_ollama_models(base_url: Option<&str>) -> Result<Vec<String>, String> {
    let endpoint_url = build_ollama_tags_endpoint(base_url)?;
    let client = build_assistant_http_client(&endpoint_url, Duration::from_secs(10))?;

    let response = client
        .get(&endpoint_url)
        .send()
        .await
        .map_err(|error| format!("failed to query Ollama models: {error}"))?;
    let status = response.status();
    let body = read_assistant_response_body(response).await?;

    if !status.is_success() {
        return Err(format!(
            "Ollama returned {} while listing models: {}",
            status,
            summarize_text(
                &redact_assistant_provider_text(&String::from_utf8_lossy(&body)),
                240
            )
        ));
    }

    let payload = serde_json::from_slice::<OllamaTagsResponse>(&body).map_err(|error| {
        format!(
            "failed to decode Ollama model list: {:?} at line {}, column {}",
            error.classify(),
            error.line(),
            error.column()
        )
    })?;
    let mut seen = HashSet::new();
    let models = payload
        .models
        .into_iter()
        .map(|model| model.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .filter(|name| seen.insert(name.clone()))
        .collect::<Vec<_>>();

    Ok(models)
}

pub async fn run_assistant(input: &AssistantRunInput) -> Result<AssistantRunOutput, String> {
    run_assistant_with_system_prompt(input, SYSTEM_PROMPT).await
}

pub async fn run_assistant_with_system_prompt(
    input: &AssistantRunInput,
    system_prompt: &str,
) -> Result<AssistantRunOutput, String> {
    let user_message = build_user_message(input);
    let protocol = ProviderProtocol::parse(&input.settings.provider)?;
    let provider = protocol.as_str().to_string();
    let model = normalize_required(&input.settings.model, "model")?;
    let system_prompt = persona::system_prompt(&normalize_required(system_prompt, "systemPrompt")?);
    #[cfg(test)]
    if input
        .settings
        .base_url
        .trim()
        .to_ascii_lowercase()
        .starts_with("mock://")
    {
        let endpoint_url = input.settings.base_url.trim().to_string();
        let normalized_prompt = mock_assistant_normalized_prompt(&input.prompt);
        let content = if system_prompt
            .to_ascii_lowercase()
            .contains("generate exactly one in-game server broadcast line")
        {
            mock_assistant_broadcast_content(&normalized_prompt)
        } else {
            mock_assistant_operation_plan_content(&input.prompt)
        };

        return Ok(AssistantRunOutput {
            provider,
            model,
            endpoint_url,
            content,
        });
    }
    let endpoint_url = protocol.endpoint(&input.settings.base_url)?;
    let api_key =
        resolve_api_key_with_backend(&input.settings, &SecretStoreBackend::SystemKeyring)?;
    let request_body = protocol.request_body(&model, &system_prompt, &user_message);

    let client =
        build_assistant_http_client(&endpoint_url, assistant_request_timeout(&input.settings))?;

    let response = protocol
        .request(&client, &endpoint_url, &request_body, &api_key)
        .send()
        .await
        .map_err(|error| format!("assistant request failed: {error}"))?;
    let status = response.status();
    let body = read_assistant_response_body(response).await?;

    if !status.is_success() {
        return Err(assistant_provider_http_error(status, &body));
    }

    let content = protocol.decode_response(&body)?;

    Ok(AssistantRunOutput {
        provider,
        model,
        endpoint_url,
        content,
    })
}

async fn read_assistant_response_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > ASSISTANT_MAX_RESPONSE_BYTES as u64)
    {
        return Err(format!(
            "assistant provider response exceeded the {} byte limit",
            ASSISTANT_MAX_RESPONSE_BYTES
        ));
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(ASSISTANT_MAX_RESPONSE_BYTES as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("failed to read assistant provider response: {error}"))?
    {
        append_assistant_response_chunk(&mut body, &chunk)?;
    }
    Ok(body)
}

fn append_assistant_response_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), String> {
    if body.len().saturating_add(chunk.len()) > ASSISTANT_MAX_RESPONSE_BYTES {
        return Err(format!(
            "assistant provider response exceeded the {} byte limit",
            ASSISTANT_MAX_RESPONSE_BYTES
        ));
    }
    body.extend_from_slice(chunk);
    Ok(())
}

fn assistant_provider_http_error(status: reqwest::StatusCode, body: &[u8]) -> String {
    let body = String::from_utf8_lossy(body);
    format!(
        "assistant provider returned {}: {}",
        status,
        summarize_text(&redact_assistant_provider_text(&body), 320)
    )
}

fn assistant_request_timeout(settings: &AssistantProviderSettings) -> Duration {
    if settings.provider.trim().eq_ignore_ascii_case("ollama")
        || reqwest::Url::parse(settings.base_url.trim())
            .ok()
            .is_some_and(|url| is_loopback_url(&url))
    {
        ASSISTANT_LOCAL_REQUEST_TIMEOUT
    } else {
        ASSISTANT_REMOTE_REQUEST_TIMEOUT
    }
}

pub fn read_secret_status(
    descriptor: &AssistantSecretDescriptor,
) -> Result<AssistantSecretStatus, String> {
    read_secret_status_with_backend(descriptor, &SecretStoreBackend::SystemKeyring)
}

fn read_secret_status_with_backend(
    descriptor: &AssistantSecretDescriptor,
    backend: &SecretStoreBackend,
) -> Result<AssistantSecretStatus, String> {
    if descriptor.provider.trim().is_empty() || descriptor.base_url.trim().is_empty() {
        return Ok(AssistantSecretStatus { stored: false });
    }

    match read_secret_value(descriptor, backend)? {
        Some(secret) => Ok(AssistantSecretStatus {
            stored: !secret.trim().is_empty(),
        }),
        None => Ok(AssistantSecretStatus { stored: false }),
    }
}

pub fn store_secret(
    descriptor: &AssistantSecretDescriptor,
    api_key: &str,
) -> Result<AssistantSecretStatus, String> {
    store_secret_with_backend(descriptor, api_key, &SecretStoreBackend::SystemKeyring)
}

fn store_secret_with_backend(
    descriptor: &AssistantSecretDescriptor,
    api_key: &str,
    backend: &SecretStoreBackend,
) -> Result<AssistantSecretStatus, String> {
    let normalized_api_key = normalize_required(api_key, "apiKey")?;
    write_secret_value(descriptor, &normalized_api_key, backend)?;

    Ok(AssistantSecretStatus { stored: true })
}

pub fn delete_secret(
    descriptor: &AssistantSecretDescriptor,
) -> Result<AssistantSecretStatus, String> {
    delete_secret_with_backend(descriptor, &SecretStoreBackend::SystemKeyring)
}

fn delete_secret_with_backend(
    descriptor: &AssistantSecretDescriptor,
    backend: &SecretStoreBackend,
) -> Result<AssistantSecretStatus, String> {
    if descriptor.provider.trim().is_empty() || descriptor.base_url.trim().is_empty() {
        return Ok(AssistantSecretStatus { stored: false });
    }

    delete_secret_value(descriptor, backend)?;
    Ok(AssistantSecretStatus { stored: false })
}

pub fn summarize_text(value: &str, limit: usize) -> String {
    let collapsed = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.chars().count() <= limit {
        return collapsed;
    }

    let mut summary = collapsed
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    summary.push_str("...");
    summary
}

fn resolve_api_key_with_backend(
    settings: &AssistantProviderSettings,
    backend: &SecretStoreBackend,
) -> Result<String, String> {
    ProviderProtocol::parse(&settings.provider)?;
    let inline = settings.api_key.trim();
    if !inline.is_empty() {
        return Ok(inline.to_string());
    }

    let descriptor = AssistantSecretDescriptor {
        provider: settings.provider.clone(),
        base_url: settings.base_url.clone(),
    };
    match read_secret_value(&descriptor, backend)? {
        Some(secret) => Ok(secret.trim().to_string()),
        None => Ok(String::new()),
    }
}

fn read_secret_value(
    descriptor: &AssistantSecretDescriptor,
    backend: &SecretStoreBackend,
) -> Result<Option<String>, String> {
    let identity = credential_identity(descriptor)?;

    match backend {
        SecretStoreBackend::SystemKeyring => {
            let entry = keyring_entry(&identity)?;
            match entry.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(KeyringError::NoEntry) => Ok(None),
                Err(error) => Err(format!("failed to read assistant credential: {error}")),
            }
        }
        #[cfg(test)]
        SecretStoreBackend::File(path) => {
            let store = read_file_store(path)?;
            Ok(store.get(&identity).cloned())
        }
    }
}

fn write_secret_value(
    descriptor: &AssistantSecretDescriptor,
    secret: &str,
    backend: &SecretStoreBackend,
) -> Result<(), String> {
    let identity = credential_identity(descriptor)?;

    match backend {
        SecretStoreBackend::SystemKeyring => {
            let entry = keyring_entry(&identity)?;
            entry
                .set_password(secret)
                .map_err(|error| format!("failed to store assistant credential: {error}"))
        }
        #[cfg(test)]
        SecretStoreBackend::File(path) => {
            let mut store = read_file_store(path)?;
            store.insert(identity, secret.to_string());
            write_file_store(path, &store)
        }
    }
}

fn delete_secret_value(
    descriptor: &AssistantSecretDescriptor,
    backend: &SecretStoreBackend,
) -> Result<(), String> {
    let identity = credential_identity(descriptor)?;

    match backend {
        SecretStoreBackend::SystemKeyring => {
            let entry = keyring_entry(&identity)?;
            match entry.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
                Err(error) => Err(format!("failed to clear assistant credential: {error}")),
            }
        }
        #[cfg(test)]
        SecretStoreBackend::File(path) => {
            let mut store = read_file_store(path)?;
            if store.remove(&identity).is_none() {
                return Ok(());
            }

            if store.is_empty() {
                if path.exists() {
                    fs::remove_file(path).map_err(|error| {
                        format!("failed to clear assistant credential file: {error}")
                    })?;
                }
                return Ok(());
            }

            write_file_store(path, &store)
        }
    }
}

fn credential_identity(descriptor: &AssistantSecretDescriptor) -> Result<String, String> {
    let provider = ProviderProtocol::parse(&descriptor.provider)?;
    let base_url = normalize_service_url(&descriptor.base_url)?;
    Ok(format!("{}|{}", provider.as_str(), base_url))
}

fn keyring_entry(identity: &str) -> Result<Entry, String> {
    Entry::new(SECRET_SERVICE_NAME, identity)
        .map_err(|error| format!("failed to create assistant credential entry: {error}"))
}

#[cfg(test)]
fn read_file_store(path: &Path) -> Result<BTreeMap<String, String>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read assistant credential file: {error}"))?;
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse assistant credential file: {error}"))
}

#[cfg(test)]
fn write_file_store(path: &Path, store: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("failed to prepare assistant credential directory: {error}")
        })?;
    }

    let body = serde_json::to_string_pretty(store)
        .map_err(|error| format!("failed to encode assistant credential file: {error}"))?;
    fs::write(path, body)
        .map_err(|error| format!("failed to write assistant credential file: {error}"))
}

fn normalize_required(value: &str, field_name: &str) -> Result<String, String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(format!("assistant setting `{field_name}` is required"));
    }

    Ok(normalized.to_string())
}

fn build_ollama_tags_endpoint(base_url: Option<&str>) -> Result<String, String> {
    let candidate = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http://127.0.0.1:11434/v1");
    provider::ollama_endpoint(candidate, "/api/tags")
}

fn build_user_message(input: &AssistantRunInput) -> String {
    [
        format!(
            "Prompt label: {}",
            redact_assistant_provider_text(input.prompt_label.trim())
        ),
        String::new(),
        String::from("Task:"),
        redact_assistant_provider_text(input.prompt.trim()),
        String::new(),
        String::from("Context:"),
        redact_assistant_provider_text(input.context.trim()),
    ]
    .join("\n")
}

const ASSISTANT_REDACTED_VALUE: &str = "[REDACTED]";

pub(crate) fn redact_assistant_provider_text(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }

    if let Ok(mut value) = serde_json::from_str::<Value>(text) {
        redact_assistant_json_value(&mut value, None);
        return serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| String::from(ASSISTANT_REDACTED_VALUE));
    }

    redact_assistant_provider_lines(text)
}

include!("assistant_file_redaction.rs");

fn redact_assistant_provider_lines(text: &str) -> String {
    let xml_redacted = redact_assistant_xml_secrets(text);
    let mut sensitive_yaml_indent = None;
    let mut redacted_text = String::new();
    for segment in xml_redacted.split_inclusive('\n') {
        let (line, ending) = if let Some(line) = segment.strip_suffix("\r\n") {
            (line, "\r\n")
        } else if let Some(line) = segment.strip_suffix('\n') {
            (line, "\n")
        } else {
            (segment, "")
        };
        let indent = line.len().saturating_sub(line.trim_start().len());
        if let Some(block_indent) = sensitive_yaml_indent {
            if line.trim().is_empty() {
                redacted_text.push_str(line);
                redacted_text.push_str(ending);
                continue;
            }
            if indent > block_indent {
                redacted_text.push_str(&format!(
                    "{}{}{}",
                    &line[..indent],
                    ASSISTANT_REDACTED_VALUE,
                    ending
                ));
                continue;
            }
            sensitive_yaml_indent = None;
        }
        if assistant_yaml_sensitive_block_indent(line).is_some() {
            sensitive_yaml_indent = Some(indent);
        }
        redacted_text.push_str(&redact_assistant_provider_line(line));
        redacted_text.push_str(ending);
    }
    redacted_text
}

fn assistant_yaml_sensitive_block_indent(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let delimiter = trimmed.find(':')?;
    let key = trimmed[..delimiter].trim().trim_matches(['"', '\'']);
    let value = trimmed[delimiter + 1..].trim_start();
    let multiline_value = value.is_empty()
        || value
            .split_ascii_whitespace()
            .find(|token| !token.starts_with('&') && !token.starts_with('!'))
            .is_some_and(|token| token.starts_with('|') || token.starts_with('>'))
        || matches!(value.chars().next(), Some('"' | '\''))
            && assistant_quoted_value_end(value).is_none();
    (assistant_key_is_sensitive(key) && multiline_value)
        .then_some(line.len().saturating_sub(trimmed.len()))
}

fn redact_assistant_json_value(value: &mut Value, parent_key: Option<&str>) {
    if parent_key.is_some_and(assistant_key_is_sensitive) {
        *value = Value::String(String::from(ASSISTANT_REDACTED_VALUE));
        return;
    }

    match value {
        Value::Object(object) => {
            for (key, child) in object {
                redact_assistant_json_value(child, Some(key));
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                redact_assistant_json_value(entry, parent_key);
            }
        }
        Value::String(text) => {
            *text = redact_assistant_provider_lines(text);
        }
        _ => {}
    }
}

pub(crate) fn redact_assistant_setting_schema(setting_key: &str, schema: &Value) -> Value {
    let mut redacted = schema.clone();
    if assistant_key_is_sensitive(setting_key) {
        redact_assistant_schema_values(&mut redacted);
    }
    redacted
}

fn redact_assistant_schema_values(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            for (keyword, value) in object {
                match keyword.as_str() {
                    "default" | "examples" | "enum" | "const" => {
                        *value = Value::String(String::from(ASSISTANT_REDACTED_VALUE));
                    }
                    // These objects map user-defined names to schemas. Their
                    // names may themselves be "default" or "enum".
                    "properties" | "patternProperties" | "$defs" | "definitions"
                    | "dependentSchemas" | "dependencies" => {
                        if let Some(schemas) = value.as_object_mut() {
                            for schema in schemas.values_mut() {
                                redact_assistant_schema_values(schema);
                            }
                        }
                    }
                    _ => redact_assistant_schema_values(value),
                }
            }
        }
        Value::Array(schemas) => {
            for schema in schemas {
                redact_assistant_schema_values(schema);
            }
        }
        _ => {}
    }
}

fn assistant_key_is_sensitive(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "password",
        "passwd",
        "pwd",
        "token",
        "secret",
        "apikey",
        "accesskey",
        "privatekey",
        "sessionkey",
        "credential",
        "authorization",
        "cookie",
        "rcon",
        "admin",
        "path",
        "directory",
        "root",
    ]
    .iter()
    .any(|keyword| normalized.contains(keyword))
}

fn assistant_xml_attribute_value(line: &str, attribute: &str) -> Option<(usize, usize, String)> {
    let lower = line.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find(attribute) {
        let start = cursor + relative;
        let before_is_boundary = start == 0
            || lower[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_whitespace() || character == '<');
        let mut equals = start + attribute.len();
        while lower
            .as_bytes()
            .get(equals)
            .is_some_and(u8::is_ascii_whitespace)
        {
            equals += 1;
        }
        if !before_is_boundary || lower.as_bytes().get(equals) != Some(&b'=') {
            cursor = start + attribute.len();
            continue;
        }
        let mut value_start = equals + 1;
        while lower
            .as_bytes()
            .get(value_start)
            .is_some_and(u8::is_ascii_whitespace)
        {
            value_start += 1;
        }
        let quote = *line.as_bytes().get(value_start)?;
        if !matches!(quote, b'\'' | b'"') {
            cursor = start + attribute.len();
            continue;
        }
        value_start += 1;
        let value_end = line.as_bytes()[value_start..]
            .iter()
            .position(|byte| *byte == quote)
            .map(|offset| value_start + offset)?;
        return Some((
            value_start,
            value_end,
            line[value_start..value_end].to_string(),
        ));
    }
    None
}

fn redact_assistant_xml_secrets(text: &str) -> String {
    let mut redacted = text.to_string();
    let mut cursor = 0;
    loop {
        let lower = redacted.to_ascii_lowercase();
        let Some(open_start) = lower[cursor..].find('<').map(|offset| cursor + offset) else {
            break;
        };
        let Some(open_end) = lower[open_start..]
            .find('>')
            .map(|offset| open_start + offset)
        else {
            break;
        };
        let opening = redacted[open_start..=open_end].to_string();
        let tag_name_start = open_start + 1;
        if lower[tag_name_start..]
            .chars()
            .next()
            .is_some_and(|character| matches!(character, '/' | '!' | '?'))
        {
            cursor = open_end + 1;
            continue;
        }
        let tag_name_end = lower[tag_name_start..]
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '/' | '>')
            })
            .map(|offset| tag_name_start + offset)
            .unwrap_or(open_end);
        let tag = lower[tag_name_start..tag_name_end].to_string();
        if tag.is_empty() {
            cursor = open_end + 1;
            continue;
        }
        let sensitive_name = assistant_xml_attribute_value(&opening, "name")
            .is_some_and(|(_, _, name)| assistant_key_is_sensitive(&name));
        let sensitive_tag = assistant_key_is_sensitive(&tag);
        if !sensitive_name && !sensitive_tag {
            cursor = open_end + 1;
            continue;
        }

        if let Some((value_start, value_end, value)) =
            assistant_xml_attribute_value(&opening, "value")
            && value != ASSISTANT_REDACTED_VALUE
        {
            redacted.replace_range(
                open_start + value_start..open_start + value_end,
                ASSISTANT_REDACTED_VALUE,
            );
            cursor = open_start;
            continue;
        }
        if opening.trim_end().ends_with("/>") {
            cursor = open_end + 1;
            continue;
        }

        let closing = format!("</{tag}");
        let Some(close_start) = lower[open_end + 1..]
            .find(&closing)
            .map(|offset| open_end + 1 + offset)
        else {
            cursor = open_end + 1;
            continue;
        };
        if &redacted[open_end + 1..close_start] != ASSISTANT_REDACTED_VALUE {
            redacted.replace_range(open_end + 1..close_start, ASSISTANT_REDACTED_VALUE);
            cursor = open_start;
        } else {
            cursor = close_start + 1;
        }
    }
    redacted
}

fn redact_assistant_provider_line(line: &str) -> String {
    let mut redacted = redact_assistant_xml_secrets(line);
    while let Some((delimiter_index, value_start)) = find_assistant_sensitive_assignment(&redacted)
    {
        let value_end = assistant_assignment_value_end(&redacted, value_start);
        let mut next = String::with_capacity(redacted.len());
        next.push_str(&redacted[..value_start]);
        next.push_str(ASSISTANT_REDACTED_VALUE);
        next.push_str(&redacted[value_end..]);
        if next == redacted || delimiter_index >= next.len() {
            break;
        }
        redacted = next;
    }

    redact_assistant_absolute_paths(&redact_assistant_whitespace_secret(&redacted))
}

fn find_assistant_sensitive_assignment(line: &str) -> Option<(usize, usize)> {
    for (delimiter_index, delimiter) in line.char_indices() {
        if delimiter != '=' && delimiter != ':' {
            continue;
        }
        let prefix = &line[..delimiter_index];
        let key_start = prefix
            .char_indices()
            .rev()
            .find_map(|(index, character)| {
                (!character.is_ascii_alphanumeric()
                    && !matches!(character, '_' | '-' | '.' | '"' | '\''))
                .then_some(index + character.len_utf8())
            })
            .unwrap_or(0);
        let key = prefix[key_start..]
            .trim()
            .trim_matches(['"', '\'', '[', ']']);
        if !assistant_key_is_sensitive(key) {
            continue;
        }
        let value_start = delimiter_index
            + delimiter.len_utf8()
            + line[delimiter_index + delimiter.len_utf8()..]
                .len()
                .saturating_sub(
                    line[delimiter_index + delimiter.len_utf8()..]
                        .trim_start()
                        .len(),
                );
        if line[value_start..].starts_with(ASSISTANT_REDACTED_VALUE) {
            continue;
        }
        return Some((delimiter_index, value_start));
    }
    None
}

fn assistant_assignment_value_end(line: &str, value_start: usize) -> usize {
    let tail = &line[value_start..];
    let Some(first) = tail.chars().next() else {
        return value_start;
    };
    if first == '"' || first == '\'' {
        return assistant_quoted_value_end(tail)
            .map(|end| value_start + end)
            .unwrap_or(line.len());
    }

    tail.char_indices()
        .find_map(|(index, character)| {
            (character.is_whitespace() || matches!(character, ',' | ';' | '&' | '}' | ']'))
                .then_some(value_start + index)
        })
        .unwrap_or(line.len())
}

fn assistant_quoted_value_end(value: &str) -> Option<usize> {
    let quote = value.chars().next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }
    let quote_len = quote.len_utf8();
    let mut characters = value[quote_len..].char_indices().peekable();
    let mut escaped = false;
    while let Some((index, character)) = characters.next() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == quote {
            // Configuration formats use either backslash escapes or doubled
            // quotes. Neither ends the secret value; retain no suffix bytes.
            if characters.peek().is_some_and(|(_, next)| *next == quote) {
                characters.next();
            } else {
                return Some(quote_len + index + quote_len);
            }
        }
    }
    None
}

fn redact_assistant_whitespace_secret(line: &str) -> String {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    for pair in tokens.windows(2) {
        let key = pair[0].trim_matches(['"', '\'', '[', ']', '(', ')', ':', '=']);
        if !assistant_key_is_sensitive(key) {
            continue;
        }
        let Some(value_index) = line.find(pair[1]) else {
            continue;
        };
        return format!("{}{}", &line[..value_index], ASSISTANT_REDACTED_VALUE);
    }
    line.to_string()
}

fn redact_assistant_absolute_paths(line: &str) -> String {
    // A bounded file can still contain thousands of paths. Iteration keeps its
    // stack usage independent of the number of matches in untrusted evidence.
    let mut redacted = String::with_capacity(line.len());
    let mut remaining = line;
    while let Some(start) = assistant_absolute_path_start(remaining) {
        let end = remaining[start..]
            .char_indices()
            .find_map(|(offset, character)| {
                (offset > 0 && matches!(character, '"' | '\'' | ',' | ';' | ')' | ']' | '}'))
                    .then_some(start + offset)
            })
            .unwrap_or(remaining.len());
        redacted.push_str(&remaining[..start]);
        redacted.push_str("[REDACTED_PATH]");
        remaining = &remaining[end..];
    }
    redacted.push_str(remaining);
    redacted
}

fn assistant_absolute_path_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    for index in 0..bytes.len() {
        let starts_at_boundary = index == 0
            || matches!(
                bytes[index - 1],
                b' ' | b'\t' | b'"' | b'\'' | b'`' | b'=' | b'(' | b'[' | b'{'
            );
        let windows_drive = index + 2 < bytes.len()
            && starts_at_boundary
            && bytes[index].is_ascii_alphabetic()
            && bytes[index + 1] == b':'
            && matches!(bytes[index + 2], b'/' | b'\\');
        let unc_path =
            index + 1 < bytes.len() && bytes[index] == b'\\' && bytes[index + 1] == b'\\';
        let absolute_slash_path = bytes[index] == b'/'
            && index + 1 < bytes.len()
            && (index == 0
                || matches!(
                    bytes[index - 1],
                    b' ' | b'\t' | b'"' | b'\'' | b'`' | b'=' | b'(' | b'[' | b'{'
                )
                || bytes[index - 1] == b':' && bytes[index + 1] != b'/');
        if windows_drive || unc_path || absolute_slash_path {
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_secret_store_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "langame-assistant-secret-smoke-{}.json",
            uuid::Uuid::new_v4().simple()
        ))
    }

    #[test]
    fn build_ollama_tags_endpoint_normalizes_v1_root() {
        assert_eq!(
            build_ollama_tags_endpoint(Some("http://127.0.0.1:11434/v1/"))
                .expect("normalize v1 root"),
            "http://127.0.0.1:11434/api/tags"
        );
        assert_eq!(
            build_ollama_tags_endpoint(Some("http://127.0.0.1:11434"))
                .expect("normalize plain root"),
            "http://127.0.0.1:11434/api/tags"
        );
    }

    #[test]
    fn build_ollama_tags_endpoint_uses_default_when_missing() {
        assert_eq!(
            build_ollama_tags_endpoint(None).expect("default ollama tags endpoint"),
            "http://127.0.0.1:11434/api/tags"
        );
    }

    #[test]
    fn assistant_request_timeout_allows_local_model_cold_start() {
        for settings in [
            AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("qwen"),
                base_url: String::from("http://192.168.31.51:11434/v1"),
                api_key: String::new(),
            },
            AssistantProviderSettings {
                provider: String::from("openai-compatible"),
                model: String::from("local-model"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            AssistantProviderSettings {
                provider: String::from("openai-compatible"),
                model: String::from("local-model"),
                base_url: String::from("http://[::1]:11434/v1"),
                api_key: String::new(),
            },
        ] {
            assert_eq!(
                assistant_request_timeout(&settings),
                ASSISTANT_LOCAL_REQUEST_TIMEOUT
            );
        }
    }

    #[test]
    fn assistant_request_timeout_keeps_remote_failures_bounded() {
        let settings = AssistantProviderSettings {
            provider: String::from("openai-compatible"),
            model: String::from("remote-model"),
            base_url: String::from("https://example.test/v1"),
            api_key: String::new(),
        };

        assert_eq!(
            assistant_request_timeout(&settings),
            ASSISTANT_REMOTE_REQUEST_TIMEOUT
        );
    }

    #[test]
    fn ollama_chat_request_disables_reasoning_without_changing_other_providers() {
        let input = AssistantRunInput {
            settings: AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("qwen3.5:9b"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            prompt_label: String::from("smoke"),
            prompt: String::from("Reply with exactly OK."),
            context: String::new(),
        };

        let user_message = build_user_message(&input);
        let ollama =
            ProviderProtocol::Ollama.request_body("qwen3.5:9b", "System prompt", &user_message);
        assert_eq!(ollama["reasoning_effort"], "none");
        assert_eq!(ollama["stream"], false);

        let compatible = ProviderProtocol::OpenAiCompatible.request_body(
            "remote-model",
            "System prompt",
            &user_message,
        );
        assert!(compatible.get("reasoning_effort").is_none());
    }

    #[test]
    fn assistant_response_chunk_enforces_the_total_body_limit() {
        let mut body = vec![b'x'; ASSISTANT_MAX_RESPONSE_BYTES - 1];
        append_assistant_response_chunk(&mut body, b"y").expect("exact response limit");
        assert_eq!(body.len(), ASSISTANT_MAX_RESPONSE_BYTES);

        let error = append_assistant_response_chunk(&mut body, b"z")
            .expect_err("response beyond limit must fail");
        assert!(error.contains("exceeded"));
        assert_eq!(body.len(), ASSISTANT_MAX_RESPONSE_BYTES);
    }

    #[test]
    fn assistant_provider_http_error_redacts_body_before_reporting_it() {
        let credential = ["provider", "credential", "fixture"].join("-");
        let body = serde_json::to_vec(&json!({
            "error": {
                "message": format!("token={credential}"),
                "api_key": credential,
                "path": "C:/fixture/private/provider.log"
            }
        }))
        .expect("serialize provider error fixture");

        let message = assistant_provider_http_error(reqwest::StatusCode::BAD_REQUEST, &body);
        assert!(message.contains("400 Bad Request"));
        assert!(!message.contains("provider-credential-fixture"));
        assert!(!message.contains("C:/fixture/private/provider.log"));
        assert!(message.contains(ASSISTANT_REDACTED_VALUE));
    }

    #[test]
    fn assistant_provider_message_recursively_redacts_structured_secrets_and_paths() {
        let credential = ["fixture", "credential"].join("-");
        let input = AssistantRunInput {
            settings: AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("qwen"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            prompt_label: String::from("config review"),
            prompt: serde_json::to_string(&json!({
                "server": {
                    "name": "safe-name",
                    "rcon_password": credential.clone(),
                    "admins": [{"password": credential.clone()}],
                    "save_path": "Z:/fixture/private/world"
                },
                "metadata": {"public": true, "api_key": credential.clone()}
            }))
            .expect("serialize test prompt"),
            context: String::from("safe context"),
        };

        let message = build_user_message(&input);
        assert!(message.contains("safe-name"));
        assert!(!message.contains(&credential));
        assert!(!message.contains("Z:/fixture/private/world"));
        assert!(message.matches(ASSISTANT_REDACTED_VALUE).count() >= 4);
    }

    #[test]
    fn setting_schema_redaction_hides_values_and_preserves_descriptive_metadata() {
        let credential = ["schema", "fixture", "credential"].join("-");
        let schema = json!({
            "type": "string",
            "title": "Administrator password",
            "description": "Password used for server administration.",
            "default": credential,
            "examples": [credential],
            "enum": [credential],
            "const": credential,
            "anyOf": [{"type": "string", "const": credential}]
        });
        let redacted = redact_assistant_setting_schema("admin_password", &schema);
        assert!(!redacted.to_string().contains(&credential));
        for keyword in ["default", "examples", "enum", "const"] {
            assert_eq!(redacted[keyword], ASSISTANT_REDACTED_VALUE);
        }
        for keyword in ["type", "title", "description"] {
            assert_eq!(redacted[keyword], schema[keyword]);
        }
        assert_eq!(redacted["anyOf"][0]["type"], "string");
        assert_eq!(redacted["anyOf"][0]["const"], ASSISTANT_REDACTED_VALUE);
        assert_eq!(
            schema["default"], credential,
            "source schema stays unchanged"
        );
    }

    #[test]
    fn setting_schema_redaction_preserves_public_defaults_and_schema_property_names() {
        let public_schema = json!({"type": "integer", "default": 8, "enum": [8, 16]});
        assert_eq!(
            redact_assistant_setting_schema("max_players", &public_schema),
            public_schema
        );
        let credential = ["nested", "fixture", "credential"].join("-");
        let schema = json!({
            "type": "object",
            "properties": {
                "default": {"type": "string", "title": "Default account", "default": credential}
            },
            "$defs": {"enum": {"type": "string", "examples": [credential]}}
        });
        let redacted = redact_assistant_setting_schema("session_token", &schema);
        assert!(!redacted.to_string().contains(&credential));
        assert_eq!(redacted["properties"]["default"]["type"], "string");
        assert_eq!(
            redacted["properties"]["default"]["title"],
            "Default account"
        );
        assert_eq!(
            redacted["properties"]["default"]["default"],
            ASSISTANT_REDACTED_VALUE
        );
        assert_eq!(redacted["$defs"]["enum"]["type"], "string");
        assert_eq!(
            redacted["$defs"]["enum"]["examples"],
            ASSISTANT_REDACTED_VALUE
        );
    }

    #[test]
    fn tool_protocol_preserves_prepared_contracts_and_excludes_unselected_input_fields() {
        let secrets = (0..3)
            .map(|index| format!("prepared-fixture-{index}"))
            .collect::<Vec<_>>();
        let contract = r#"{"tool":"read_config_file","path":"server.properties","offset":0}"#;
        let evidence = serde_json::to_string(&json!({"password": secrets[0]})).unwrap();
        let mut input = AssistantRunInput {
            settings: AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("fixture-model"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            prompt_label: format!("token={}", secrets[1]),
            prompt: evidence.clone(),
            context: format!("password={}", secrets[2]),
        };
        let ordinary = build_user_message(&input);
        for secret in &secrets {
            assert!(!ordinary.contains(secret));
        }

        input.prompt = format!(
            "Available tool:\n{contract}\nUntrusted evidence:\n{}",
            redact_assistant_provider_text(&evidence)
        );
        let contract_line = input
            .prompt
            .lines()
            .find(|line| line.starts_with("{\"tool\":"))
            .expect("complete tool contract");
        let parsed: Value = serde_json::from_str(contract_line).expect("valid tool contract JSON");
        assert_eq!(parsed["path"], "server.properties");
        assert_eq!(contract_line, contract);
        let body = ProviderProtocol::Ollama
            .tool_request_body(
                "fixture-model",
                "Use the structured tools.",
                &[AssistantToolMessage::User(input.prompt.clone())],
                &[],
            )
            .unwrap();
        assert_eq!(body["messages"][1]["content"], input.prompt);
        for secret in &secrets {
            assert!(!body.to_string().contains(secret));
        }
    }

    #[test]
    fn assistant_provider_message_redacts_config_log_and_query_assignments() {
        let credentials = (0..4)
            .map(|index| format!("fixture-credential-{index}"))
            .collect::<Vec<_>>();
        let input = AssistantRunInput {
            settings: AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("qwen"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            prompt_label: String::from("diagnose"),
            prompt: String::from("Why did the server fail?"),
            context: [
                format!("{}={}", "RconPassword", credentials[0]),
                format!("{}: Bearer {}", "Authorization", credentials[1]),
                format!("https://example.test/status?{}={}", "token", credentials[2]),
                format!("--{} {}", "api-key", credentials[3]),
                String::from("ordinary=value"),
            ]
            .join("\n"),
        };

        let message = build_user_message(&input);
        for credential in credentials {
            assert!(!message.contains(&credential));
        }
        assert!(message.contains("ordinary=value"));
        assert!(message.matches(ASSISTANT_REDACTED_VALUE).count() >= 3);
    }

    #[test]
    fn assistant_provider_message_redacts_xml_yaml_and_unix_paths() {
        let values = (0..7)
            .map(|index| format!("fixture-sensitive-value-{index}"))
            .collect::<Vec<_>>();
        let input = AssistantRunInput {
            settings: AssistantProviderSettings {
                provider: String::from("ollama"),
                model: String::from("qwen"),
                base_url: String::from("http://127.0.0.1:11434/v1"),
                api_key: String::new(),
            },
            prompt_label: String::from("diagnose"),
            prompt: String::from("Inspect configuration safely"),
            context: [
                format!(
                    r#"<property name="PublicName" value="safe"/><property name="ServerPassword" value="{}"/>"#,
                    values[0]
                ),
                format!(
                    "<property\n name=\"AdminToken\"\n value=\"{}\"\n/>",
                    values[1]
                ),
                format!("<Password>\n{}\n</Password>", values[2]),
                format!("<Password value=\"{}\"/>", values[3]),
                format!(
                    "api_token: &credential |\n  {}\n  {}",
                    values[4], values[5]
                ),
                format!("nested_password:\n  {}", values[6]),
                String::from("source: `/home/operator/private/server.yaml`"),
                String::from("config:/home/operator/private/alternate.yaml"),
                String::from("share: //server/private/config.ini"),
                String::from("docs: https://example.test/public/config"),
            ]
            .join("\n"),
        };

        let message = build_user_message(&input);
        for value in values {
            assert!(!message.contains(&value));
        }
        assert!(!message.contains("/home/operator"));
        assert!(!message.contains("//server/private"));
        assert!(message.contains("https://example.test/public/config"));
        assert!(message.matches(ASSISTANT_REDACTED_VALUE).count() >= 7);
        assert!(message.matches("[REDACTED_PATH]").count() >= 3);
    }

    #[test]
    fn system_prompt_identifies_lan_assistant() {
        assert!(SYSTEM_PROMPT.starts_with("You are LAN,"));
        assert!(SYSTEM_PROMPT.contains("operator-focused"));
    }

    #[test]
    fn credentials_are_isolated_by_protocol_and_case_sensitive_path() {
        let store_path = temp_secret_store_path();
        let backend = SecretStoreBackend::File(store_path.clone());
        let original = AssistantSecretDescriptor {
            provider: String::from("openai-compatible"),
            base_url: String::from("https://gateway.example/Tenant/v1"),
        };
        store_secret_with_backend(&original, "fixture-key", &backend).unwrap();
        for descriptor in [
            AssistantSecretDescriptor {
                provider: String::from("anthropic-compatible"),
                base_url: original.base_url.clone(),
            },
            AssistantSecretDescriptor {
                provider: String::from("ollama"),
                base_url: original.base_url.clone(),
            },
            AssistantSecretDescriptor {
                provider: original.provider.clone(),
                base_url: String::from("https://gateway.example/tenant/v1"),
            },
        ] {
            assert!(
                !read_secret_status_with_backend(&descriptor, &backend)
                    .unwrap()
                    .stored
            );
        }
        delete_secret_with_backend(&original, &backend).unwrap();
        assert!(!store_path.exists());
    }

    #[test]
    fn secret_roundtrip_file_backend_smoke() {
        let store_path = temp_secret_store_path();
        let backend = SecretStoreBackend::File(store_path.clone());
        let original_descriptor = AssistantSecretDescriptor {
            provider: String::from("OpenAI-Compatible"),
            base_url: String::from("https://api.openai.com/v1/"),
        };
        let normalized_descriptor = AssistantSecretDescriptor {
            provider: String::from("openai-compatible"),
            base_url: String::from("https://api.openai.com/v1"),
        };

        assert!(
            !read_secret_status_with_backend(&original_descriptor, &backend)
                .expect("read empty secret status")
                .stored
        );

        store_secret_with_backend(&original_descriptor, "  sk-test-roundtrip  ", &backend)
            .expect("store secret");

        assert!(
            read_secret_status_with_backend(&normalized_descriptor, &backend)
                .expect("read stored secret status")
                .stored
        );

        let resolved = resolve_api_key_with_backend(
            &AssistantProviderSettings {
                provider: normalized_descriptor.provider.clone(),
                model: String::from("fixture-model"),
                base_url: normalized_descriptor.base_url.clone(),
                api_key: String::new(),
            },
            &backend,
        )
        .expect("resolve stored secret");
        assert_eq!(resolved, "sk-test-roundtrip");

        let inline = resolve_api_key_with_backend(
            &AssistantProviderSettings {
                provider: normalized_descriptor.provider.clone(),
                model: String::from("fixture-model"),
                base_url: normalized_descriptor.base_url.clone(),
                api_key: String::from("inline-secret"),
            },
            &backend,
        )
        .expect("resolve inline secret");
        assert_eq!(inline, "inline-secret");

        delete_secret_with_backend(&normalized_descriptor, &backend).expect("delete secret");
        assert!(
            !read_secret_status_with_backend(&original_descriptor, &backend)
                .expect("read cleared secret status")
                .stored
        );
        assert!(!store_path.exists());
    }

    #[test]
    fn mock_assistant_infer_action_prefers_gm_over_start() {
        let prompt = mock_assistant_normalized_prompt(
            "User request:\nUse the AI GM tool on the selected Rust server to show server status now. Only send one GM command; do not broadcast, change config, kick players, ban players, or start the server.\n\nActions:\n- run_gm_command: send GM commands\n- start_server: start now",
        );

        assert_eq!(mock_assistant_infer_action(&prompt), "runGmCommand");
    }

    #[test]
    fn mock_assistant_task_is_start_command_respects_do_not_start() {
        let start_prompt = mock_assistant_normalized_prompt(
            "User request:\nStart the selected server now.\n\nActions:",
        );
        assert!(mock_assistant_task_is_start_command(&start_prompt));

        let no_start_prompt = mock_assistant_normalized_prompt(
            "User request:\nUse the AI GM tool on the selected Rust server to show server status now. Only send one GM command; do not broadcast, change config, kick players, ban players, or start the server.\n\nActions:\n- run_gm_command\n- start_server\n",
        );
        assert!(!mock_assistant_task_is_start_command(&no_start_prompt));
    }

    #[test]
    fn mock_assistant_task_is_start_command_ignores_restart_requests() {
        let restart_prompt = mock_assistant_normalized_prompt(
            "User request:\nRestart the selected server now.\n\nActions:\n- start_server: start now\n",
        );
        assert!(!mock_assistant_task_is_start_command(&restart_prompt));
        assert_eq!(
            mock_assistant_infer_action(&restart_prompt),
            "none",
            "restart requests should remain a safe no-op action in mock inference",
        );
    }

    #[test]
    fn mock_assistant_infer_gm_command_from_status_prompt() {
        let prompt = mock_assistant_normalized_prompt(
            "User request:\nUse the AI GM tool on the selected Rust server to show server status now. Only send one GM command; do not broadcast, change config, kick players, ban players, or start the server.\n\nActions:",
        );

        assert_eq!(mock_assistant_infer_gm_command(&prompt), "status");
    }

    #[test]
    fn mock_assistant_infer_action_prefers_mock_marker() {
        let prompt = mock_assistant_normalized_prompt(
            "User request:\nmock-action:apply_beginner_config\nmock-settings-patch:{\"max_players\":24}",
        );

        assert_eq!(mock_assistant_infer_action(&prompt), "applyBeginnerConfig");
    }

    #[test]
    fn mock_assistant_infer_runtime_commands_from_markers() {
        let prompt = mock_assistant_normalized_prompt(
            "User request:\nmock-action:runGmCommand\nmock-runtime-commands:[\"list\",\"status\"]",
        );

        assert_eq!(
            mock_assistant_infer_runtime_commands(&prompt),
            vec![String::from("list"), String::from("status")]
        );
        assert_eq!(mock_assistant_infer_transport(None, &prompt), "source_rcon");
    }

    #[test]
    fn mock_assistant_infer_explicit_mock_patches() {
        let prompt = mock_assistant_normalized_prompt(
            "User request:\nmock-action:repairPorts\nmock-port-patch:{\"game\":28016}",
        );

        assert_eq!(
            mock_assistant_infer_port_patch(&prompt),
            Some(json!({"game":28016}))
        );

        let patch_prompt = mock_assistant_normalized_prompt(
            "User request:\nmock-action:runGmCommand\nmock-runtime-commands:[\"status\"]",
        );
        assert_eq!(
            mock_assistant_infer_runtime_commands(&patch_prompt),
            vec![String::from("status")]
        );
        assert_eq!(
            mock_assistant_infer_transport(None, &patch_prompt),
            String::from("source_rcon")
        );
    }

    #[test]
    fn mock_assistant_infer_port_patch_from_inline_sentence() {
        let patch = json!({"game":28016, "query_port":28999});
        let prompt = mock_assistant_normalized_prompt(&format!(
            "Update ports with mock-action:repairPorts and mock-port-patch:{}.",
            patch
        ));

        assert_eq!(
            mock_assistant_infer_action(&prompt),
            String::from("repairPorts")
        );
        assert_eq!(
            mock_assistant_infer_port_patch(&prompt),
            Some(json!({"game":28016, "query_port":28999})),
        );
    }
}
