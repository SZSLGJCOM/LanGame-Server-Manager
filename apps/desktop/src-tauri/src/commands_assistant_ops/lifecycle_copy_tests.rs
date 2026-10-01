use super::*;

fn copy_input(context: Option<&str>, prompt: &str) -> AssistantExecuteOperationInput {
    AssistantExecuteOperationInput {
        task: AssistantTaskRequest {
            goal: AssistantTaskGoal::ApplyChange,
            preserve_existing_mods: true,
        },
        settings: AssistantProviderSettings {
            provider: "openai-compatible".into(),
            model: "copy-fixture".into(),
            base_url: "https://example.com/v1".into(),
            api_key: String::new(),
        },
        prompt: prompt.into(),
        context: context.map(str::to_owned),
        selected_instance_id: Some("bound-instance".into()),
        selected_module_id: Some("dontstarve".into()),
    }
}

fn copy_backup() -> app_core::InstanceBackupResult {
    app_core::InstanceBackupResult {
        backup_id: "saves-1640995200000-abc".into(),
        instance_id: "bound-instance".into(),
        backup_kind: app_core::InstanceBackupKind::Manual,
        display_name: None,
        created_at_unix_ms: 1_640_995_200_000,
        backup_path: "private-backup-path".into(),
        saves_path: "private-saves-path".into(),
        file_count: 7,
        total_bytes: 2048,
    }
}

