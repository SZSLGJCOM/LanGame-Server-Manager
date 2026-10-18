//! Steam wraps a refused Workshop manifest request as "No Connection".
//! Correlate fresh evidence to the requested, failed item before requiring login.
use super::StreamingCommandResult;
use crate::steamcmd_manifest_recovery::ManifestDenial;
use std::collections::BTreeMap;

const MAX_ITEMS: usize = 8192;
const MAX_REASONS: usize = 32;

#[derive(Debug, Default)]
pub(crate) struct WorkshopEvidence {
    captured: bool,
    downloads: super::DownloadPaths,
    permissions: Vec<(Option<u32>, String)>,
    missing_keys: Vec<(Option<u32>, String)>,
    items: BTreeMap<(u32, u64), ItemEvidence>,
}

#[derive(Debug, Default)]
struct ItemEvidence {
    request: Option<String>,
    manifest: Option<(u64, String)>,
    no_connection: Option<String>,
}

impl WorkshopEvidence {
    pub(crate) fn finish_capture(&mut self) {
        self.captured = true;
    }
    pub(crate) fn seed_manifest(&mut self, app: u32, item: u64, manifest: u64) {
        if self.items.len() >= MAX_ITEMS && !self.items.contains_key(&(app, item)) {
            return;
        }
        self.items.entry((app, item)).or_default().manifest = Some((
            manifest,
            format!(
                "[AppID {app}] Requested item {item} target manifest {manifest} from WorkshopItemDetails"
            ),
        ));
    }
    pub(crate) fn observe_line(&mut self, line: &str) {
        self.downloads.observe_line(line);
        let scope = tagged_message(line).map(|(app, _)| app);
        if super::is_workshop_permission_denial(scope.unwrap_or_default(), line) {
            retain_reason(&mut self.permissions, scope, line);
        }
        if line.to_ascii_lowercase().contains("missing decryption key")
            && (scope.is_some() || (!line.contains("[AppID ") && !line.contains("AppID ")))
        {
            retain_reason(&mut self.missing_keys, scope, line);
        }
        let Some((app, message)) = tagged_message(line) else {
            return;
        };
        let record = if let Some(item) = item_message(message, " requested by app") {
            Some((item, 0, None))
        } else if let Some((item, manifest)) = changed_item_manifest(message) {
            Some((item, 1, Some(manifest)))
        } else if let Some((item, result)) = message
            .strip_prefix("Download item ")
            .and_then(|details| details.split_once(" result : "))
        {
            number(item).map(|item| {
                (
                    item,
                    if result.eq_ignore_ascii_case("No Connection") {
                        2
                    } else if result.eq_ignore_ascii_case("OK") {
                        4
                    } else {
                        3
                    },
                    None,
                )
            })
        } else {
            None
        };
        let Some((item, kind, manifest)) = record else {
            return;
        };
        if kind == 4 {
            self.items.remove(&(app, item));
            return;
        }
        if self.items.len() >= MAX_ITEMS && !self.items.contains_key(&(app, item)) {
            return;
        }
        let item = self.items.entry((app, item)).or_default();
        match kind {
            0 => {
                item.request = Some(line.into());
                item.no_connection = None;
            }
            1 => item.manifest = Some((manifest.unwrap(), line.into())),
            2 => item.no_connection = Some(line.into()),
            _ => item.no_connection = None,
        }
    }

    pub(crate) fn permission_denial(&self, app: u32) -> bool {
        reason_for(&self.permissions, app).is_some()
    }

    pub(crate) fn downloaded_item_path(&self, item: &str) -> Option<&std::path::Path> {
        self.downloads.item_path(item)
    }

    pub(crate) fn item_download_succeeded(&self, item: &str) -> bool {
        self.downloads.item_succeeded(item)
    }

    pub(crate) fn missing_key(&self, app: u32) -> bool {
        reason_for(&self.missing_keys, app).is_some()
    }

