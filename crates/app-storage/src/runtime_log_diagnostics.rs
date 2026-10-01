/// Returns unresolved fatal diagnostics in their original log order.
pub fn runtime_fatal_log_lines(module_id: &str, lines: &[String]) -> Vec<String> {
    let is_dst = module_id.trim().eq_ignore_ascii_case("dontstarve");
    let mut generation_completed = false;
    let mut completed_retry = false;
    let mut failures = Vec::new();
    for line in lines.iter().rev() {
        if is_dst && let Some(body) = native_dst_log_body(line) {
            if body == "Generation complete, injecting world entities." {
                generation_completed = true;
                completed_retry = false;
            } else if body.starts_with("An error occured during world gen we will retry!") {
                completed_retry = generation_completed;
            } else if completed_retry && is_missing_required_prefab(body) {
                // DST returns nil for this failed generation attempt, then tries
                // a new map. Only a later retry and completed map clear it;
                // listener output and readiness acknowledgements do not.
                continue;
            }
        }
        let normalized = line.to_ascii_lowercase();
        if [
            "fatal",
            "unhandled exception",
            "segmentation fault",
            "access violation",
            "assertion failed",
            "panic:",
        ]
        .iter()
        .any(|pattern| normalized.contains(pattern))
        {
            failures.push(line.clone());
        }
    }
    failures.reverse();
    failures
}

fn native_dst_log_body(line: &str) -> Option<&str> {
    let line = line.trim();
    let Some(timestamped) = line.strip_prefix('[') else {
        return Some(line);
    };
    let (timestamp, body) = timestamped.split_once("]: ")?;
    let mut parts = timestamp.split(':');
    let hours = parts.next()?;
    let minutes = parts.next()?;
    let seconds = parts.next()?;
    if parts.next().is_some()
        || hours.len() < 2
        || !hours.bytes().all(|byte| byte.is_ascii_digit())
        || ![minutes, seconds].iter().all(|part| {
            part.len() == 2
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u8>().is_ok_and(|value| value < 60)
        })
    {
        return None;
    }
    Some(body.trim())
}

fn is_missing_required_prefab(body: &str) -> bool {
    let Some(rest) = body.strip_prefix("PANIC: missing required prefab [") else {
        return false;
    };
    let Some((prefab, counts)) = rest.split_once("]! Expected ") else {
        return false;
    };
    let Some((expected, actual)) = counts.split_once(", got ") else {
        return false;
    };
    !prefab.is_empty()
        && !prefab.contains(['[', ']'])
        && expected
            .parse::<u64>()
            .ok()
            .zip(actual.parse::<u64>().ok())
            .is_some_and(|(expected, actual)| actual < expected)
}

#[cfg(test)]
#[path = "runtime_log_diagnostics_tests.rs"]
mod tests;
