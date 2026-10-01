pub(crate) fn is_date_or_local_datetime(value: &str) -> bool {
    if is_iso_calendar_date(value) {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() == 19
        && value.is_ascii()
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[16] == b':'
        && is_iso_calendar_date(&value[..10])
        && parse_ascii_digits(&bytes[11..13]).is_some_and(|hour| hour <= 23)
        && parse_ascii_digits(&bytes[14..16]).is_some_and(|minute| minute <= 59)
        && parse_ascii_digits(&bytes[17..19]).is_some_and(|second| second <= 59)
}

pub(crate) fn is_iso_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }

    let year = parse_ascii_digits(&bytes[0..4]);
    let month = parse_ascii_digits(&bytes[5..7]);
    let day = parse_ascii_digits(&bytes[8..10]);
    let Some((year, month, day)) = year.zip(month).zip(day).map(|((y, m), d)| (y, m, d)) else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }

    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days_in_month).contains(&day)
}

pub(crate) fn validate_platform_account_id(platform: &str, account_id: &str) -> Result<(), String> {
    let valid = match platform.trim().to_ascii_lowercase().as_str() {
        "steam" => account_id.len() == 17 && account_id.bytes().all(|byte| byte.is_ascii_digit()),
        "eos" => {
            (8..=32).contains(&account_id.len())
                && account_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        }
        "xbl" | "psn" => {
            !account_id.is_empty()
                && account_id.len() <= 64
                && account_id.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
                })
        }
        _ => false,
    };
    if valid {
        return Ok(());
    }

    let requirement = match platform.trim().to_ascii_lowercase().as_str() {
        "steam" => String::from("must be a 17-digit Steam64 ID for the Steam platform"),
        "eos" => String::from("must be an 8-32 character hexadecimal Epic Online Services ID"),
        "xbl" | "psn" => format!(
            "must be a 1-64 character safe account ID for the {} platform",
            platform.trim().to_ascii_uppercase()
        ),
        _ => String::from("cannot be validated because the account platform is unsupported"),
    };
    Err(requirement)
}

fn parse_ascii_digits(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u32::from(*byte - b'0'))
    })
}
