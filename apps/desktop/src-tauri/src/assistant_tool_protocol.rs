use super::super::{AssistantToolDefinition, AssistantToolMessage, AssistantToolReply};
use super::{
    ANTHROPIC_MAX_OUTPUT_TOKENS, OLLAMA_INVESTIGATION_CONTEXT_TOKENS,
    OLLAMA_INVESTIGATION_OUTPUT_TOKENS, ProviderProtocol, ollama_endpoint,
};
use serde_json::{Value, json};
use std::collections::HashSet;

#[path = "assistant_tool_protocol_decode.rs"]
mod decode;

#[path = "assistant_tool_stream.rs"]
mod stream;

impl ProviderProtocol {
    pub(in crate::assistant) fn tool_endpoint(self, base_url: &str) -> Result<String, String> {
        if self != Self::Ollama {
            return self.endpoint(base_url);
        }
        ollama_endpoint(base_url, "/api/chat")
    }

    pub(in crate::assistant) fn tool_request_body(
        self,
        model: &str,
        system: &str,
        messages: &[AssistantToolMessage],
        tools: &[AssistantToolDefinition],
    ) -> Result<Value, String> {
        let mut names = HashSet::new();
        if tools.len() > 64 {
            return Err(String::from(
                "assistant tool definition count exceeded its limit",
            ));
        }
        for tool in tools {
            super::super::tool_conversation::validate_tool_identity("definition", &tool.name)?;
            if !names.insert(&tool.name) || !tool.parameters.is_object() {
                return Err(String::from(
                    "assistant tool definitions must have unique names and object schemas",
                ));
            }
        }
        let definitions: Vec<Value> = tools.iter().map(|tool| match self {
            Self::AnthropicCompatible => json!({"name":tool.name,"description":tool.description,"input_schema":tool.parameters}),
            Self::OpenAiCompatible | Self::Ollama => json!({"type":"function","function":{
                "name":tool.name,"description":tool.description,"parameters":tool.parameters
            }}),
        }).collect();
        let mut native_messages = Vec::new();
        if self != Self::AnthropicCompatible {
            native_messages.push(json!({"role":"system","content":system}));
        }
        for message in messages {
            match message {
                AssistantToolMessage::User(content) => {
                    native_messages.push(json!({"role":"user","content":content}))
                }
                AssistantToolMessage::Assistant(reply) => {
                    native_messages.push(self.assistant_message(reply)?)
                }
                AssistantToolMessage::ToolResult {
                    call_id,
                    name,
                    content,
                    is_error,
                } => match self {
                    Self::OpenAiCompatible => native_messages
                        .push(json!({"role":"tool","tool_call_id":call_id,"content":content})),
                    Self::Ollama => native_messages
                        .push(json!({"role":"tool","tool_name":name,"content":content})),
                    Self::AnthropicCompatible => {
                        let block = json!({"type":"tool_result","tool_use_id":call_id,"content":content,"is_error":is_error});
                        // Anthropic requires all results from a parallel call batch in
                        // the immediately following user message, before any new text.
                        if let Some(last) = native_messages.last_mut()
                            && last["role"] == "user"
                            && let Some(blocks) = last["content"].as_array_mut()
                            && blocks.iter().all(|block| block["type"] == "tool_result")
                        {
                            blocks.push(block);
                        } else {
                            native_messages.push(json!({"role":"user","content":[block]}));
                        }
                    }
                },
            }
        }
        let mut body = json!({"model":model,"messages":native_messages,"stream":false});
        if !tools.is_empty() {
            body["tools"] = json!(definitions);
        }
        match self {
            Self::AnthropicCompatible => {
                body["system"] = json!(system);
                body["max_tokens"] = json!(ANTHROPIC_MAX_OUTPUT_TOKENS);
                if !tools.is_empty() {
                    body["tool_choice"] = json!({"type":"auto"});
                }
            }
            Self::Ollama => {
                body["truncate"] = json!(false);
                body["shift"] = json!(false);
                body["options"] = json!({"num_ctx":OLLAMA_INVESTIGATION_CONTEXT_TOKENS,"num_predict":OLLAMA_INVESTIGATION_OUTPUT_TOKENS});
            }
            Self::OpenAiCompatible => {
                if !tools.is_empty() {
                    body["tool_choice"] = json!("auto");
                }
            }
        }
        Ok(body)
    }

    fn assistant_message(self, reply: &AssistantToolReply) -> Result<Value, String> {
        if !reply.raw_message.is_null() {
            if reply.raw_message["role"] != "assistant" {
                return Err(String::from(
                    "assistant tool history has an invalid assistant role",
                ));
            }
            return Ok(reply.raw_message.clone());
        }
        // Trusted callers and test fixtures can construct a canonical assistant
        // message without raw provider blocks. Real decoded replies retain them.
        if self == Self::AnthropicCompatible {
            let mut blocks = Vec::new();
            if !reply.content.is_empty() {
                blocks.push(json!({"type":"text","text":reply.content}));
            }
            blocks.extend(reply.calls.iter().map(|call| json!({"type":"tool_use","id":call.id,"name":call.name,"input":call.arguments})));
            return Ok(json!({"role":"assistant","content":blocks}));
        }
        let calls: Vec<Value> = reply.calls.iter().enumerate().map(|(index, call)| {
            if self == Self::Ollama {
                json!({"type":"function","function":{"index":index,"name":call.name,"arguments":call.arguments}})
            } else {
                let arguments = call.arguments.as_str().map(str::to_owned).unwrap_or_else(|| call.arguments.to_string());
                json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":arguments}})
            }
        }).collect();
        let mut message = json!({"role":"assistant","content":reply.content});
        if !calls.is_empty() {
            message["tool_calls"] = json!(calls);
        }
        Ok(message)
    }

    pub(in crate::assistant) fn decode_tool_response(
        self,
        body: &[u8],
    ) -> Result<AssistantToolReply, String> {
        decode::decode(self, body)
    }
}

#[cfg(test)]
#[path = "assistant_tool_protocol_tests.rs"]
mod tests;
