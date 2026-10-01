use super::*;

struct ProgramBaselineFixture {
    root: PathBuf,
    descriptor: ModuleDescriptor,
    state: DesktopState,
}

impl ProgramBaselineFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "lgsm-program-baseline-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root)?;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let mut descriptor = discover_modules(workspace.join("modules"))?
            .into_iter()
            .find(|module| module.summary.id == "minecraft")
            .unwrap();
        descriptor
            .storage
            .runtime_copy_exclusions
            .push("user.cfg".into());
        fs::write(root.join("server.jar"), b"official server payload")?;
        fs::write(root.join("user.cfg"), b"official default")?;
        Ok(Self {
            root,
            descriptor,
            state: DesktopState::default(),
        })
    }

    fn record(&self) -> Result<(), app_storage::StorageError> {
        app_storage::record_library_program_baseline(&self.root, &self.descriptor, true, None)
    }

    fn recorder(&self) -> Result<LibraryBaselineRecorder, String> {
        Ok(LibraryBaselineRecorder::new(
            &self
                .state
                .begin_storage_context_operation("program baseline regression")?,
            &self.descriptor,
            &app_steamcmd::InstallCancellation::new(),
        ))
    }

    fn clean_manifest(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        Ok(serde_json::from_slice(&fs::read(
            self.root.join(".langame-clean-package.json"),
        )?)?)
    }
}

impl Drop for ProgramBaselineFixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn program_update_same_version_preserves_verified_allowlist_without_trusting_user_files()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ProgramBaselineFixture::new()?;
    fixture.record()?;
    let before = fixture.clean_manifest()?;
    fs::write(
        fixture.root.join("custom-loader.dll"),
        b"operator modification",
    )?;
    fixture
        .recorder()?
        .finish((), fixture.root.clone(), false, true)
        .await?;
    assert_eq!(fixture.clean_manifest()?, before);
    assert!(app_storage::library_program_is_pristine(
        &fixture.root,
        &fixture.descriptor,
        None
    )?);
    assert!(
        fixture.clean_manifest()?["files"]
            .get("custom-loader.dll")
            .is_none()
    );
    assert_eq!(
        fs::read(fixture.root.join("custom-loader.dll"))?,
        b"operator modification"
    );
    Ok(())
}

#[tokio::test]
async fn program_update_new_version_invalidates_old_allowlist_even_when_old_hashes_still_match()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ProgramBaselineFixture::new()?;
    fixture.record()?;
    fs::write(
        fixture.root.join("new-required-library.jar"),
        b"new version dependency",
    )?;
    fixture
        .recorder()?
        .finish((), fixture.root.clone(), false, false)
        .await?;
    assert!(!fixture.root.join(".langame-clean-package.json").exists());
    assert!(!fixture.root.join(".langame-initial-package.json").exists());
    assert_eq!(
        fs::read(fixture.root.join("server.jar"))?,
        b"official server payload"
    );
    Ok(())
}

#[tokio::test]
async fn program_update_same_version_cannot_certify_changed_program_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ProgramBaselineFixture::new()?;
    fixture.record()?;
    fs::write(fixture.root.join("server.jar"), b"modified server")?;
    fixture
        .recorder()?
        .finish((), fixture.root.clone(), false, true)
        .await?;
    assert!(!fixture.root.join(".langame-clean-package.json").exists());
    assert_eq!(
        fs::read(fixture.root.join("server.jar"))?,
        b"modified server"
    );
    Ok(())
}

#[tokio::test]
async fn program_update_fresh_payload_keeps_program_hashes_after_retaining_operator_data()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = ProgramBaselineFixture::new()?;
    let recorder = fixture.recorder()?;
    recorder.prepare_fresh_payload(fixture.root.clone()).await?;
    fs::write(fixture.root.join("user.cfg"), b"operator settings")?;
    fs::write(fixture.root.join("custom-loader.dll"), b"operator loader")?;
    recorder
        .finish((), fixture.root.clone(), false, false)
        .await?;
    assert!(app_storage::library_program_is_pristine(
        &fixture.root,
        &fixture.descriptor,
        None
    )?);
    let manifest = fixture.clean_manifest()?;
    assert!(manifest["files"].get("server.jar").is_some());
    assert!(manifest["files"].get("user.cfg").is_none());
    assert!(manifest["files"].get("custom-loader.dll").is_none());
    assert!(!fixture.root.join(".langame-initial-package.json").exists());
    assert_eq!(
        fs::read(fixture.root.join("user.cfg"))?,
        b"operator settings"
    );
    Ok(())
}
