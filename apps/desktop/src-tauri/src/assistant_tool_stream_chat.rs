use super::*;
use std::collections::BTreeMap;

pub(super) struct ChatStream {
    protocol: ProviderProtocol,
    message: Value,
    calls: BTreeMap<usize, Value>,
    finish: Option<String>,
    done: bool,
    pub(super) retained_bytes: usize,
}

impl ChatStream {
    pub(super) fn new(protocol: ProviderProtocol) -> Self {
        Self {
            protocol,
            message: json!({"role":"assistant","content":""}),
            calls: BTreeMap::new(),
            finish: None,
            done: false,
            retained_bytes: 0,
        }
    }

    pub(super) fn event(
        &mut self,
        data: &str,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<(), String> {
        if self.done {
            return Err(invalid());
        }
        if data == "[DONE]" {
            if self.protocol != ProviderProtocol::OpenAiCompatible || self.finish.is_none() {
                return Err(invalid());
            }
            self.done = true;
            return Ok(());
        }
        let payload: Value = serde_json::from_str(data).map_err(|_| invalid())?;
        if payload.get("error").is_some_and(|value| !value.is_null()) {
            return Err("assistant provider reported a stream error".into());
        }
        if self.protocol == ProviderProtocol::Ollama {
            return self.ollama(payload, observer);
        }
        let choices = payload["choices"].as_array().ok_or_else(invalid)?;
        // An optional final usage frame contains no choice or text.
        if choices.is_empty() && self.finish.is_some() && payload["usage"].is_object() {
            return Ok(());
        }
        if choices.len() != 1 || choices[0]["index"] != 0 || self.finish.is_some() {
            return Err(invalid());
        }
        let choice = &choices[0];
        let delta = &choice["delta"];
        if !delta.is_object() || delta.get("role").is_some_and(|role| role != "assistant") {
            return Err(invalid());
        }
        self.text(delta, observer)?;
        let had_refusal = self.message["refusal"]
            .as_str()
            .is_some_and(|text| !text.is_empty());
        append_string(
            &mut self.message,
            "refusal",
            &delta["refusal"],
            &mut self.retained_bytes,
        )?;
        if let Some(text) = delta["refusal"].as_str().filter(|text| !text.is_empty()) {
            if !had_refusal
                && self.message["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            {
                observer("\n")?;
            }
            observer(text)?;
        }
        for field in ["reasoning_content", "reasoning"] {
            append_string(
                &mut self.message,
                field,
                &delta[field],
                &mut self.retained_bytes,
            )?;
        }
        if let Some(calls) = delta.get("tool_calls").filter(|value| !value.is_null()) {
            for raw in calls.as_array().ok_or_else(invalid)? {
                let index = raw["index"]
                    .as_u64()
                    .filter(|index| *index < 32)
                    .ok_or_else(invalid)? as usize;
                if raw.get("type").is_some_and(|kind| kind != "function") {
                    return Err(invalid());
                }
                let call = self.calls.entry(index).or_insert_with(
                    || json!({"type":"function","id":"","function":{"name":"","arguments":""}}),
                );
                append_string(call, "id", &raw["id"], &mut self.retained_bytes)?;
                append_string(
                    &mut call["function"],
                    "name",
                    &raw["function"]["name"],
                    &mut self.retained_bytes,
                )?;
                append_string(
                    &mut call["function"],
                    "arguments",
                    &raw["function"]["arguments"],
                    &mut self.retained_bytes,
                )?;
            }
        }
        if !choice["finish_reason"].is_null() {
            let finish = choice["finish_reason"].as_str().ok_or_else(invalid)?;
            retain_bytes(&mut self.retained_bytes, finish.len())?;
            self.finish = Some(finish.into());
        }
        Ok(())
    }

    fn text(&mut self, delta: &Value, observer: &AssistantTextObserver<'_>) -> Result<(), String> {
        append_string(
            &mut self.message,
            "content",
            &delta["content"],
            &mut self.retained_bytes,
        )?;
        if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            observer(text)?;
        }
        Ok(())
    }

    fn ollama(
        &mut self,
        payload: Value,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<(), String> {
        let message = &payload["message"];
        if !message.is_object() || message.get("role").is_some_and(|role| role != "assistant") {
            return Err(invalid());
        }
        self.text(message, observer)?;
        append_string(
            &mut self.message,
            "thinking",
            &message["thinking"],
            &mut self.retained_bytes,
        )?;
        if let Some(calls) = message.get("tool_calls").filter(|value| !value.is_null()) {
            for call in calls.as_array().ok_or_else(invalid)? {
                if !call["function"]["arguments"].is_object() {
                    return Err(invalid());
                }
                // Native Ollama emits complete argument objects, never OpenAI
                // argument-string deltas. Calls retain their arrival order.
                let index = self.calls.len();
                if index >= 32 {
                    return Err(invalid());
                }
                retain_value(&mut self.retained_bytes, call)?;
                self.calls.insert(index, call.clone());
            }
        }
        match payload["done"].as_bool() {
            Some(true) => {
                let finish = payload["done_reason"].as_str().ok_or_else(invalid)?;
                retain_bytes(&mut self.retained_bytes, finish.len())?;
                self.finish = Some(finish.into());
                self.done = true;
            }
            Some(false) => {}
            None => return Err(invalid()),
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<Value, String> {
        if !self.done || self.finish.is_none() {
            return Err(invalid());
        }
        if !self.calls.is_empty() {
            if self.calls.keys().copied().ne(0..self.calls.len()) {
                return Err(invalid());
            }
            self.message["tool_calls"] = Value::Array(self.calls.into_values().collect());
        }
        Ok(if self.protocol == ProviderProtocol::Ollama {
            json!({"message":self.message,"done":true,"done_reason":self.finish})
        } else {
            json!({"choices":[{"message":self.message,"finish_reason":self.finish}]})
        })
    }
}
