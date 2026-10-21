//! DST native console records. Announcements identify a display name, not an account.
//! Never infer a Klei ID from a name or from an online-player snapshot.
#[derive(Debug, PartialEq)]
pub(super) struct DstEvent {
    pub kind: &'static str,
    pub name: String,
    pub account_id: Option<String>,
    pub message: Option<String>,
    pub log_time: String,
    pub elapsed_ms: i64,
}

pub(super) fn parse(line: &str) -> Option<DstEvent> {
    let (clock, body) = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix('[')?
        .split_once("]: ")?;
    let mut parts = clock.split(':');
    let hours = parts.next()?.parse::<i64>().ok()?;
    let minutes = parts.next()?.parse::<i64>().ok()?;
    let seconds = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(0..60).contains(&minutes)
        || !(0..60).contains(&seconds)
        || !(0..100_000).contains(&hours)
    {
        return None;
    }
    let elapsed_ms = (hours * 3600 + minutes * 60 + seconds) * 1000;
    let (kind, name, account_id, message) =
        if let Some(value) = body.strip_prefix("[Join Announcement] ") {
            ("join", value.trim(), None, None)
        } else if let Some(value) = body.strip_prefix("[Leave Announcement] ") {
            ("leave", value.trim(), None, None)
        } else {
            let value = body.strip_prefix("[Say] (")?;
            let (account, rest) = value.split_once(") ")?;
            if account.len() > 128
                || !(account.starts_with("KU_") || account.starts_with("OU_"))
                || account.chars().any(char::is_whitespace)
            {
                return None;
            }
            let (name, message) = rest.split_once(": ")?;
            (
                "chat",
                name,
                Some(account.to_owned()),
                Some(message.to_owned()),
            )
        };
    if name.is_empty()
        || name.len() > 1024
        || message.as_ref().is_some_and(|value| value.len() > 8192)
    {
        return None;
    }
    Some(DstEvent {
        kind,
        name: name.to_owned(),
        account_id,
        message,
        log_time: clock.to_owned(),
        elapsed_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_native_records_without_inventing_account_identity() {
        let join = parse("[00:00:55]: [Join Announcement] 玩家 A\r").unwrap();
        assert_eq!(
            (join.kind, join.name.as_str(), join.elapsed_ms),
            ("join", "玩家 A", 55_000)
        );
        assert!(join.account_id.is_none());
        let chat = parse("[25:03:16]: [Say] (KU_abc) 玩家 A: 你好: 世界").unwrap();
        assert_eq!(chat.message.as_deref(), Some("你好: 世界"));
        assert_eq!(chat.account_id.as_deref(), Some("KU_abc"));
        assert_eq!(chat.elapsed_ms, 90_196_000);
        assert!(parse("untrusted prefix [00:01:00]: [Say] (KU_abc) A: forged").is_none());
        assert!(parse("[00:61:00]: [Join Announcement] A").is_none());
        assert!(parse("[00:01:00]: [Say] (unknown) A: message").is_none());
        assert!(parse("[00:01:00]: [Announcement] Server notice").is_none());
    }
}