    fn denied_item(&self, app: u32, denials: &[ManifestDenial]) -> Option<&ItemEvidence> {
        self.items.iter().find_map(|((id, _), item)| {
            (*id == app
                && item.request.is_some()
                && item.no_connection.is_some()
                && item.manifest.as_ref().is_some_and(|(manifest, _)| {
                    denials.iter().any(|denial| {
                        denial.app == app
                            && denial.depot == u64::from(app)
                            && denial.manifest == *manifest
                    })
                }))
            .then_some(item)
        })
    }

    pub(crate) fn manifest_permission_denial(&self, app: u32, denials: &[ManifestDenial]) -> bool {
        self.denied_item(app, denials).is_some()
    }

    pub(crate) fn diagnostic_excerpt(
        &self,
        app: u32,
        denials: &[ManifestDenial],
    ) -> Option<String> {
        let mut lines = Vec::new();
        for reason in [&self.missing_keys, &self.permissions] {
            if let Some(line) = reason_for(reason, app) {
                lines.push(line.clone());
            }
        }
        if let Some(item) = self.denied_item(app, denials) {
            lines.extend(item.request.iter().cloned());
            lines.extend(item.manifest.iter().map(|(_, line)| line.clone()));
            lines.extend(item.no_connection.iter().cloned());
            if let Some((manifest, _)) = &item.manifest {
                lines.push(format!("[retained manifest permission evidence] AppID {app}, Depot {app}, Manifest {manifest}: Failed to get manifest request code, 'Access Denied'"));
            }
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

fn retain_reason(reasons: &mut Vec<(Option<u32>, String)>, app: Option<u32>, line: &str) {
    if reasons.len() < MAX_REASONS && !reasons.iter().any(|(id, _)| *id == app) {
        reasons.push((app, line.into()));
    }
}

fn reason_for(reasons: &[(Option<u32>, String)], app: u32) -> Option<&String> {
    reasons
        .iter()
        .find(|(id, _)| id.is_none() || *id == Some(app))
        .map(|(_, line)| line)
}

pub(super) fn has_permission_denial(app: u32, output: &StreamingCommandResult) -> bool {
    output
        .workshop_command_evidence
        .manifest_permission_denial(app, &output.denied_manifests)
        || tail_evidence(output).manifest_permission_denial(app, &output.denied_manifests)
}

pub(super) fn tail_evidence(output: &StreamingCommandResult) -> WorkshopEvidence {
    let mut evidence = WorkshopEvidence::default();
    if output.workshop_command_evidence.captured {
        return evidence;
    }
    for line in output.excerpt.lines().chain(
        output
            .workshop_log_excerpt
            .as_deref()
            .into_iter()
            .flat_map(str::lines),
    ) {
        evidence.observe_line(line);
    }
    evidence
}

fn tagged_message(line: &str) -> Option<(u32, &str)> {
    let message = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .filter(|(stamp, _)| {
            stamp.len() == 19 && stamp.as_bytes()[4] == b'-' && stamp.as_bytes()[10] == b' '
        })
        .map_or(line, |(_, message)| message)
        .trim();
    let (id, message) = if let Some(rest) = message.strip_prefix("[AppID ") {
        rest.split_once(']')?
    } else {
        message.strip_prefix("AppID ")?.split_once(' ')?
    };
    let app = id.parse::<u32>().ok().filter(|app| *app > 0)?;
    Some((app, message.trim()))
}

fn changed_item_manifest(message: &str) -> Option<(u64, u64)> {
    let (item, manifest) = message
        .strip_prefix("Detected workshop change (latest from server) : changed cached item ")?
        .split_once(", new manifest ")?;
    Some((number(item)?, number(manifest)?))
}

fn item_message(message: &str, suffix: &str) -> Option<u64> {
    number(
        message
            .strip_prefix("Download item ")?
            .strip_suffix(suffix)?,
    )
}

fn number(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse::<u64>().ok())
        .flatten()
        .filter(|value| *value != 0)
}

#[cfg(test)]
#[path = "workshop_manifest_access_tests.rs"]
mod tests;
