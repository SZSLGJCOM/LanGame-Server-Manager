use super::*;

pub fn resolve_runtime_performance_policy(
    module_policy: &RuntimePerformancePolicy,
    settings: &Value,
) -> RuntimePerformancePolicy {
    resolve_runtime_performance_policy_for_instance(module_policy, settings, "")
}

pub fn resolve_runtime_performance_policy_for_instance(
    module_policy: &RuntimePerformancePolicy,
    settings: &Value,
    instance_id: &str,
) -> RuntimePerformancePolicy {
    resolve_runtime_performance_policy_with_preview_for_instance(
        module_policy,
        settings,
        instance_id,
    )
    .0
}

pub fn resolve_runtime_performance_policy_with_preview_for_instance(
    module_policy: &RuntimePerformancePolicy,
    settings: &Value,
    instance_id: &str,
) -> (RuntimePerformancePolicy, RuntimePerformancePolicyPreview) {
    let resolution =
        resolve_runtime_performance_resolution_for_instance(module_policy, settings, instance_id);
    (resolution.policy, resolution.preview)
}

pub(super) fn resolve_runtime_performance_resolution_for_instance(
    module_policy: &RuntimePerformancePolicy,
    settings: &Value,
    instance_id: &str,
) -> RuntimePerformanceResolution {
    let priority_override = resolve_runtime_priority_override(settings);
    let priority_class = priority_override
        .clone()
        .unwrap_or_else(|| module_policy.priority_class.clone());
    let priority_source = if priority_override.is_some() {
        "instance_override"
    } else {
        "module_default"
    };

    let logical_cpu_count = available_logical_cpu_count().clamp(1, 64);
    let explicit_affinity_mask = resolve_u64_setting(
        settings,
        &[
            "runtime_performance.cpu_affinity_mask",
            "performance.cpu_affinity_mask",
            "runtime_cpu_affinity_mask",
        ],
    )
    .filter(|mask| *mask > 0);
    let affinity_preset =
        resolve_cpu_affinity_preset_descriptor(settings, instance_id, logical_cpu_count);
    let (cpu_affinity_mask, cpu_affinity_source, cpu_affinity_preset) =
        if let Some(mask) = explicit_affinity_mask {
            (Some(mask), "instance_mask", None)
        } else if let Some((preset, mask)) = affinity_preset {
            (mask, "instance_preset", Some(preset))
        } else if module_policy.cpu_affinity_mask.is_some() {
            (module_policy.cpu_affinity_mask, "module_default", None)
        } else {
            (None, "all_cpus", None)
        };

    let policy = RuntimePerformancePolicy {
        resource_limits: settings
            .pointer("/runtime_performance/resource_limits")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_else(|| module_policy.resource_limits.clone()),
        priority_class,
        cpu_affinity_mask,
        apply_to_child_processes: resolve_bool_setting(
            settings,
            &[
                "runtime_performance.apply_to_child_processes",
                "performance.apply_to_child_processes",
                "runtime_apply_to_child_processes",
            ],
        )
        .unwrap_or(module_policy.apply_to_child_processes),
        startup_stagger_ms: resolve_u64_setting(
            settings,
            &[
                "runtime_performance.startup_stagger_ms",
                "performance.startup_stagger_ms",
                "runtime_startup_stagger_ms",
            ],
        )
        .unwrap_or(module_policy.startup_stagger_ms),
        child_process_stagger_ms: resolve_u64_setting(
            settings,
            &[
                "runtime_performance.child_process_stagger_ms",
                "performance.child_process_stagger_ms",
                "runtime_child_process_stagger_ms",
            ],
        )
        .unwrap_or(module_policy.child_process_stagger_ms),
    };
    let preview = RuntimePerformancePolicyPreview {
        summary: runtime_performance_preview_summary(
            &policy,
            priority_source,
            cpu_affinity_source,
            cpu_affinity_preset.as_deref(),
            logical_cpu_count,
        ),
        priority_source: String::from(priority_source),
        cpu_affinity_source: String::from(cpu_affinity_source),
        cpu_affinity_preset,
        logical_cpu_count,
    };

    RuntimePerformanceResolution { policy, preview }
}

pub(super) fn resolve_cpu_affinity_preset_descriptor(
    settings: &Value,
    instance_id: &str,
    cpu_count: usize,
) -> Option<(String, Option<u64>)> {
    let preset = resolve_string_setting(
        settings,
        &[
            "runtime_performance.cpu_affinity_preset",
            "performance.cpu_affinity_preset",
            "runtime_cpu_affinity_preset",
        ],
    )?;
    let preset = preset.trim().to_ascii_lowercase().replace('-', "_");
    let mask = match preset.as_str() {
        "all" | "all_cpus" => None,
        "host_reserve" | "reserve_first_core" => host_reserve_affinity_mask(cpu_count),
        "first_half" => half_affinity_mask(cpu_count, false),
        "second_half" => half_affinity_mask(cpu_count, true),
        "multi_instance_balance" | "balanced_multi_instance" | "auto_balance" => {
            balanced_instance_affinity_mask(cpu_count, instance_id)
        }
        _ => return None,
    };
    Some((preset, mask))
}

