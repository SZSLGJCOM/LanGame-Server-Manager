use reqwest::{Client, RequestBuilder, Url};
use serde::Deserialize;
use serde_json::{Value, json};

const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const ANTHROPIC_MAX_OUTPUT_TOKENS: u32 = 4096;
const OLLAMA_INVESTIGATION_CONTEXT_TOKENS: u32 = 32_768;
const OLLAMA_INVESTIGATION_OUTPUT_TOKENS: u32 = 8_192;

#[path = "assistant_tool_protocol.rs"]
mod tool_protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderProtocol {
    OpenAiCompatible,
    AnthropicCompatible,
    Ollama,
}

impl ProviderProtocol {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            "anthropic-compatible" => Ok(Self::AnthropicCompatible),
            "ollama" => Ok(Self::Ollama),
            _ => Err(String::from("unsupported assistant provider protocol")),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai-compatible",
            Self::AnthropicCompatible => "anthropic-compatible",
            Self::Ollama => "ollama",
        }
    }

    pub(super) fn endpoint(self, base_url: &str) -> Result<String, String> {
        if self == Self::Ollama {
            return ollama_endpoint(base_url, "/v1/chat/completions");
        }
        let base = normalize_service_url(base_url)?;
        let route = match self {
            Self::AnthropicCompatible => "/messages",
            Self::OpenAiCompatible | Self::Ollama => "/chat/completions",
        };
        if base.ends_with(route) {
            Ok(base)
        } else {
            Ok(format!("{base}{route}"))
        }
    }

    pub(super) fn request_body(self, model: &str, system: &str, user: &str) -> Value {
        match self {
            Self::AnthropicCompatible => json!({
                "model": model,
                "max_tokens": ANTHROPIC_MAX_OUTPUT_TOKENS,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
                "stream": false
            }),
            Self::OpenAiCompatible | Self::Ollama => {
                let mut body = json!({
                    "model": model,
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": user }
                    ],
                    "stream": false
                });
                if self == Self::Ollama {
                    body["reasoning_effort"] = json!("none");
                }
                body
            }
        }
    }

    pub(super) fn request(
        self,
        client: &Client,
        endpoint: &str,
        body: &Value,
        api_key: &str,
    ) -> RequestBuilder {
        let request = client.post(endpoint).json(body);
        match self {
            Self::AnthropicCompatible => request
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header("x-api-key", api_key),
            Self::OpenAiCompatible | Self::Ollama if !api_key.is_empty() => {
                request.bearer_auth(api_key)
            }
            Self::OpenAiCompatible | Self::Ollama => request,
        }
    }

    pub(super) fn decode_response(self, body: &[u8]) -> Result<String, String> {
        let content = match self {
            Self::AnthropicCompatible => {
                let payload: MessagesResponse = serde_json::from_slice(body)
                    .map_err(|error| format!("failed to decode assistant response: {error}"))?;
                match payload.stop_reason.as_deref() {
                    Some("end_turn") => {}
                    Some("max_tokens") => {
                        return Err(String::from(
                            "assistant response exceeded its output token limit",
                        ));
                    }
                    _ => {
                        return Err(String::from("assistant response did not complete normally"));
                    }
                }
                join_text(payload.content.iter().filter_map(|part| match part {
                    MessageBlock::Text { text } => Some(text.as_str()),
                    MessageBlock::Other => None,
                }))
            }
            Self::OpenAiCompatible | Self::Ollama => {
                let payload: ChatCompletionsResponse = serde_json::from_slice(body)
                    .map_err(|error| format!("failed to decode assistant response: {error}"))?;
                let choice = payload.choices.first().ok_or_else(|| {
                    String::from("assistant response did not include message text")
                })?;
                match choice.finish_reason.as_deref() {
                    Some("stop") => {}
                    Some("length") => {
                        return Err(String::from(
                            "assistant response exceeded its output token limit",
                        ));
                    }
                    _ => {
                        return Err(String::from("assistant response did not complete normally"));
                    }
                }
                if choice
                    .message
                    .refusal
                    .as_deref()
                    .is_some_and(|refusal| !refusal.trim().is_empty())
                {
                    return Err(String::from(
                        "assistant provider refused the requested text generation",
                    ));
                }
                match &choice.message.content {
                    Some(ChatMessageContent::Text(text)) => join_text([text.as_str()]),
                    Some(ChatMessageContent::Parts(parts)) => {
                        join_text(parts.iter().filter_map(|part| part.text.as_deref()))
                    }
                    None => None,
                }
            }
        };
        content.ok_or_else(|| String::from("assistant response did not include message text"))
    }
}

pub(super) fn ollama_endpoint(base_url: &str, route: &str) -> Result<String, String> {
    let base = normalize_service_url(base_url)?;
    let mut endpoint = Url::parse(&base)
        .map_err(|_| String::from("assistant service URL must be an absolute URL"))?;
    let path = endpoint.path().trim_end_matches('/');
    // Settings accept either a service root or one of its public request URLs.
    // Every Ollama entry point must resolve that same root, including proxies.
    let prefix = path
        .strip_suffix("/v1/chat/completions")
        .or_else(|| path.strip_suffix("/chat/completions"))
        .or_else(|| path.strip_suffix("/v1"))
        .or_else(|| path.strip_suffix("/api/chat"))
        .or_else(|| path.strip_suffix("/api/tags"))
        .unwrap_or(path);
    endpoint.set_path(&format!("{prefix}{route}"));
    Ok(endpoint.to_string())
}

pub(super) fn normalize_service_url(value: &str) -> Result<String, String> {
    let parsed = Url::parse(value.trim())
        .map_err(|_| String::from("assistant service URL must be an absolute URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(String::from(
            "assistant service URL must use HTTP or HTTPS without credentials, query, or fragment",
        ));
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

fn join_text<'a>(parts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let content = parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!content.is_empty()).then_some(content)
}

#[derive(Deserialize)]
struct ChatCompletionsResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<ChatMessageContent>,
    #[serde(default)]
    refusal: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatMessagePart>),
}

#[derive(Deserialize)]
struct ChatMessagePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<MessageBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum MessageBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[cfg(test)]
#[path = "assistant_provider_tests.rs"]
mod tests;
