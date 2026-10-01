use super::{FileIdentity, Ledger};
use std::collections::HashSet;
use std::io;

fn invalid() -> io::Error {
    io::Error::other("invalid console ownership ledger structure")
}

fn run_name(name: &str) -> bool {
    name.len() <= 256
        && name.starts_with("run-")
        && name.ends_with(".log")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn numbered(name: &str, prefix: &str, suffix: &str) -> Option<u64> {
    let digits = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (digits.len() == 20 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

fn archive(name: &str) -> bool {
    numbered(name, "segment-", "/entries.log").is_some()
}

pub(super) fn validate(ledger: &Ledger, manifest_identity: &FileIdentity) -> io::Result<()> {
    if ledger.next_sequence == 0
        || ledger.moves.len() > 2
        || ledger.deletions.len() > 1
        || (!ledger.moves.is_empty() && !ledger.deletions.is_empty())
    {
        return Err(invalid());
    }
    let mut files = HashSet::new();
    let mut sequences = HashSet::new();
    let mut identities = Vec::new();
    for (run, parts) in &ledger.runs {
        if !run_name(run) || parts.is_empty() || parts.len() > 4 {
            return Err(invalid());
        }
        let mut previous = None;
        for (index, part) in parts.iter().enumerate() {
            if part.sequence == 0
                || part.sequence >= ledger.next_sequence
                || !files.insert(part.file.as_str())
                || !sequences.insert(part.sequence)
                || identities.contains(&&part.identity)
                || &part.identity == manifest_identity
                || (index + 1 == parts.len() && part.file != *run)
                || (index + 1 != parts.len() && !archive(&part.file))
                || previous.is_some_and(|(start, sequence)| {
                    part.start <= start || part.sequence <= sequence
                })
            {
                return Err(invalid());
            }
            identities.push(&part.identity);
            previous = Some((part.start, part.sequence));
        }
    }
    let mut move_sources = HashSet::new();
    let mut move_targets = HashSet::new();
    for movement in &ledger.moves {
        if !move_sources.insert(&movement.from) || !move_targets.insert(&movement.to) {
            return Err(invalid());
        }
        let Some((owner, target)) = ledger.runs.iter().find_map(|(run, parts)| {
            parts
                .iter()
                .find(|part| part.file == movement.to && part.identity == movement.identity)
                .map(|part| (run, part))
        }) else {
            return Err(invalid());
        };
        if let Some(sequence) = numbered(&movement.from, "pending-", ".log") {
            if movement.to != *owner || sequence != target.sequence {
                return Err(invalid());
            }
        } else if movement.from != *owner || !archive(&movement.to) {
            return Err(invalid());
        }
    }
    for part in &ledger.deletions {
        if (!run_name(&part.file) && !archive(&part.file))
            || files.contains(part.file.as_str())
            || identities.contains(&&part.identity)
            || &part.identity == manifest_identity
            || part.sequence == 0
            || part.sequence >= ledger.next_sequence
        {
            return Err(invalid());
        }
    }
    Ok(())
}