pub(super) fn runtime_performance_preview_summary(
    policy: &RuntimePerformancePolicy,
    priority_source: &str,
    cpu_affinity_source: &str,
    cpu_affinity_preset: Option<&str>,
    logical_cpu_count: usize,
) -> String {
    let affinity = policy
        .cpu_affinity_mask
        .map(|mask| format!("0x{mask:X}"))
        .unwrap_or_else(|| String::from("all CPUs"));
    let preset = cpu_affinity_preset
        .map(|preset| format!(", preset={preset}"))
        .unwrap_or_default();
    format!(
        "Runtime performance policy resolves to priority={:?} ({priority_source}), affinity={affinity} ({cpu_affinity_source}{preset}) on {logical_cpu_count} logical CPU(s), instance stagger={}ms, child process stagger={}ms. Instance CPU cap={}, committed-memory cap={}, host memory reserve={} MiB (only with a memory cap).",
        policy.priority_class,
        policy.startup_stagger_ms,
        policy.child_process_stagger_ms,
        policy
            .resource_limits
            .cpu_percent
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "unlimited".into()),
        policy
            .resource_limits
            .memory_limit_mib
            .map(|value| format!("{value} MiB"))
            .unwrap_or_else(|| "unlimited".into()),
        policy.resource_limits.host_memory_reserve_mib
    )
}

pub(super) fn available_logical_cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

pub(super) fn host_reserve_affinity_mask(cpu_count: usize) -> Option<u64> {
    if cpu_count <= 2 {
        return None;
    }
    let all = affinity_mask_for_range(0, cpu_count);
    Some(all & !1)
}

pub(super) fn half_affinity_mask(cpu_count: usize, second_half: bool) -> Option<u64> {
    if cpu_count <= 2 {
        return None;
    }
    let split = (cpu_count / 2).max(1);
    if second_half {
        Some(affinity_mask_for_range(split, cpu_count))
    } else {
        Some(affinity_mask_for_range(0, split))
    }
    .filter(|mask| *mask > 0)
}

pub(super) fn balanced_instance_affinity_mask(cpu_count: usize, instance_id: &str) -> Option<u64> {
    if cpu_count <= 2 {
        return None;
    }
    let group_count = balanced_affinity_group_count(cpu_count);
    let bucket = deterministic_instance_bucket(instance_id) as usize;
    balanced_affinity_mask_for_group(cpu_count, bucket % group_count, group_count)
}

pub(super) fn balanced_affinity_group_count(cpu_count: usize) -> usize {
    (cpu_count / 2).clamp(2, 4)
}

pub(super) fn balanced_affinity_mask_for_group(
    cpu_count: usize,
    group_index: usize,
    group_count: usize,
) -> Option<u64> {
    if cpu_count <= 2 || group_count == 0 {
        return None;
    }
    let cpu_count = cpu_count.min(64);
    let group_count = group_count.min(cpu_count).max(1);
    let group_index = group_index.min(group_count - 1);
    let base_width = cpu_count / group_count;
    let remainder = cpu_count % group_count;
    let start = group_index * base_width + group_index.min(remainder);
    let width = base_width + usize::from(group_index < remainder);
    Some(affinity_mask_for_range(start, start + width)).filter(|mask| *mask > 0)
}

pub(super) fn affinity_mask_for_range(start: usize, end: usize) -> u64 {
    let mut mask = 0_u64;
    for index in start.min(64)..end.min(64) {
        mask |= 1_u64 << index;
    }
    mask
}

pub(super) fn deterministic_instance_bucket(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub(super) fn resolve_runtime_priority_override(settings: &Value) -> Option<RuntimePriorityClass> {
    resolve_string_setting(
        settings,
        &[
            "runtime_performance.priority_class",
            "performance.priority_class",
            "runtime_priority_class",
        ],
    )
    .and_then(|value| parse_runtime_priority_class(&value))
}

pub(super) fn parse_runtime_priority_class(value: &str) -> Option<RuntimePriorityClass> {
    match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "idle" => Some(RuntimePriorityClass::Idle),
        "below_normal" | "belownormal" => Some(RuntimePriorityClass::BelowNormal),
        "normal" => Some(RuntimePriorityClass::Normal),
        "above_normal" | "abovenormal" => Some(RuntimePriorityClass::AboveNormal),
        "high" => Some(RuntimePriorityClass::High),
        _ => None,
    }
}

pub(super) fn resolve_string_setting(settings: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        lookup_json_value(settings, path).and_then(|value| match value {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
    })
}

pub(super) fn resolve_bool_setting(settings: &Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|path| {
        lookup_json_value(settings, path).and_then(|value| match value {
            Value::Bool(flag) => Some(*flag),
            Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        })
    })
}

pub(super) fn resolve_u64_setting(settings: &Value, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|path| {
        lookup_json_value(settings, path).and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => parse_u64_setting(text),
            _ => None,
        })
    })
}

pub(super) fn parse_u64_setting(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u64>().ok()
    }
}
