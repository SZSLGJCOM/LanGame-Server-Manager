const ASSISTANT_REQUIREMENTS_GUIDE: &str = r#"
Before proposing the first service or lifecycle operation, prepare the complete request using record_task_requirements and finish_task_requirements. Copy sourceId from the application's original-request reference catalog; the application retains the exact original excerpt. Read actual schema declarations before setting values. Record every explicitly requested setting even when its current/default value already matches. Use forbiddenActions only for excluded explicit application actions. Hidden network or gameplay effects that available evidence cannot establish belong in unverified.
Distinguish a supported setting awaiting the application's runtime verification from a requirement with no supported verification path. For a declared strict bind-address capability, record the requested bind_ip setting; actual sockets are verified during confirmed startup, and the capability's port_names define that scope. Do not promise other sockets or remote reachability. A loopback request does not imply that other LAN devices must be able to join. Do not add requirements the user did not ask for. Checks the user explicitly excludes are not unverified obligations; do not turn scope exclusions or optional limitation notes into mutation blockers.
The supplied file tools cannot edit shared game installations or binaries. For a request to preserve game programs, exclude any requested-against install, validate or mod-install actions and use only supported instance configuration. This does not promise that operational tracking metadata or generated instance configuration remains byte-identical. If the user explicitly prohibits those necessary effects or asks for a broader guarantee without an available check, retain that gap in unverified.
When the request contains selected previous user requests and a current user reply, explicit current corrections supersede conflicting earlier values. Keep the earlier constraints that were not changed. Cite the corresponding exact source references; never rewrite source text or promote assistant suggestions into user requirements.
Drafting changes only in-memory task requirements, never a server. Valid entries survive a later invalid entry so you can correct the indicated fields. finish_task_requirements does not execute or prove complete understanding: the operator still reviews the extracted list in the first operation confirmation. Unsupported requirements prevent mutations; explain the gap with report_limitation.
Once the draft is ready, propose_operation takes only the next operation's fields. The application attaches the complete requirements; do not recreate them inside the action. Subsequent confirmed operations inherit the immutable requirements. Partial configuration may omit unfinished keys but cannot contradict their expected values. Startup requires every confirmed requirement to be known and satisfied.
After finish_task_requirements, the draft tools are unavailable. Correct only the rejected operation fields, or report_limitation for a frozen unsupported requirement; never try to reset the finished draft.
"#;

impl AssistantTaskContract {
    fn bind_requirements(
        &self,
        plan: &AssistantOperationPlan,
        instance: Option<&InstanceDetails>,
        module: Option<&ModuleDetails>,
    ) -> Result<Self, String> {
        if let Some(bound) = &self.requirements {
            if plan
                .task_requirements
                .as_ref()
                .is_some_and(|proposed| proposed != bound)
            {
                return Err(String::from(
                    "The confirmed request requirements are immutable. Keep the original requirements and correct the operation; changing the request requires a new task preview.",
                ));
            }
            return Ok(self.clone());
        }
        let Some(proposed) = &plan.task_requirements else {
            if (self.requires_requirements() || assistant_is_lifecycle_operation(plan.action))
                && plan.action != AssistantOperationAction::None
            {
                return Err(String::from(
                    "The first service or lifecycle plan must include complete taskRequirements (settings, ports, forbiddenActions, unverified). Read current settings or the selected module schema, then present every request requirement for confirmation before any operation.",
                ));
            }
            return Ok(self.clone());
        };
        proposed.validated(&self.original_request, instance, module)?;
        let mut task = self.clone();
        task.requirements = Some(proposed.clone());
        task.requirements_schema = module
            .and_then(|module| module.schema_json.as_deref())
            .map(std::sync::Arc::from);
        Ok(task)
    }

    fn requirements_summary(&self) -> String {
        let Some(requirements) = &self.requirements else {
            return String::new();
        };
        let views = requirements.views();
        let mut summary = String::from(
            "\nRequest requirements — review the extracted list before confirming; follow-up steps cannot change it:",
        );
        if views.is_empty() {
            summary.push_str("\nNo additional requirements were declared. The service and preservation checks still apply.");
        }
        for item in views {
            summary.push_str(&format!("\n{}: {}", item.id, item.description));
            if let Some(target) = item.target {
                summary.push_str(&format!("; target={target}"));
            }
            if let Some(expected) = item.expected_display {
                summary.push_str(&format!("; expected={expected}"));
            }
            summary.push_str(&format!("; request excerpt={}", item.source_text));
        }
        summary
    }

    fn append_requirements_guide(&self, prompt: &mut String) -> Result<(), String> {
        if self.requires_requirements() && self.requirements.is_none() {
            prompt.push_str(ASSISTANT_REQUIREMENTS_GUIDE);
        } else if self.requirements.is_some() {
            prompt.push_str("\nThe request requirements above are fixed. The application inherits them automatically into each operation proposal. Do not alter, remove or replace a requirement. Read the actual saved values and satisfy every requirement. Preparation-only tasks must finish without starting a server.\n");
        }
        if prompt.len() > ASSISTANT_INVESTIGATION_EVIDENCE_BYTES {
            return Err(String::from(
                "The complete request and its requirements exceed the investigation budget; no requirements were truncated and no operation was executed.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "requirements_planning_tests.rs"]
mod requirements_planning_tests;
