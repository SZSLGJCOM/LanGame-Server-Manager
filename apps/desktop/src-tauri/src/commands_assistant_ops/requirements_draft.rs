#[derive(Debug)]
pub(super) struct AssistantRequirementsDraft {
    original_request: String,
    sources: Vec<AssistantRequirementSource>,
    initialization_error: Option<String>,
    draft: AssistantTaskRequirements,
    recorded: bool,
    last_record_had_errors: bool,
    ready: bool,
    deferred_until_lifecycle: bool,
}

impl AssistantRequirementsDraft {
    pub(super) fn new(original_request: &str) -> Self {
        let (sources, initialization_error) = match assistant_requirement_sources(original_request)
        {
            Ok(sources)
                if assistant_draft_source_catalog(&sources).to_string().len()
                    <= ASSISTANT_DRAFT_BYTES =>
            {
                (sources, None)
            }
            Ok(_) => (
                Vec::new(),
                Some(String::from(
                    "The complete request source catalog exceeds 16 KiB; it was not truncated.",
                )),
            ),
            Err(error) => (Vec::new(), Some(error)),
        };
        Self {
            original_request: if initialization_error.is_none() {
                original_request.into()
            } else {
                String::new()
            },
            sources,
            initialization_error,
            draft: assistant_empty_requirements(),
            recorded: false,
            last_record_had_errors: false,
            ready: false,
            deferred_until_lifecycle: false,
        }
    }

    pub(super) fn deferred_for_lifecycle(original_request: &str) -> Self {
        let mut draft = Self::new(original_request);
        draft.deferred_until_lifecycle = true;
        draft
    }

    fn is_active(&self) -> bool {
        !self.deferred_until_lifecycle
    }

    fn activate_for_lifecycle(&mut self, action: AssistantOperationAction) -> bool {
        if self.deferred_until_lifecycle && assistant_is_lifecycle_operation(action) {
            self.deferred_until_lifecycle = false;
            true
        } else {
            false
        }
    }

    pub(super) fn source_catalog(&self) -> Value {
        match &self.initialization_error {
            Some(error) => json!({"error":error,"sources":[],"complete":false}),
            None => assistant_draft_source_catalog(&self.sources),
        }
    }

