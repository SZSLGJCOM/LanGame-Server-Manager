#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantTaskRequirements {
    pub(super) settings: Vec<AssistantSettingRequirement>,
    pub(super) ports: Vec<AssistantPortRequirement>,
    pub(super) forbidden_actions: Vec<AssistantForbiddenActionRequirement>,
    pub(super) unverified: Vec<AssistantUnverifiedRequirement>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantSettingRequirement {
    pub(super) key: String,
    pub(super) expected: Value,
    pub(super) description: String,
    pub(super) source_text: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantPortRequirement {
    pub(super) name: String,
    pub(super) expected: u16,
    pub(super) description: String,
    pub(super) source_text: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantForbiddenActionRequirement {
    pub(super) action: AssistantOperationAction,
    pub(super) description: String,
    pub(super) source_text: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AssistantUnverifiedRequirement {
    pub(super) description: String,
    pub(super) source_text: String,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTaskRequirementView {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub source_text: String,
    pub target: Option<String>,
    pub expected_display: Option<String>,
}

impl std::fmt::Debug for AssistantTaskRequirements {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("AssistantTaskRequirements")
            .field(&self.views())
            .finish()
    }
}

impl AssistantTaskRequirements {
    pub(super) fn validated(
        &self,
        prompt: &str,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<(), String> {
        let count = self.settings.len()
            + self.ports.len()
            + self.forbidden_actions.len()
            + self.unverified.len();
        if count > 32
            || serde_json::to_vec(self)
                .map_err(|_| "Cannot encode request requirements")?
                .len()
                > 16 * 1024
        {
            return Err(String::from(
                "Request requirements exceed the 32-item or 16 KiB limit.",
            ));
        }
        if let (Some(instance), Some(module)) = (instance, module)
            && instance.summary.module_id != module.summary.id
        {
            return Err(String::from(
                "Requirement schema does not belong to the selected instance.",
            ));
        }
        let settings: Option<Value> = instance
            .map(|value| serde_json::from_str(&value.settings_json))
            .transpose()
            .map_err(|_| "Cannot read saved settings for requirements.")?;
        let schema: Option<Value> = module
            .and_then(|value| value.schema_json.as_deref())
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| "Cannot read the requirement schema.")?;
        let mut keys = HashSet::new();
        for (index, item) in self.settings.iter().enumerate() {
            assistant_requirement_text(&item.description, &item.source_text, prompt)?;
            if item.key.trim().is_empty() || !keys.insert(item.key.as_str()) {
                return Err(assistant_requirement_error(
                    index,
                    "setting key is empty or duplicated",
                ));
            }
            let actual = settings.as_ref().and_then(|value| value.get(&item.key));
            let property = schema
                .as_ref()
                .and_then(|value| value.get("properties"))
                .and_then(|value| value.get(&item.key));
            if actual.is_none()
                && property.is_none()
                && !(item.key == "bind_ip" && module.is_some())
            {
                return Err(assistant_requirement_error(
                    index,
                    "unknown setting key; read actual settings or the module schema",
                ));
            }
            if !assistant_requirement_value_valid(&item.expected, property, actual) {
                return Err(assistant_requirement_error(
                    index,
                    "expected value does not satisfy the declared type, enum or bounds",
                ));
            }
            if item.key == "bind_ip"
                && item
                    .expected
                    .as_str()
                    .and_then(|value| {
                        value
                            .parse::<std::net::IpAddr>()
                            .ok()
                            .filter(|address| address.to_string() == value)
                    })
                    .is_none()
            {
                return Err(assistant_requirement_error(
                    index,
                    "listener address must be a canonical IPv4 or IPv6 string",
                ));
            }
        }
        let available_ports = instance
            .map(|value| value.ports.as_slice())
            .or_else(|| module.map(|value| value.default_ports.as_slice()))
            .unwrap_or_default();
        let mut names = HashSet::new();
        for (index, item) in self.ports.iter().enumerate() {
            assistant_requirement_text(&item.description, &item.source_text, prompt)?;
            if item.expected == 0
                || item.name.trim() != item.name
                || item.name.is_empty()
                || !names.insert(item.name.to_ascii_lowercase())
                || available_ports
                    .iter()
                    .filter(|port| port.name.eq_ignore_ascii_case(&item.name))
                    .count()
                    != 1
            {
                return Err(assistant_requirement_error(
                    self.settings.len() + index,
                    "port must be declared, unique and within 1..65535",
                ));
            }
        }
        let mut actions = Vec::new();
        for item in &self.forbidden_actions {
            assistant_requirement_text(&item.description, &item.source_text, prompt)?;
            if item.action == AssistantOperationAction::None || actions.contains(&item.action) {
                return Err(String::from(
                    "Forbidden actions must be supported, mutating and unique.",
                ));
            }
            actions.push(item.action);
        }
        for item in &self.unverified {
            assistant_requirement_text(&item.description, &item.source_text, prompt)?;
            if item.reason.trim().is_empty() || item.reason.len() > 512 {
                return Err(String::from(
                    "An unverified requirement needs a reason within 512 bytes.",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_action(
        &self,
        plan: &AssistantOperationPlan,
        instance: Option<&InstanceDetails>,
        schema_json: Option<&str>,
    ) -> Result<(), String> {
        if plan.action == AssistantOperationAction::None {
            return Ok(());
        }
        if !self.unverified.is_empty() {
            return Err(String::from(
                "Unverified request requirements prevent mutations. Explain the verification gap before proceeding.",
            ));
        }
        if self.forbidden_actions.iter().any(|item| {
            item.action == plan.action
                || (plan.action == AssistantOperationAction::RestartServer
                    && matches!(
                        item.action,
                        AssistantOperationAction::StartServer
                            | AssistantOperationAction::StopServer
                    ))
                || (item.action == AssistantOperationAction::CreateBackup
                    && (plan.action == AssistantOperationAction::RestoreBackup
                        || (matches!(
                            plan.action,
                            AssistantOperationAction::StopServer
                                | AssistantOperationAction::RestartServer
                        ) && instance.is_some_and(|instance| instance.auto_backup_on_stop))))
        }) {
            return Err(String::from(
                "This operation is forbidden by the confirmed request requirements.",
            ));
        }
        if let Some(patch) = &plan.settings_patch {
            for (index, item) in self.settings.iter().enumerate() {
                if patch
                    .get(&item.key)
                    .is_some_and(|value| value != &item.expected)
                {
                    return Err(assistant_requirement_error(
                        index,
                        "configuration patch contradicts the confirmed expected value",
                    ));
                }
            }
        }
        if let Some(patch) = &plan.port_patch {
            for (name, port) in assistant_port_patch_entries(patch)? {
                if self.ports.iter().any(|item| {
                    item.name.eq_ignore_ascii_case(&name) && port != Some(item.expected)
                }) {
                    return Err(String::from(
                        "Port patch contradicts a confirmed request requirement.",
                    ));
                }
            }
        }
        if matches!(
            plan.action,
            AssistantOperationAction::StartServer | AssistantOperationAction::RestartServer
        ) {
            let missing = self.views().into_iter().zip(self.checks(instance, schema_json))
                .filter(|(_, check)| check.status != AssistantTaskCheckStatus::Satisfied)
                .take(4).map(|(view, check)| json!({
                    "id":view.id,"target":view.target,"status":check.status,
                    "expectedDisplay":view.expected_display.map(|value| summarize_text(&value,96))
                })).collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(format!(
                    "Start requires every confirmed requirement to be known and satisfied. Unmet requirements: {}",
                    Value::Array(missing)
                ));
            }
        }
        Ok(())
    }

    pub(super) fn views(&self) -> Vec<AssistantTaskRequirementView> {
        let mut views = Vec::new();
        let mut add = |kind: &str,
                       description: &str,
                       source: &str,
                       target: Option<String>,
                       expected: Option<String>| {
            views.push(AssistantTaskRequirementView {
                id: format!("requirement_{}", views.len() + 1),
                kind: kind.into(),
                description: self.redact_text(description),
                source_text: self.redact_text(source),
                target: target.map(|value| self.redact_text(&value)),
                expected_display: expected.map(|value| self.redact_text(&value)),
            });
        };
        for item in &self.settings {
            add(
                "setting",
                &item.description,
                &item.source_text,
                Some(item.key.clone()),
                Some(assistant_requirement_display_value(&item.key, &item.expected).to_string()),
            );
        }
        for item in &self.ports {
            add(
                "port",
                &item.description,
                &item.source_text,
                Some(item.name.clone()),
                Some(item.expected.to_string()),
            );
        }
        for item in &self.forbidden_actions {
            let action = serde_json::to_value(item.action)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned));
            add(
                "forbidden_action",
                &item.description,
                &item.source_text,
                action,
                None,
            );
        }
        for item in &self.unverified {
            add(
                "unverified",
                &item.description,
                &item.source_text,
                None,
                Some(item.reason.clone()),
            );
        }
        views
    }

    pub(super) fn checks(
        &self,
        instance: Option<&InstanceDetails>,
        schema_json: Option<&str>,
    ) -> Vec<AssistantTaskCheck> {
        use AssistantTaskCheckStatus::{Failed, Satisfied, Unknown};
        let settings =
            instance.and_then(|value| serde_json::from_str::<Value>(&value.settings_json).ok());
        let mut statuses = Vec::new();
        for item in &self.settings {
            let actual = settings.as_ref().and_then(|value| value.get(&item.key));
            let status = match actual {
                None => Unknown,
                Some(actual) if actual != &item.expected => Failed,
                Some(_) if item.key == "bind_ip" => match (instance, item.expected.as_str()) {
                    (Some(instance), Some(expected)) if instance.summary.bind_ip == expected => {
                        Satisfied
                    }
                    _ => Failed,
                },
                Some(_) => Satisfied,
            };
            // The adapter may narrow canonical equality to unknown when custom
            // configuration cannot be statically projected into guided fields.
            let status = assistant_requirement_saved_status(
                instance,
                settings.as_ref(),
                schema_json,
                &item.key,
                status,
            );
            statuses.push((
                status,
                "Checks the saved canonical setting; runtime effects need separate evidence.",
            ));
        }
        for item in &self.ports {
            let matching = instance
                .map(|value| {
                    value
                        .ports
                        .iter()
                        .filter(|port| port.name.eq_ignore_ascii_case(&item.name))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let status = match matching.as_slice() {
                [port] if port.port == item.expected => Satisfied,
                [_] => Failed,
                _ => Unknown,
            };
            statuses.push((
                status,
                "Checks the saved declared port; it does not prove network reachability.",
            ));
        }
        statuses.extend(self.forbidden_actions.iter().map(|_| (Satisfied, "The dispatcher forbids this explicit action; indirect game or network effects are not covered.")));
        statuses.extend(self.unverified.iter().map(|_| {
            (
                Unknown,
                "This request requirement cannot be verified by the available checks.",
            )
        }));
        self.views()
            .into_iter()
            .zip(statuses)
            .map(|(view, (status, summary))| {
                assistant_task_check(
                    &view.id,
                    status,
                    summary,
                    json!({"requirementId": view.id, "requirement": view}),
                )
            })
            .collect()
    }

    fn redact_text(&self, text: &str) -> String {
        let mut redacted = text.to_owned();
        for item in &self.settings {
            let safe = assistant_requirement_display_value(&item.key, &item.expected);
            assistant_requirement_scrub_values(&item.expected, &safe, &mut redacted);
        }
        redact_assistant_provider_text(&redacted)
    }
}

fn assistant_requirement_error(index: usize, reason: &str) -> String {
    format!("Requirement {}: {reason}.", index + 1)
}

fn assistant_requirement_saved_status(
    instance: Option<&InstanceDetails>,
    settings: Option<&Value>,
    schema_json: Option<&str>,
    key: &str,
    status: AssistantTaskCheckStatus,
) -> AssistantTaskCheckStatus {
    if instance.is_some_and(|instance| instance.summary.module_id == "dontstarve") {
        let known = settings.is_some_and(|settings| {
            app_storage::dst_world_setting_evidence_known(
                settings,
                schema_json.unwrap_or("{}"),
                key,
            )
            .unwrap_or(false)
        });
        if !known {
            return AssistantTaskCheckStatus::Unknown;
        }
    }
    status
}

fn assistant_requirement_text(description: &str, source: &str, prompt: &str) -> Result<(), String> {
    if description.trim().is_empty()
        || description.len() > 512
        || source.trim().is_empty()
        || source.len() > 512
        || !prompt.contains(source)
    {
        return Err(String::from(
            "Each requirement needs a description and an exact nonempty request excerpt, each within 512 bytes.",
        ));
    }
    Ok(())
}

fn assistant_requirement_display_value(key: &str, value: &Value) -> Value {
    let wrapped = json!({key: value});
    serde_json::from_str::<Value>(&redact_assistant_provider_text(&wrapped.to_string()))
        .ok()
        .and_then(|value| value.get(key).cloned())
        .unwrap_or_else(|| json!("[REDACTED]"))
}

fn assistant_requirement_scrub_values(original: &Value, safe: &Value, text: &mut String) {
    if safe == "[REDACTED]" && original != safe {
        match original {
            Value::String(value) if !value.is_empty() => *text = text.replace(value, "[REDACTED]"),
            Value::Array(values) => {
                for value in values {
                    assistant_requirement_scrub_values(value, safe, text);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    assistant_requirement_scrub_values(value, safe, text);
                }
            }
            Value::Number(_) | Value::Bool(_) => {
                *text = text.replace(&original.to_string(), "[REDACTED]")
            }
            _ => {}
        }
    } else if original.is_string() && safe.is_string() && original != safe {
        // A string may contain several partially masked credentials. The
        // redactor does not expose their spans, so do not echo other text that
        // could quote a recognized credential without its identifying label.
        *text = String::from("[REDACTED]");
    } else if let (Some(original), Some(safe)) = (original.as_object(), safe.as_object()) {
        for (key, value) in original {
            if let Some(safe) = safe.get(key) {
                assistant_requirement_scrub_values(value, safe, text);
            }
        }
    } else if let (Some(original), Some(safe)) = (original.as_array(), safe.as_array()) {
        for (value, safe) in original.iter().zip(safe) {
            assistant_requirement_scrub_values(value, safe, text);
        }
    }
}

include!("requirements_validation.rs");

#[cfg(test)]
#[path = "requirements_tests.rs"]
mod requirements_tests;