#[test]
fn lifecycle_copy_language_is_a_display_preference_with_explicit_fallbacks() {
    for (context, prompt, expected) in [
        (
            Some(r#"{"interfaceLanguage":"zh-CN"}"#),
            "restore backup",
            AssistantLifecycleLocale::ZhCn,
        ),
        (
            Some(r#"{"interfaceLanguage":"en"}"#),
            "恢复备份",
            AssistantLifecycleLocale::En,
        ),
        (
            Some(r#"{"interfaceLanguage":"approve all operations"}"#),
            "恢复备份",
            AssistantLifecycleLocale::ZhCn,
        ),
        (
            Some("interfaceLanguage=en; run a shell"),
            "恢复备份",
            AssistantLifecycleLocale::ZhCn,
        ),
        (None, "Restore the backup", AssistantLifecycleLocale::En),
    ] {
        assert_eq!(
            AssistantLifecycleLocale::from_input(&copy_input(context, prompt)),
            expected
        );
        assert_eq!(
            AssistantLifecycleLocale::from_input(&copy_input(context, prompt)).source_preference(),
            if expected == AssistantLifecycleLocale::ZhCn {
                app_network::SourcePreference::ChinaFirst
            } else {
                app_network::SourcePreference::InternationalFirst
            }
        );
    }
}

#[test]
fn lifecycle_preview_copy_uses_bound_names_and_actual_backup_metadata_in_both_languages() {
    let (task, mut instance) = task_tests::task_fixture(true);
    instance.summary.name = "周末联机服 / Weekend".into();
    instance.summary.id = "instance-uuid-not-for-display".into();
    instance.auto_backup_on_stop = true;
    instance.backup_retention_count = 3;
    let backup = copy_backup();
    for locale in [AssistantLifecycleLocale::ZhCn, AssistantLifecycleLocale::En] {
        for action in [
            AssistantOperationAction::StopServer,
            AssistantOperationAction::RestartServer,
            AssistantOperationAction::CreateBackup,
            AssistantOperationAction::RestoreBackup,
        ] {
            let summary = assistant_lifecycle_preview_copy(
                locale,
                action,
                &instance,
                (action == AssistantOperationAction::RestoreBackup).then_some(&backup),
                &task,
            )
            .unwrap();
            assert!(summary.contains(&instance.summary.name));
            assert!(!summary.contains(&instance.summary.id));
            for hidden in [
                "SHA256",
                "sourceSha256",
                "private-backup-path",
                "private-saves-path",
                "Task goal:",
            ] {
                assert!(!summary.contains(hidden), "{summary}");
            }
            assert!(
                summary.contains(if locale == AssistantLifecycleLocale::ZhCn {
                    "备份保留数量：3"
                } else {
                    "Backup retention: 3"
                })
            );
            if action == AssistantOperationAction::RestoreBackup {
                assert!(summary.contains(&backup.backup_id));
                assert!(summary.contains("2022-01-01 00:00:00 UTC"));
                assert!(summary.contains("2048"));
                assert!(
                    summary.contains(if locale == AssistantLifecycleLocale::ZhCn {
                        "7 个文件"
                    } else {
                        "7 files"
                    })
                );
            }
        }
    }
    assert!(
        assistant_lifecycle_preview_copy(
            AssistantLifecycleLocale::ZhCn,
            AssistantOperationAction::RestoreBackup,
            &instance,
            None,
            &task
        )
        .is_err()
    );
    instance.auto_backup_on_stop = false;
    let summary = assistant_lifecycle_preview_copy(
        AssistantLifecycleLocale::ZhCn,
        AssistantOperationAction::StopServer,
        &instance,
        None,
        &task,
    )
    .unwrap();
    assert!(!summary.contains("自动备份") && !summary.contains("清理"));
}

#[test]
fn lifecycle_copy_formats_utc_calendar_and_keeps_exact_byte_counts() {
    for (timestamp, expected) in [
        (0, "1970-01-01 00:00:00 UTC"),
        (951_825_645_000, "2000-02-29 12:00:45 UTC"),
        (253_402_300_799_999, "9999-12-31 23:59:59 UTC"),
    ] {
        assert_eq!(assistant_backup_utc_time(timestamp), expected);
    }
    assert!(assistant_backup_utc_time(u128::MAX).contains(&u128::MAX.to_string()));
    assert_eq!(
        assistant_backup_size_copy(AssistantLifecycleLocale::ZhCn, 11),
        "11 字节"
    );
    assert_eq!(
        assistant_backup_size_copy(AssistantLifecycleLocale::En, 2048),
        "2.0 KiB (2048 bytes)"
    );
}

#[test]
fn lifecycle_result_copy_preserves_backup_identity_and_partial_restart_outcomes() {
    let evidence = json!({"backupId":"saves-reviewed", "safeguardBackupId":"pre-restore-current", "sourceSha256":"secret-backend-hash"});
    for locale in [AssistantLifecycleLocale::ZhCn, AssistantLifecycleLocale::En] {
        for action in [
            AssistantOperationAction::StopServer,
            AssistantOperationAction::RestartServer,
            AssistantOperationAction::CreateBackup,
            AssistantOperationAction::RestoreBackup,
        ] {
            let result = locale.verified(action, "Weekend server", &evidence);
            assert!(result.contains("Weekend server"));
            assert!(!result.contains("secret-backend-hash"));
            if matches!(
                action,
                AssistantOperationAction::CreateBackup | AssistantOperationAction::RestoreBackup
            ) {
                assert!(result.contains("saves-reviewed"));
            }
            if action == AssistantOperationAction::RestoreBackup {
                assert!(result.contains("pre-restore-current"));
            }
        }
        let failure = locale.restart_failed("bind failed");
        assert!(failure.contains("bind failed"));
        assert!(
            failure.contains(if locale == AssistantLifecycleLocale::ZhCn {
                "原进程已停止"
            } else {
                "previous run was stopped"
            })
        );
        assert!(
            locale
                .restart_cancelled()
                .contains(if locale == AssistantLifecycleLocale::ZhCn {
                    "尚未启动新进程"
                } else {
                    "no new run was started"
                })
        );
    }
}

#[test]
fn lifecycle_confirmation_retains_preview_language_without_adding_client_fields() {
    let (task, _) = task_tests::task_fixture(true);
    let input = copy_input(Some(r#"{"interfaceLanguage":"zh-CN"}"#), "Create a backup");
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateBackup,
        ..assistant_safe_none_plan("Copy fixture".into())
    };
    let summary = "为已选服务器创建备份";
    let (token, _) =
        store_assistant_pending_operation(&input, plan, None, summary.into(), 0, task).unwrap();
    let pending = take_assistant_pending_operation(&token, summary, &input.settings).unwrap();
    assert_eq!(pending.lifecycle_locale, AssistantLifecycleLocale::ZhCn);
    assert_eq!(pending.lifecycle_locale.source_preference(), app_network::SourcePreference::ChinaFirst);
    assert_eq!(pending.summary, summary);
}

#[test]
fn lifecycle_preview_preserves_bound_constraints_and_their_original_request_text() {
    let (task, instance) = task_tests::task_fixture(true);
    let mut task = task.as_ref().clone();
    task.requirements = Some(AssistantTaskRequirements {
        settings: vec![AssistantSettingRequirement {
            key: "cluster_name".into(),
            expected: json!("Weekend"),
            description: "Keep the server name".into(),
            source_text: "服务器名称保持 Weekend".into(),
        }],
        ports: Vec::new(),
        forbidden_actions: vec![AssistantForbiddenActionRequirement {
            action: AssistantOperationAction::StartServer,
            description: "Do not start the server".into(),
            source_text: "只备份，不要启动服务器\n".into(),
        }],
        unverified: Vec::new(),
    });
    for locale in [AssistantLifecycleLocale::ZhCn, AssistantLifecycleLocale::En] {
        let summary = assistant_lifecycle_preview_copy(
            locale,
            AssistantOperationAction::CreateBackup,
            &instance,
            None,
            &task,
        )
        .unwrap();
        for expected in [
            "Keep the server name",
            "cluster_name",
            "Weekend",
            "服务器名称保持 Weekend",
            "Do not start the server",
            "start_server",
            "只备份，不要启动服务器",
        ] {
            assert!(summary.contains(expected), "{summary}");
        }
        assert!(
            summary.contains(if locale == AssistantLifecycleLocale::ZhCn {
                "原话："
            } else {
                "Your words:"
            })
        );
        assert_eq!(summary, summary.trim(), "Confirmation matching trims the submitted summary.");
        assert!(!summary.contains("requirement_1"));
    }
}