    pub(super) fn tool_definitions(
        &self,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<Vec<crate::assistant::AssistantToolDefinition>, String> {
        self.ensure_available()?;
        assistant_draft_tool_definitions(&self.sources, instance, module)
    }

    pub(super) fn handle_tool(
        &mut self,
        name: &str,
        arguments: &Value,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<Value, String> {
        self.ensure_available()?;
        if self.ready {
            return Err(String::from(
                "The requirements draft is finished and immutable.",
            ));
        }
        match name {
            "record_task_requirements" => {
                self.last_record_had_errors = true;
                self.record(arguments, instance, module)
            }
            "finish_task_requirements" => {
                if !arguments
                    .as_object()
                    .is_some_and(|object| object.is_empty())
                {
                    return Err(String::from(
                        "finish_task_requirements takes an empty object.",
                    ));
                }
                if self.sources.iter().any(|source| !source.readable) {
                    return Err(String::from(
                        "Request fragments were hidden by redaction. Clarify the request or enter sensitive values in settings; the draft cannot silently omit them or authorize mutations.",
                    ));
                }
                if !self.recorded || self.last_record_had_errors {
                    return Err(String::from(
                        "Record an explicit requirements list and correct the last record's errors before finishing. An empty list must also be recorded explicitly.",
                    ));
                }
                self.draft
                    .validated(&self.original_request, instance, module)?;
                self.ready = true;
                Ok(
                    json!({"ready":true,"itemCount":assistant_draft_count(&self.draft),"note":"Requirements are ready for the operation preview. No operation was authorized or executed; the operator must review their completeness."}),
                )
            }
            _ => Err(String::from("Unsupported requirements-draft tool.")),
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    pub(super) fn requirements(&self) -> Option<&AssistantTaskRequirements> {
        self.ready.then_some(&self.draft)
    }

    fn ensure_available(&self) -> Result<(), String> {
        if let Some(error) = &self.initialization_error {
            return Err(error.clone());
        }
        if self.source_catalog().to_string().len() > ASSISTANT_DRAFT_BYTES {
            return Err(String::from(
                "The complete request source catalog exceeds 16 KiB; it was not truncated.",
            ));
        }
        Ok(())
    }

    fn record(
        &mut self,
        arguments: &Value,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<Value, String> {
        let object = arguments
            .as_object()
            .ok_or("Requirements arguments must be an object.")?;
        let groups = ["settings", "ports", "forbiddenActions", "unverified"];
        if arguments.to_string().len() > ASSISTANT_DRAFT_BYTES
            || object
                .keys()
                .any(|key| !groups.contains(&key.as_str()) && key != "reset")
        {
            return Err(String::from(
                "Requirements arguments contain unknown fields or exceed 16 KiB.",
            ));
        }
        let mut count = 0;
        for group in groups {
            count += object
                .get(group)
                .and_then(Value::as_array)
                .ok_or("Record all four requirements arrays explicitly.")?
                .len();
        }
        if count > 32 {
            return Err(String::from(
                "A requirements record accepts at most 32 items.",
            ));
        }
        let reset = match object.get("reset") {
            None => false,
            Some(Value::Bool(value)) => *value,
            _ => return Err(String::from("reset must be a boolean.")),
        };
        if reset {
            self.draft = assistant_empty_requirements();
        }
        let mut accepted = Vec::new();
        let mut errors = Vec::new();
        for group in groups {
            for (index, item) in object[group].as_array().into_iter().flatten().enumerate() {
                let path = format!("{group}[{index}]");
                match self.record_item(group, item, instance, module) {
                    Ok(()) => accepted.push(path),
                    Err((field, message)) => {
                        errors.push(json!({"field":format!("{path}.{field}"),"message":message}))
                    }
                }
            }
        }
        self.recorded = true;
        self.last_record_had_errors = !errors.is_empty();
        Ok(
            json!({"accepted":accepted,"errors":errors,"itemCount":assistant_draft_count(&self.draft),"ready":false,
            "note":"Valid items were retained by kind and target. Correct rejected entries before finishing; reset=true explicitly discards the unconfirmed draft. No operation was executed."}),
        )
    }

    fn record_item(
        &mut self,
        group: &str,
        item: &Value,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<(), (&'static str, String)> {
        let malformed = || {
            (
                "item",
                String::from("Use only the fields and JSON types declared by this tool."),
            )
        };
        let object = item.as_object().ok_or_else(malformed)?;
        let allowed = match group {
            "settings" => &["sourceId", "key", "expected"][..],
            "ports" => &["sourceId", "name", "expected"][..],
            "forbiddenActions" => &["sourceId", "action"][..],
            _ => &["sourceId", "reason"][..],
        };
        if object.len() != allowed.len()
            || object.keys().any(|key| !allowed.contains(&key.as_str()))
        {
            return Err(malformed());
        }
        let source = object
            .get("sourceId")
            .and_then(Value::as_str)
            .and_then(|id| self.sources.iter().find(|source| source.id == id))
            .ok_or_else(|| {
                (
                    "sourceId",
                    String::from("Choose an exact source ID from the request catalog."),
                )
            })?;
        if !source.readable {
            return Err((
                "sourceId",
                String::from(
                    "This fragment is redacted and cannot be automatically interpreted. Clarify it or enter sensitive values in settings.",
                ),
            ));
        }
        let text = source.text.clone();
        let mut candidate = self.draft.clone();
        let field = match group {
            "settings" => {
                let key = object["key"].as_str().ok_or_else(malformed)?.to_owned();
                let expected = object["expected"].clone();
                let requirement = AssistantSettingRequirement {
                    key: key.clone(),
                    expected,
                    description: text.clone(),
                    source_text: text,
                };
                if let Some(existing) = candidate.settings.iter_mut().find(|item| item.key == key) {
                    *existing = requirement;
                } else {
                    candidate.settings.push(requirement);
                }
                "key/expected"
            }
            "ports" => {
                let name = object["name"].as_str().ok_or_else(malformed)?.to_owned();
                let expected = object["expected"]
                    .as_u64()
                    .and_then(|value| u16::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| {
                        ("expected", String::from("Use an integer port in 1..65535."))
                    })?;
                let requirement = AssistantPortRequirement {
                    name: name.clone(),
                    expected,
                    description: text.clone(),
                    source_text: text,
                };
                if let Some(existing) = candidate
                    .ports
                    .iter_mut()
                    .find(|item| item.name.eq_ignore_ascii_case(&name))
                {
                    *existing = requirement;
                } else {
                    candidate.ports.push(requirement);
                }
                "name/expected"
            }
            "forbiddenActions" => {
                let action =
                    serde_json::from_value::<AssistantOperationAction>(object["action"].clone())
                        .map_err(|_| malformed())?;
                let requirement = AssistantForbiddenActionRequirement {
                    action,
                    description: text.clone(),
                    source_text: text,
                };
                if let Some(existing) = candidate
                    .forbidden_actions
                    .iter_mut()
                    .find(|item| item.action == action)
                {
                    *existing = requirement;
                } else {
                    candidate.forbidden_actions.push(requirement);
                }
                "action"
            }
            _ => {
                let reason = object["reason"].as_str().ok_or_else(malformed)?.to_owned();
                let requirement = AssistantUnverifiedRequirement {
                    description: text.clone(),
                    source_text: text.clone(),
                    reason,
                };
                if let Some(existing) = candidate
                    .unverified
                    .iter_mut()
                    .find(|item| item.source_text == text)
                {
                    *existing = requirement;
                } else {
                    candidate.unverified.push(requirement);
                }
                "reason"
            }
        };
        candidate
            .validated(&self.original_request, instance, module)
            .map_err(|error| (field, error))?;
        self.draft = candidate;
        Ok(())
    }
}

fn assistant_empty_requirements() -> AssistantTaskRequirements {
    AssistantTaskRequirements {
        settings: Vec::new(),
        ports: Vec::new(),
        forbidden_actions: Vec::new(),
        unverified: Vec::new(),
    }
}

fn assistant_draft_count(requirements: &AssistantTaskRequirements) -> usize {
    requirements.settings.len()
        + requirements.ports.len()
        + requirements.forbidden_actions.len()
        + requirements.unverified.len()
}

include!("requirements_draft_sources.rs");
include!("requirements_draft_tools.rs");

#[cfg(test)]
#[path = "requirements_draft_tests.rs"]
mod requirements_draft_tests;
