//! Counted native RimWorld Together console tables, without a save hook.
use super::super::log_capture::{log_baseline, read_checked_delta};
use crate::runtime_log_stream::RuntimeLogTailState;
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

const SEPARATOR: &str = "----------------------------------------";
const MAX_ROWS: usize = 1024;

fn content(line: &str) -> &str {
    // ServerPrinter prepends one fixed HH:mm:ss timestamp. No name trimming:
    // whitespace in an account cannot become another native command argument.
    let bytes = line.as_bytes();
    if bytes.len() >= 13
        && bytes[0] == b'['
        && bytes[3] == b':'
        && bytes[6] == b':'
        && &bytes[9..13] == b"] | "
        && [1, 2, 4, 5, 7, 8]
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
    {
        &line[13..]
    } else {
        line
    }
}

fn table(lines: &[String], header: &str) -> Result<Option<Vec<String>>, String> {
    let Some((start, count)) = lines.iter().enumerate().find_map(|(index, line)| {
        content(line)
            .strip_prefix(header)
            .map(|count| (index, count))
    }) else {
        return Ok(None);
    };
    let count = count
        .strip_suffix(']')
        .and_then(|count| count.parse::<usize>().ok())
        .filter(|count| *count <= MAX_ROWS)
        .ok_or("RimWorld Together returned an invalid native player count.")?;
    if lines.len() < start + count + 3 {
        return Ok(None);
    }
    if content(&lines[start + 1]) != SEPARATOR || content(&lines[start + count + 2]) != SEPARATOR {
        return Err("RimWorld Together did not return a complete native player table.".into());
    }
    Ok(Some(
        lines[start + 2..start + count + 2]
            .iter()
            .map(|line| content(line).to_owned())
            .collect(),
    ))
}

fn validate_names(names: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    for name in &names {
        if name.is_empty()
            || name.len() > 128
            || name
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            || name == SEPARATOR
            || !seen.insert(name)
        {
            return Err(
                "RimWorld Together returned an unsafe or ambiguous username; no save was sent."
                    .into(),
            );
        }
    }
    Ok(names)
}

async fn collect<Dispatch, Future>(
    path: &Path,
    header: &str,
    dispatch: Dispatch,
) -> Result<Vec<String>, String>
where
    Dispatch: FnOnce() -> Future,
    Future: std::future::Future<Output = Result<(), String>>,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    let owned_path = path.to_owned();
    let baseline = tokio::task::spawn_blocking(move || log_baseline(&owned_path))
        .await
        .map_err(|_| "RimWorld Together console could not be opened.")??;
    dispatch().await?;
    let mut tail = RuntimeLogTailState::default();
    tail.byte_offset = baseline.byte_offset;
    let mut lines = Vec::new();
    let mut total = 0usize;
    loop {
        if Instant::now() >= deadline {
            return Err(
                "RimWorld Together native player table timed out; no save was sent.".into(),
            );
        }
        let remaining = 64 * 1024usize - total;
        if remaining == 0 {
            return Err("RimWorld Together native player table exceeded its limit.".into());
        }
        let owned_path = path.to_owned();
        let expected = baseline.clone();
        let (next, delta) = tokio::task::spawn_blocking(move || {
            let mut state = tail;
            let delta = read_checked_delta(&owned_path, &expected, &mut state, remaining);
            (state, delta)
        })
        .await
        .map_err(|_| "RimWorld Together console read failed.")?;
        tail = next;
        let delta = delta?;
        if delta.limit_exhausted
            || delta.stream_error.is_some()
            || tail.is_discarding_overlong_line()
        {
            return Err("RimWorld Together native player table was truncated or changed.".into());
        }
        total += delta.bytes_read;
        lines.extend(delta.lines);
        if let Some(rows) = table(&lines, header)? {
            return Ok(rows);
        }
        if delta.bytes_read == 0 {
            tokio::time::sleep(
                Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
    }
}

pub(crate) async fn connected_count<Dispatch, Future>(
    path: &Path,
    dispatch: Dispatch,
) -> Result<usize, String>
where
    Dispatch: FnOnce() -> Future,
    Future: std::future::Future<Output = Result<(), String>>,
{
    // Native list rows contain IP addresses, not usernames. Only their count is
    // used, and addresses never enter commands or new diagnostic output.
    collect(path, "Connected players: [", dispatch)
        .await
        .map(|rows| rows.len())
}

pub(crate) async fn usernames<Dispatch, Future>(
    path: &Path,
    dispatch: Dispatch,
) -> Result<Vec<String>, String>
where
    Dispatch: FnOnce() -> Future,
    Future: std::future::Future<Output = Result<(), String>>,
{
    validate_names(collect(path, "Server players: [", dispatch).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn lines(values: &[&str]) -> Vec<String> {
        values
            .iter()
            .map(|line| format!("[12:34:56] | {line}"))
            .collect()
    }
    #[test]
    fn counted_tables_require_both_boundaries_and_all_rows() {
        let rows = lines(&["Server players: [2]", SEPARATOR, "Alice", "Bob", SEPARATOR]);
        assert_eq!(
            table(&rows, "Server players: [").unwrap().unwrap(),
            vec!["Alice", "Bob"]
        );
        for length in 0..rows.len() {
            assert!(
                table(&rows[..length], "Server players: [")
                    .unwrap()
                    .is_none()
            );
        }
        let mut bad = rows.clone();
        bad[4] = "unrelated output".into();
        assert!(table(&bad, "Server players: [").is_err());
        assert!(table(&lines(&["Server players: [1025]"]), "Server players: [").is_err());
    }
    #[test]
    fn all_usernames_are_checked_before_any_command_is_built() {
        assert_eq!(
            validate_names(vec!["Alice".into(), "玩家".into()])
                .unwrap()
                .len(),
            2
        );
        for name in ["", "A B", " Alice", "x\nexit", "x\tY", SEPARATOR] {
            assert!(validate_names(vec!["Alice".into(), name.into()]).is_err());
        }
        assert!(validate_names(vec!["Alice".into(), "Alice".into()]).is_err());
    }
}
