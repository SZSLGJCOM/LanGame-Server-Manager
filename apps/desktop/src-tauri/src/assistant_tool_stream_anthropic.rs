use super::*;

#[derive(Default)]
pub(super) struct AnthropicStream {
    started: bool,
    done: bool,
    stop: Option<String>,
    blocks: Vec<Value>,
    open: Option<usize>,
    input_json: String,
    had_text: bool,
    pub(super) retained_bytes: usize,
}

impl AnthropicStream {
    pub(super) fn event(
        &mut self,
        data: &str,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<(), String> {
        if self.done {
            return Err(invalid());
        }
        let event: Value = serde_json::from_str(data).map_err(|_| invalid())?;
        match event["type"].as_str().ok_or_else(invalid)? {
            "ping" => {}
            "error" => return Err("assistant provider reported a stream error".into()),
            "message_start" => {
                if self.started
                    || event["message"]["role"] != "assistant"
                    || event["message"]["content"]
                        .as_array()
                        .is_none_or(|blocks| !blocks.is_empty())
                {
                    return Err(invalid());
                }
                self.started = true;
            }
            "content_block_start" => self.start_block(&event, observer)?,
            "content_block_delta" => self.delta(&event, observer)?,
            "content_block_stop" => {
                let index = self.index(&event)?;
                if self.blocks[index]["type"] == "tool_use" && !self.input_json.is_empty() {
                    let arguments: Value =
                        serde_json::from_str(&self.input_json).map_err(|_| invalid())?;
                    if !arguments.is_object() {
                        return Err(invalid());
                    }
                    self.blocks[index]["input"] = arguments;
                }
                self.input_json.clear();
                self.open = None;
            }
            "message_delta" => {
                if !self.started || self.open.is_some() {
                    return Err(invalid());
                }
                if let Some(stop) = event["delta"]["stop_reason"].as_str() {
                    retain_bytes(&mut self.retained_bytes, stop.len())?;
                    self.stop = Some(stop.into());
                }
            }
            "message_stop" => {
                if !self.started || self.open.is_some() || self.stop.is_none() {
                    return Err(invalid());
                }
                self.done = true;
            }
            // New metadata events carry no public text and cannot close a block.
            _ => {}
        }
        Ok(())
    }

    fn start_block(
        &mut self,
        event: &Value,
        observer: &AssistantTextObserver<'_>,
    ) -> Result<(), String> {
        if !self.started
            || self.open.is_some()
            || self.stop.is_some()
            || self.blocks.len() >= 128
            || event["index"].as_u64() != Some(self.blocks.len() as u64)
        {
            return Err(invalid());
        }
        let block = event["content_block"].clone();
        retain_value(&mut self.retained_bytes, &block)?;
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().ok_or_else(invalid)?;
                if self.had_text {
                    observer("\n")?;
                }
                self.had_text = true;
                if !text.is_empty() {
                    observer(text)?;
                }
            }
            Some("tool_use") if block["input"].is_object() => {}
            Some("thinking") if block["thinking"].is_string() => {}
            Some("redacted_thinking") if block["data"].is_string() => {}
            _ => return Err(invalid()),
        }
        self.open = Some(self.blocks.len());
        self.blocks.push(block);
        Ok(())
    }

    fn index(&self, event: &Value) -> Result<usize, String> {
        let index = event["index"].as_u64().ok_or_else(invalid)?;
        self.open
            .filter(|open| *open as u64 == index)
            .ok_or_else(invalid)
    }

    fn delta(&mut self, event: &Value, observer: &AssistantTextObserver<'_>) -> Result<(), String> {
        let index = self.index(event)?;
        let block = &mut self.blocks[index];
        let delta = &event["delta"];
        match (block["type"].as_str(), delta["type"].as_str()) {
            (Some("text"), Some("text_delta")) => {
                append_string(block, "text", &delta["text"], &mut self.retained_bytes)?;
                observer(delta["text"].as_str().ok_or_else(invalid)?)?;
            }
            (Some("tool_use"), Some("input_json_delta")) => {
                let partial = delta["partial_json"].as_str().ok_or_else(invalid)?;
                retain_bytes(&mut self.retained_bytes, partial.len())?;
                self.input_json.push_str(partial);
            }
            (Some("thinking"), Some("thinking_delta")) => append_string(
                block,
                "thinking",
                &delta["thinking"],
                &mut self.retained_bytes,
            )?,
            (Some("thinking"), Some("signature_delta")) => append_string(
                block,
                "signature",
                &delta["signature"],
                &mut self.retained_bytes,
            )?,
            _ => return Err(invalid()),
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Value, String> {
        if !self.done {
            return Err(invalid());
        }
        Ok(json!({"role":"assistant","content":self.blocks,"stop_reason":self.stop}))
    }
}
