use app_storage::SevenDaysBanReceipt;

const OWNER_PREFIX: &str = "Steam Family Sharing license owner ";
const RECEIPT_SEPARATOR: &str = " banned until ";
const RECEIPT_SUFFIX: &str = ", reason: LanGame.";

/// Command output is not proof of disk persistence. Return only the exact native
/// receipts that storage must independently match against the complete XML.
pub(super) fn confirmed_ban_receipts(
    response: Option<&str>,
    expected_target: &str,
) -> Result<Vec<SevenDaysBanReceipt>, String> {
    let response = response.ok_or_else(invalid_receipt)?;
    if !canonical_account(expected_target)
        || response.len() > 512 * 1024
        || !response.ends_with('\n')
        || response
            .chars()
            .any(|value| value.is_control() && !matches!(value, '\r' | '\n' | '\t'))
    {
        return Err(invalid_receipt());
    }
    let mut receipts: Vec<SevenDaysBanReceipt> = Vec::new();
    for line in response.lines().filter(|line| !line.is_empty()) {
        if line.starts_with("*** ERROR:") {
            return Err(invalid_receipt());
        }
        if timestamped_native_log(line)
            && !line.contains(RECEIPT_SEPARATOR)
            && !line.contains(OWNER_PREFIX)
        {
            continue;
        }
        let owner_line = line.strip_prefix(OWNER_PREFIX);
        let body = owner_line.unwrap_or(line);
        let Some((target, remainder)) = body.split_once(RECEIPT_SEPARATOR) else {
            // The dedicated transport proves the envelope's completion. Only
            // recognized timestamped log records may interrupt receipt lines.
            if !receipts.is_empty() || owner_line.is_some() {
                return Err(invalid_receipt());
            }
            continue;
        };
        let expiry = remainder
            .strip_suffix(RECEIPT_SUFFIX)
            .ok_or_else(invalid_receipt)?;
        if !canonical_account(target) || !local_datetime_shape(expiry) {
            return Err(invalid_receipt());
        }
        if owner_line.is_some() {
            if receipts.len() != 1
                || !target.starts_with("Steam_")
                || target == expected_target
                || receipts[0].unban_date != expiry
            {
                return Err(invalid_receipt());
            }
        } else if !receipts.is_empty() || target != expected_target {
            return Err(invalid_receipt());
        }
        receipts.push(SevenDaysBanReceipt {
            canonical_target: target.to_string(),
            unban_date: expiry.to_string(),
            reason: "LanGame".to_string(),
        });
    }
    if receipts.is_empty() {
        return Err(invalid_receipt());
    }
    Ok(receipts)
}

fn canonical_account(value: &str) -> bool {
    if let Some(id) = value.strip_prefix("Steam_") {
        id.len() == 17 && id.bytes().all(|byte| byte.is_ascii_digit())
    } else if let Some(id) = value.strip_prefix("EOS_") {
        (8..=32).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
    } else {
        false
    }
}

fn local_datetime_shape(value: &str) -> bool {
    // Full calendar validation is performed by storage's native expiry codec.
    value.len() == 19
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            10 => byte == b' ',
            13 | 16 => byte == b':',
            _ => byte.is_ascii_digit(),
        })
}

fn timestamped_native_log(line: &str) -> bool {
    let Some((timestamp, rest)) = line.split_once(' ') else {
        return false;
    };
    let timestamp = if let Some((seconds, fraction)) = timestamp.split_once('.') {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        seconds
    } else {
        timestamp
    };
    if timestamp.len() != 19 || !timestamp.is_ascii() || timestamp.as_bytes()[10] != b'T' {
        return false;
    }
    let date = format!("{} {}", &timestamp[..10], &timestamp[11..]);
    if !local_datetime_shape(&date) {
        return false;
    }
    let Some((elapsed, rest)) = rest.split_once(' ') else {
        return false;
    };
    let Some((level, _)) = rest.split_once(' ') else {
        return false;
    };
    !elapsed.is_empty()
        && elapsed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && elapsed
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && value >= 0.0)
        && matches!(level, "INF" | "WRN" | "ERR" | "EXC" | "DBG")
}

fn invalid_receipt() -> String {
    String::from(
        "The native response did not contain an unambiguous confirmation for the selected account and any family-sharing owner.",
    )
}

#[cfg(test)]
#[path = "commands_seven_days_ban_tests.rs"]
mod tests;
