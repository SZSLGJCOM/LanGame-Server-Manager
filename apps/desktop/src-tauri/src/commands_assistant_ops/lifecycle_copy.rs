#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssistantLifecycleLocale {
    ZhCn,
    En,
}

impl AssistantLifecycleLocale {
    // This preference selects copy and official network routing, never a plan or its authorization.
    fn from_input(input: &AssistantExecuteOperationInput) -> Self {
        let language = input
            .context
            .as_deref()
            .and_then(|context| serde_json::from_str::<Value>(context).ok())
            .and_then(|context| {
                context
                    .get("interfaceLanguage")
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)
            });
        match language.as_deref() {
            Some("zh" | "zh-cn" | "zh-hans") => Self::ZhCn,
            Some("en" | "en-us" | "en-gb") => Self::En,
            _ if input
                .prompt
                .chars()
                .any(|character| ('\u{3400}'..='\u{9fff}').contains(&character)) =>
            {
                Self::ZhCn
            }
            _ => Self::En,
        }
    }

    fn pending(self, summary: &str) -> String {
        match self {
            Self::ZhCn => format!("待确认：{summary}"),
            Self::En => format!("Pending confirmation: {summary}"),
        }
    }

    fn source_preference(self) -> app_network::SourcePreference {
        match self {
            Self::ZhCn => app_network::SourcePreference::ChinaFirst,
            Self::En => app_network::SourcePreference::InternationalFirst,
        }
    }

    fn stopped(self, name: &str) -> String {
        match self {
            Self::ZhCn => format!("已停止「{name}」，并确认该服务进程已退出。"),
            Self::En => format!("Stopped “{name}” and verified that its managed run has exited."),
        }
    }

    fn restarting(self, name: &str) -> String {
        match self {
            Self::ZhCn => format!("「{name}」的原进程已停止，新进程已启动，正在检查是否就绪。"),
            Self::En => format!(
                "Stopped the previous run of “{name}” and started a new run; checking its readiness."
            ),
        }
    }

    fn restart_cancelled(self) -> String {
        match self {
            Self::ZhCn => "原进程已停止。请求在重新启动前被取消，尚未启动新进程。".into(),
            Self::En => "The previous run was stopped. The request was cancelled before restart; no new run was started.".into(),
        }
    }

    fn restart_failed(self, reason: &str) -> String {
        match self {
            Self::ZhCn => format!("原进程已停止，但重新启动失败：{reason}"),
            Self::En => format!("The previous run was stopped, but restart failed: {reason}"),
        }
    }

    fn verified(self, action: AssistantOperationAction, name: &str, evidence: &Value) -> String {
        let backup_id = evidence["backupId"].as_str().unwrap_or_default();
        let safeguard_id = evidence["safeguardBackupId"].as_str().unwrap_or_default();
        match (self, action) {
            (_, AssistantOperationAction::StopServer) => self.stopped(name),
            (Self::ZhCn, AssistantOperationAction::RestartServer) => {
                format!("已重启「{name}」，新进程已通过就绪检查。")
            }
            (Self::En, AssistantOperationAction::RestartServer) => {
                format!("Restarted “{name}”; the new run passed its readiness checks.")
            }
            (Self::ZhCn, AssistantOperationAction::CreateBackup) => {
                format!("已为「{name}」创建存档备份 {backup_id}，并核对备份内容。服务器保持停止。")
            }
            (Self::En, AssistantOperationAction::CreateBackup) => format!(
                "Created save backup {backup_id} for “{name}” and verified its contents. The server remains stopped."
            ),
            (Self::ZhCn, AssistantOperationAction::RestoreBackup) => format!(
                "已为「{name}」恢复备份 {backup_id}，并核对恢复后的内容。恢复前的存档已保存在备份 {safeguard_id} 中，服务器保持停止。"
            ),
            (Self::En, AssistantOperationAction::RestoreBackup) => format!(
                "Restored backup {backup_id} for “{name}” and verified its contents. Previous saves are preserved in {safeguard_id}; the server remains stopped."
            ),
            _ => unreachable!("lifecycle copy is only used for lifecycle actions"),
        }
    }

    fn unverified(self, name: &str, failed: bool, detail: &str) -> String {
        match (self, failed) {
            (Self::ZhCn, true) => format!("「{name}」的操作未完成。{detail}"),
            (Self::ZhCn, false) => format!("「{name}」的操作结果尚未确认。{detail}"),
            (Self::En, true) => format!("The operation on “{name}” did not complete. {detail}"),
            (Self::En, false) => {
                format!("The result for “{name}” could not be fully verified. {detail}")
            }
        }
    }
}

