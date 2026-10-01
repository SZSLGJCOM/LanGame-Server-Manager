use super::*;

#[tokio::test]
async fn archive_root_cannot_be_claimed_by_instance_settings_creation_or_start() {
    for custom_root in [false, true] {
        let mut fixture = Fixture::new().await;
        if custom_root {
            fixture.storage.archives_root = fixture.root.join("archive-area");
        }
        fs::create_dir_all(&fixture.storage.archives_root).unwrap();
        let sentinel = fixture.storage.archives_root.join("retained-archive");
        fs::write(&sentinel, b"retained archive bytes").unwrap();
        let descriptor = fixture
            .module("fixture", "{{paths.config_dir}}/world")
            .await;
        let instance = fixture.create(&descriptor, "Normal instance").await;
        let details = read_instance_details(&fixture.storage, &instance.summary.id)
            .await
            .unwrap();
        let before = fs::read(&instance.config_file_path).unwrap();
        fixture.module("fixture", "{{settings.world}}").await;

        for target in [
            fixture.storage.archives_root.clone(),
            fixture.storage.archives_root.parent().unwrap().to_owned(),
            fixture.storage.archives_root.join("new-world"),
        ] {
            let mut input = update_input(&details);
            let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
            settings["world"] = json!(target.to_string_lossy());
            input.settings_json = settings.to_string();
            let error = update_instance(&fixture.storage, input).await.unwrap_err();
            assert!(error.to_string().contains("archive directory"), "{error}");
            assert_eq!(fs::read(&instance.config_file_path).unwrap(), before);
            assert_eq!(fs::read(&sentinel).unwrap(), b"retained archive bytes");
        }
        assert!(!fixture.storage.archives_root.join("new-world").exists());

        // A manually edited instance document is rejected again at start.
        let mut edited: Value = serde_json::from_slice(&before).unwrap();
        edited["settings"]["world"] = json!(fixture.storage.archives_root.to_string_lossy());
        let edited = serde_json::to_vec(&edited).unwrap();
        fs::write(&instance.config_file_path, &edited).unwrap();
        let error =
            materialize_instance_configuration_for_start(&fixture.storage, &instance.summary.id)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("archive directory"), "{error}");
        assert_eq!(fs::read(&instance.config_file_path).unwrap(), edited);

        let descriptor = fixture
            .module("fixture", &fixture.storage.archives_root.to_string_lossy())
            .await;
        let error = create_instance(
            &fixture.storage,
            &descriptor,
            CreateInstanceInput {
                name: "Blocked instance".into(),
                module_id: "fixture".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("archive directory"), "{error}");
        assert_eq!(fs::read(&sentinel).unwrap(), b"retained archive bytes");
    }
}
