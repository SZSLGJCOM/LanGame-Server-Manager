use super::*;

const LIVE_ORIGINALS: [&str; 2] = [
    "{\"listenPort\": 8088, \"preserved\": \"keep-json\", }\r\n",
    "retry_limit = 3\r\npreserved = \"keep-toml\r\n",
];

/// Real local model evaluation over synthetic files. This establishes only the
/// observed model's ability to use native tools and repair this syntax fixture.
/// It neither downloads models nor runs a game or an arbitrary plugin command.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires LANGAME_ASSISTANT_LIVE=1 and an already installed local Ollama model"]
async fn assistant_live_ollama_repairs_generic_synthetic_service_files() -> TestResult {
    if env::var("LANGAME_ASSISTANT_LIVE").as_deref() != Ok("1") {
        return Err("Set LANGAME_ASSISTANT_LIVE=1 to run the local-model evaluation.".into());
    }
    let model = env::var("LANGAME_ASSISTANT_LIVE_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
    if model.trim().is_empty() {
        return Err("LANGAME_ASSISTANT_LIVE_MODEL must be a nonempty installed model name.".into());
    }
    let provider = AssistantProviderSettings {
        provider: "ollama".into(),
        model: model.trim().into(),
        base_url: "http://127.0.0.1:11434/v1".into(),
        api_key: String::new(),
    };
    if !assistant_list_ollama_models(Some(provider.base_url.clone()))
        .await?
        .contains(&provider.model)
    {
        return Err(
            "The selected model is not installed locally; this evaluation never downloads models."
                .into(),
        );
    }
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("generic-live-evaluation");
    println!("ASSISTANT_GENERIC_LIVE_EVIDENCE={}", root.display());
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let instance = create_fake_module_instance(
        state.clone(),
        "minecraft",
        "Generic synthetic local-model fixture",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("instance root")?;
    for (file, source) in FILES.into_iter().zip(LIVE_ORIGINALS) {
        let path = instance_root.join(file);
        fs::create_dir_all(path.parent().ok_or("fixture parent")?)?;
        fs::write(path, source)?;
    }
    let untouched = instance_root.join("data/plugins/generic-synthetic-service/notes.txt");
    fs::write(
        &untouched,
        "Unrelated service notes must remain unchanged.\r\n",
    )?;
    let mut evidence = json!({"scenario":"generic_synthetic_json_toml_syntax", "provider":provider.provider,
        "model":provider.model,"endpoint":provider.base_url,"files":FILES,"scope":"syntax_and_file_receipts_only"});
    fs::write(
        root.join("generic-live-evaluation.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    let exercise = async {
        for file in FILES {
            let invalid = commands_assistant_ops::validate_assistant_workspace_file(
                state.inner(),
                &storage,
                &details,
                file,
            )
            .await?;
            assert_eq!(
                invalid["status"], "invalid",
                "the fixture must demonstrate a real syntax defect"
            );
        }
        // The prompt supplies the observed failure and user constraints, never
        // the corrected text, a tool name or a prescribed sequence of calls.
        let input = AssistantExecuteOperationInput {
            task: commands_assistant_ops::AssistantTaskRequest {
                goal: commands_assistant_ops::AssistantTaskGoal::ApplyChange,
                preserve_existing_mods: true,
            },
            settings: provider.clone(),
            prompt: String::from(
                "The synthetic service under data/plugins/generic-synthetic-service cannot load its JSON and TOML configuration because both files have syntax errors. Investigate the local files and propose one reviewed change that repairs both formats. Preserve every existing configuration value and all unrelated files. Keep the server stopped; only the configuration repair is requested.",
            ),
            context: None,
            selected_instance_id: Some(instance.summary.id.clone()),
            selected_module_id: Some("minecraft".into()),
        };
        let preview = assistant_preview_operation_inner(state.clone(), input).await?;
        evidence["preview"] = serde_json::to_value(&preview)?;
        fs::write(
            root.join("generic-live-evaluation.json"),
            serde_json::to_vec_pretty(&evidence)?,
        )?;
        if preview.action != AssistantOperationAction::PatchInstanceFiles
            || !preview.requires_confirmation
        {
            return Err(format!(
                "The model did not produce the required multi-file confirmation: {}",
                preview.message
            )
            .into());
        }
        let mut preview_files = preview
            .file_change_previews
            .iter()
            .map(|entry| entry.file.as_str())
            .collect::<Vec<_>>();
        preview_files.sort();
        assert_eq!(
            preview_files, FILES,
            "the model must limit its proposal to the broken files"
        );
        for (file, original) in FILES.into_iter().zip(LIVE_ORIGINALS) {
            assert_eq!(
                fs::read_to_string(instance_root.join(file))?,
                original,
                "preview cannot modify files"
            );
        }
        let confirmation = AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider.clone(),
            confirmation_token: preview.confirmation_token.ok_or("confirmation token")?,
            plan_summary: preview.plan_summary.ok_or("plan summary")?,
        };
        let output = assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
        evidence["confirmed"] = serde_json::to_value(&output)?;
        fs::write(
            root.join("generic-live-evaluation.json"),
            serde_json::to_vec_pretty(&evidence)?,
        )?;
        let result = output
            .file_changes_result
            .as_ref()
            .ok_or("The confirmed operation has no file receipts.")?;
        assert_eq!(
            result.status,
            app_storage::InstanceFilePatchesStatus::Applied
        );
        assert_eq!(
            output.task.as_ref().ok_or("task receipt")?.status,
            AssistantTaskStatus::Completed
        );
        let parsed_json: Value =
            serde_json::from_str(&fs::read_to_string(instance_root.join(FILES[0]))?)?;
        let parsed_toml: toml::Value =
            toml::from_str(&fs::read_to_string(instance_root.join(FILES[1]))?)?;
        assert_eq!(
            parsed_json,
            json!({"listenPort":8088,"preserved":"keep-json"})
        );
        let expected_toml: toml::Value =
            toml::from_str("retry_limit = 3\npreserved = 'keep-toml'\n")?;
        assert_eq!(
            parsed_toml, expected_toml,
            "syntax repairs must not remove or change values"
        );
        let mut validations = Vec::new();
        for (index, file) in FILES.into_iter().enumerate() {
            let receipt = result
                .files
                .iter()
                .find(|entry| entry.file == file)
                .ok_or("missing per-file receipt")?;
            assert!(receipt.read_back_verified);
            let backup = instance_root
                .join("data/.langame/file-patches")
                .join(receipt.backup_id.as_ref().ok_or("backup ID")?);
            assert_eq!(
                fs::read_to_string(backup.join("original"))?,
                LIVE_ORIGINALS[index]
            );
            let validation = commands_assistant_ops::validate_assistant_workspace_file(
                state.inner(),
                &storage,
                &details,
                file,
            )
            .await?;
            assert_eq!(validation["status"], "valid");
            assert_eq!(validation["sourceSha256"], receipt.result_sha256);
            validations.push(validation);
        }
        evidence["nativeValidations"] = json!(validations);
        assert_eq!(
            fs::read_to_string(&untouched)?,
            "Unrelated service notes must remain unchanged.\r\n"
        );
        assert!(
            read_instance_details(&storage.paths, &instance.summary.id)
                .await?
                .active_run
                .is_none()
        );
        assert!(output.follow_up.is_none());
        assert!(
            assistant_confirm_operation_inner(state.clone(), confirmation)
                .await
                .is_err()
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = match tokio::time::timeout(Duration::from_secs(10 * 60), exercise).await {
        Ok(result) => result,
        Err(error) => Err(error.into()),
    };
    evidence["passed"] = json!(result.is_ok());
    evidence["error"] = json!(result.as_ref().err().map(ToString::to_string));
    fs::write(
        root.join("generic-live-evaluation.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    result
}