fn assistant_lifecycle_preview_copy(
    locale: AssistantLifecycleLocale,
    action: AssistantOperationAction,
    instance: &InstanceDetails,
    backup: Option<&app_core::InstanceBackupResult>,
    task: &AssistantTaskContract,
) -> Result<String, String> {
    use AssistantLifecycleLocale::{En, ZhCn};
    let name = &instance.summary.name;
    let mut summary = match (locale, action) {
        (ZhCn, AssistantOperationAction::StopServer) => {
            format!("停止「{name}」的当前服务进程，并确认已停止。当前玩家连接会中断。")
        }
        (En, AssistantOperationAction::StopServer) => format!(
            "Stop the current managed run of “{name}” and verify it has stopped. Connected players will be disconnected."
        ),
        (ZhCn, AssistantOperationAction::RestartServer) => format!(
            "重启「{name}」：先停止当前服务进程，再启动新进程并检查是否就绪。当前玩家连接会中断。"
        ),
        (En, AssistantOperationAction::RestartServer) => format!(
            "Restart “{name}”: stop the current run, then start a new run and check its readiness. Connected players will be disconnected."
        ),
        (ZhCn, AssistantOperationAction::CreateBackup) => {
            format!("为「{name}」创建当前存档的备份，并核对备份内容。服务器保持停止。")
        }
        (En, AssistantOperationAction::CreateBackup) => format!(
            "Create a backup of the current saves for “{name}” and verify its contents. The server remains stopped."
        ),
        (ZhCn, AssistantOperationAction::RestoreBackup) => format!(
            "恢复「{name}」的存档备份。会先备份当前存档，再用所选备份替换当前存档。完成后服务器保持停止。"
        ),
        (En, AssistantOperationAction::RestoreBackup) => format!(
            "Restore a save backup for “{name}”. First preserve a safeguard of the current saves, then replace them with the selected backup. The server remains stopped."
        ),
        _ => return Err("Lifecycle preview requires a lifecycle action.".into()),
    };
    if action == AssistantOperationAction::RestoreBackup {
        let backup = backup.ok_or("The reviewed backup metadata is missing.")?;
        let created = assistant_backup_utc_time(backup.created_at_unix_ms);
        let bytes = assistant_backup_size_copy(locale, backup.total_bytes);
        summary.push_str(&match locale {
            ZhCn => format!(
                "\n备份：{}\n创建时间：{created}\n内容：{} 个文件，共 {bytes}",
                backup.backup_id, backup.file_count
            ),
            En => format!(
                "\nBackup: {}\nCreated: {created}\nContents: {} files, {bytes}",
                backup.backup_id, backup.file_count
            ),
        });
    }
    let stops = matches!(
        action,
        AssistantOperationAction::StopServer | AssistantOperationAction::RestartServer
    );
    if stops && instance.auto_backup_on_stop {
        summary.push_str(match locale {
            ZhCn => "\n停服后将按当前策略自动备份。",
            En => "\nThe current policy also creates an automatic backup after stopping.",
        });
    }
    if !stops || instance.auto_backup_on_stop {
        summary.push_str(&match locale {
            ZhCn => format!("\n备份保留数量：{}；按现有规则清理超出的旧备份。", instance.backup_retention_count.max(1)),
            En => format!("\nBackup retention: {}; older backups beyond this limit are cleaned up under the existing policy.", instance.backup_retention_count.max(1)),
        });
        if action == AssistantOperationAction::RestoreBackup {
            summary.push_str(match locale {
                ZhCn => "本次使用的备份和恢复前备份都会保留。",
                En => " The selected backup and the safeguard are both preserved.",
            });
        }
    }
    if task.request.preserve_existing_mods {
        summary.push_str(match locale {
            ZhCn => "\n保留现有启用的 Mod、分片及 Mod 配置。",
            En => "\nPreserve the existing enabled mods, shards and mod configuration.",
        });
    }
    if let Some(requirements) = &task.requirements {
        let views = requirements.views();
        if !views.is_empty() {
            summary.push_str(match locale {
                ZhCn => "\n请同时核对本次请求的约束：",
                En => "\nAlso review the requirements for this request:",
            });
        }
        for view in views {
            summary.push_str(&format!("\n- {}", view.description));
            if let Some(target) = &view.target {
                summary.push_str(&match locale {
                    ZhCn => format!(
                        "\n  {}：{target}",
                        if view.kind == "forbidden_action" {
                            "禁止操作"
                        } else {
                            "对象"
                        }
                    ),
                    En => format!(
                        "\n  {}: {target}",
                        if view.kind == "forbidden_action" {
                            "Forbidden action"
                        } else {
                            "Target"
                        }
                    ),
                });
            }
            if let Some(expected) = &view.expected_display {
                summary.push_str(&match locale {
                    ZhCn => format!("\n  期望：{expected}"),
                    En => format!("\n  Expected: {expected}"),
                });
            }
            summary.push_str(&match locale {
                ZhCn => format!("\n  原话：「{}」", view.source_text),
                En => format!("\n  Your words: “{}”", view.source_text),
            });
        }
    }
    Ok(summary)
}

fn assistant_backup_size_copy(locale: AssistantLifecycleLocale, bytes: u64) -> String {
    let exact = match locale {
        AssistantLifecycleLocale::ZhCn => format!("{bytes} 字节"),
        AssistantLifecycleLocale::En => format!("{bytes} bytes"),
    };
    for (unit, divisor) in [
        ("TiB", 1_u64 << 40),
        ("GiB", 1 << 30),
        ("MiB", 1 << 20),
        ("KiB", 1 << 10),
    ] {
        if bytes >= divisor {
            return format!("{:.1} {unit} ({exact})", bytes as f64 / divisor as f64);
        }
    }
    exact
}

// No date crate is used in this crate. Convert bounded Unix days to the Gregorian
// calendar; label UTC explicitly rather than guessing the operator's time zone.
fn assistant_backup_utc_time(unix_ms: u128) -> String {
    if unix_ms > 253_402_300_799_999 {
        return format!("Unix ms {unix_ms}");
    }
    let seconds = (unix_ms / 1000) as i64;
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
#[path = "lifecycle_copy_tests.rs"]
mod lifecycle_copy_tests;
