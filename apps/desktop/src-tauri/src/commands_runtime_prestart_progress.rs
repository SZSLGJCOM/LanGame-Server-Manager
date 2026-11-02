use app_core::{InstallPhase, InstallVerificationProgress, InstallVerificationStage};
use app_steamcmd::{InstallConsoleOutput, InstallProgressUpdate};

#[derive(Default)]
pub(super) struct PrestartProgressLog {
    last: Option<LoggedProgress>,
}

struct LoggedProgress {
    line: String,
    verification: Option<InstallVerificationProgress>,
    phase: Option<InstallPhase>,
    committed: String,
}

#[derive(Default)]
pub(super) struct Projection {
    pub durable: Vec<String>,
    pub preview: Option<String>,
}

impl PrestartProgressLog {
    pub(super) fn update(&mut self, update: &InstallProgressUpdate) -> Projection {
        let progress = update
            .install_progress
            .as_ref()
            .filter(|progress| progress.phase == InstallPhase::Verifying);
        let verification = progress.and_then(|progress| progress.verification.as_ref());
        let mut line = update.detail.clone();
        if let Some(verification) = verification {
            if verification.stage != InstallVerificationStage::RecordingBaseline {
                if let Some(percent) = progress.and_then(|progress| progress.percent) {
                    line.push_str(&format!(" {percent:.1}%"));
                }
                if verification.total_bytes > 0 {
                    line.push_str(&format!(
                        " · {} / {}",
                        format_bytes(verification.completed_bytes),
                        format_bytes(verification.total_bytes),
                    ));
                }
            }
            if verification.total_files > 0 {
                line.push_str(&format!(
                    " · {} / {} 个文件",
                    verification.completed_files, verification.total_files,
                ));
            }
        }
        if verification.is_none()
            && !line.contains("progress:")
            && let Some(value) = update.install_progress.as_ref()
        {
            if let Some(percent) = value.percent
                && !line.contains('%')
            {
                line.push_str(&format!(" {percent:.1}%"));
            }
            if let (Some(done), Some(total)) = (value.downloaded_bytes, value.total_bytes) {
                line.push_str(&format!(
                    " · {} / {}",
                    format_bytes(done),
                    format_bytes(total)
                ));
            }
        }
        if let Some(InstallConsoleOutput::Progress {
            retain_boundaries: false,
            preview,
        }) = &update.console_output
        {
            let mut output = self.finish();
            output.preview = (*preview && !line.is_empty()).then_some(line);
            return output;
        }
        let phase = update
            .install_progress
            .as_ref()
            .map(|progress| progress.phase);
        let measured = verification.is_some()
            || matches!(
                update.console_output,
                Some(InstallConsoleOutput::Progress { .. })
            );
        if !measured {
            let mut output = self.finish();
            if !line.is_empty() {
                output.durable.push(line);
            }
            return output;
        }
        let same_inventory = self.last.as_ref().is_some_and(|last| {
            last.phase == phase
                && match (verification, &last.verification) {
                    (Some(current), Some(previous)) => {
                        current.stage == previous.stage
                            && current.total_bytes == previous.total_bytes
                            && current.total_files == previous.total_files
                    }
                    (None, None) => true,
                    _ => false,
                }
        });
        let complete = verification.map_or_else(
            || {
                update
                    .install_progress
                    .as_ref()
                    .and_then(|value| value.percent)
                    .is_some_and(|percent| percent >= 100.0)
            },
            |value| {
                (value.total_bytes > 0 || value.total_files > 0)
                    && value.completed_bytes >= value.total_bytes
                    && value.completed_files >= value.total_files
            },
        );
        let mut output = Projection::default();
        if !same_inventory {
            output = self.finish();
            output.durable.push(line.clone());
            self.last = Some(LoggedProgress {
                line: line.clone(),
                verification: verification.cloned(),
                phase,
                committed: line.clone(),
            });
        }
        if let Some(last) = &mut self.last {
            last.line = line.clone();
            if complete && last.committed != line {
                output.durable.push(line.clone());
                last.committed = line;
            } else if last.committed != line {
                output.preview = Some(line);
            }
        }
        output
    }

    pub(super) fn finish(&mut self) -> Projection {
        let mut output = Projection::default();
        if let Some(last) = self.last.take()
            && last.line != last.committed
        {
            output.durable.push(last.line);
        }
        output
    }
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

#[cfg(test)]
#[path = "commands_runtime_prestart_progress_tests.rs"]
mod tests;
