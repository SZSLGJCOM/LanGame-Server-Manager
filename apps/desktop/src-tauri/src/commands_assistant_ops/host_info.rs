const ASSISTANT_HOST_CACHE_AGE: Duration = Duration::from_secs(2);
static ASSISTANT_HOST_READ_SLOTS: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
    OnceLock::new();

fn assistant_host_info_tool() -> crate::assistant::AssistantToolDefinition {
    assistant_native_tool(
        "read_host_info",
        "Read actual CPU profile, host memory and OS platform/process architecture of the computer running LanGame Server Manager. In LAN Web this is the manager host, not the browser device. Physical cores are compute cores within a processor, not a count of CPU chips or sockets. Logical processors are hardware execution contexts exposed to the OS, including SMT when present; they are not a limit on the number of programs, processes or tasks that can run. CPU profile counts can describe only the first processor or fall back to process-available parallelism; never present them as guaranteed total host cores. No selected game or server is required. Read-only; does not change settings, initialize storage or run a user command. Null or unavailable means not measured, not that hardware or software is absent. GPU and OS version are not collected. Use this before answering factual questions about this host, never infer hardware from UI configuration. Call at most once per conversation turn with no arguments.",
        json!({}),
        &[],
    )
}

fn validate_assistant_host_call(
    response: &crate::assistant::AssistantToolReply,
    already_read: bool,
) -> Result<&crate::assistant::AssistantToolCall, String> {
    let [call] = response.calls.as_slice() else {
        return Err(String::from(
            "Request read_host_info alone; a mixed tool batch is not executed.",
        ));
    };
    if call.name != "read_host_info"
        || !assistant_intent_id_is_valid(&call.id)
        || !call
            .arguments
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
    {
        return Err(String::from(
            "read_host_info requires a valid call ID and exactly an empty object, without paths, commands or target overrides.",
        ));
    }
    if already_read {
        return Err(String::from(
            "Host information was already requested in this turn. Use its result, including any reported failure, to answer without repeating the read.",
        ));
    }
    Ok(call)
}

async fn read_assistant_host_info(state: &DesktopState) -> Result<Value, String> {
    let cached = state
        .system_snapshot_cache
        .lock()
        .map_err(|_| String::from("Host snapshot cache is unavailable."))?
        .fresh(ASSISTANT_HOST_CACHE_AGE);
    if let Some(snapshot) = cached {
        return Ok(assistant_host_info_evidence(
            &snapshot,
            "host_metrics_cache",
            2_000,
        ));
    }
    let monitor = state.host_monitor.clone();
    let monitored_path = state
        .app_state
        .read()
        .map_err(|_| String::from("Host monitor settings are unavailable."))?
        .settings
        .servers_root
        .clone();
    run_assistant_read_worker_with(
        ASSISTANT_HOST_READ_SLOTS
            .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
            .clone(),
        ASSISTANT_EVIDENCE_READ_TIMEOUT,
        move || {
            let snapshot = monitor
                .lock()
                .map_err(|_| String::from("Host monitor is unavailable."))?
                .capture(&monitored_path);
            // Project the same metrics used by the UI. Do not serialize the
            // monitor's paths, volume labels, adapters or memory module IDs.
            let facts = SystemSnapshot {
                cpu_name: snapshot.cpu_name,
                cpu_physical_cores: snapshot.cpu_physical_cores,
                cpu_logical_cores: snapshot.cpu_logical_cores,
                cpu_max_frequency_mhz: snapshot.cpu_max_frequency_mhz,
                memory_total_bytes: snapshot.memory_total_bytes,
                memory_available_bytes: snapshot.memory_available_bytes,
                ..SystemSnapshot::default()
            };
            Ok(assistant_host_info_evidence(&facts, "host_monitor", 0))
        },
    )
    .await
}

fn assistant_host_info_evidence(snapshot: &SystemSnapshot, source: &str, max_age_ms: u32) -> Value {
    let cpu_name = truncate_assistant_prompt_text(
        &redact_assistant_provider_text(snapshot.cpu_name.trim()),
        256,
    );
    let memory_known = snapshot.memory_total_bytes > 0;
    json!({
        "scope":"manager_host", "source":source,
        "retrievedAtUnixMs":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as u64).unwrap_or(0),
        "maximumSnapshotAgeMs":max_age_ms,
        "cpu":{
            "name":(!cpu_name.is_empty()).then_some(cpu_name),
            "profileScope":"first_processor_or_process_fallback",
            "reportedPhysicalCores":(snapshot.cpu_physical_cores > 0).then_some(snapshot.cpu_physical_cores),
            "reportedLogicalCores":(snapshot.cpu_logical_cores > 0).then_some(snapshot.cpu_logical_cores),
            "maximumFrequencyMhz":(snapshot.cpu_max_frequency_mhz > 0).then_some(snapshot.cpu_max_frequency_mhz)
        },
        "memory":{
            "totalBytes":memory_known.then_some(snapshot.memory_total_bytes),
            "availableBytes":(memory_known && snapshot.memory_available_bytes <= snapshot.memory_total_bytes)
                .then_some(snapshot.memory_available_bytes),
            "totalDisplay":memory_known.then(|| assistant_memory_reading(snapshot.memory_total_bytes)),
            "availableDisplay":(memory_known && snapshot.memory_available_bytes <= snapshot.memory_total_bytes)
                .then(|| assistant_memory_reading(snapshot.memory_available_bytes)),
            "unitDefinition":"The display readings are calculated from bytes. Preserve the stated units: GiB = 1,073,741,824 bytes; GB = 1,000,000,000 bytes. Do not label a GiB value as GB."
        },
        "os":{"platform":std::env::consts::OS,"processArchitecture":std::env::consts::ARCH,"version":null},
        "unavailableFields":["gpu", "os.version", "cpu.totalHostCores", "installedSoftware", "gameServerConfiguration"],
        "fieldSemantics":{
            "cpu.reportedPhysicalCores":{
                "meaning":"Physical compute cores within the reported processor profile. Multiple cores can be inside one processor; this number does not count CPU chips, packages or sockets.",
                "isCpuChipOrSocketCount":false
            },
            "cpu.reportedLogicalCores":{
                "meaning":"Hardware execution contexts exposed to the operating system, including simultaneous multithreading when available. The scheduler shares them among many software threads; this is not a maximum number of programs, processes or tasks.",
                "isProgramOrTaskCountLimit":false
            },
            "unavailableFields":{
                "meaning":"The collector did not obtain these facts. Missing or null evidence cannot establish that hardware, software or a feature is absent.",
                "provesAbsence":false
            }
        },
        "limitations":"Null fields were not measured. CPU profile fields may describe only the first processor; logical core fallback is process-available parallelism, not whole-host core count. Multi-socket topology is not collected. Process architecture does not prove native hardware architecture. This snapshot alone does not establish game performance or compatibility."
    })
}

fn assistant_memory_reading(bytes: u64) -> String {
    format!(
        "{:.2} GiB ({:.2} GB)",
        bytes as f64 / 1_073_741_824.0,
        bytes as f64 / 1_000_000_000.0
    )
}

#[cfg(test)]
#[path = "host_info_tests.rs"]
mod host_info_tests;
