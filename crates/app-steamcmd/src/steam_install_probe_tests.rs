use crate::*;

// From the observed Workshop-only prefetch on PLAP: the old build remains
// installed while Steam advertises 50,548,454 bytes for a future update.
const PREFETCHED: &str = r#""AppState"
{
    "appid" "343050"
    "StateFlags" "6"
    "buildid" "26335402"
    "TargetBuildID" "26474880"
    "UpdateResult" "0"
    "BytesToDownload" "50548454"
    "BytesDownloaded" "0"
    "BytesToStage" "0"
    "BytesStaged" "0"
    "InstalledDepots"
    {
        "1004" { "manifest" "7604377918839582995" }
        "343051" { "manifest" "4099411213896431867" }
    }
    "SharedDepots" { "228982" "228980" "228988" "228980" }
}"#;
const DEPENDENCY: &str = r#""AppState"
{
    "appid" "228980"
    "StateFlags" "4"
    "InstalledDepots"
    {
        "228982" { "manifest" "6413394087650432851" }
        "228988" { "manifest" "6645201662696499616" }
    }
}"#;

struct Fixture {
    root: PathBuf,
    settings: AppSettings,
    install: InstallSpec,
    process: ProcessSpec,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        let mut fixture = Self {
            settings: AppSettings {
                archives_root: String::new(),
                servers_root: root.join("instances").to_string_lossy().into_owned(),
                games_root: root.join("library").to_string_lossy().into_owned(),
                modules_root: root.join("modules").to_string_lossy().into_owned(),
                steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
            },
            root,
            install: InstallSpec {
                shared_game_dir: "dontstarve".into(),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                verification_path: None,
                minecraft: None,
            },
            process: ProcessSpec {
                environment_template: Default::default(),
                executable: "bin64/dontstarve_dedicated_server_nullrenderer_x64.exe".into(),
                args_template: Vec::new(),
                working_directory_template: None,
                window_policy: app_core::ProcessWindowPolicy::Background,
                host_surface: app_core::ProcessHostSurface::ManagedTerminal,
                host_notes: None,
            },
        };
        fs::create_dir_all(fixture.runtime().join("bin64")).unwrap();
        fs::create_dir_all(fixture.runtime().join("steamapps")).unwrap();
        fs::write(
            fixture.runtime().join(&fixture.process.executable),
            b"installed server",
        )
        .unwrap();
        fs::write(
            fixture.runtime().join("steamapps/appmanifest_228980.acf"),
            DEPENDENCY,
        )
        .unwrap();
        fixture.manifest(PREFETCHED);
        fixture
    }

    fn runtime(&self) -> PathBuf {
        self.root.join("instances/dst/runtime")
    }

    fn manifest(&mut self, contents: &str) {
        fs::write(
            self.runtime().join("steamapps/appmanifest_343050.acf"),
            contents,
        )
        .unwrap();
    }

    fn probe(&self) -> ModuleInstallProbe {
        probe_module_install_state_with_override(
            &self.settings,
            "dontstarve",
            Some(343050),
            Some(&self.install),
            Some(&self.process),
            Some(&self.runtime().to_string_lossy()),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn workshop_prefetch_keeps_the_existing_instance_build_installed() {
    let fixture = Fixture::new();
    let probe = fixture.probe();
    assert_eq!(probe.install_state, InstallState::Installed);
    assert_eq!(probe.current_version.as_deref(), Some("26335402"));
    assert!(probe.executable_exists);
    let manifest = probe.steam_manifest.unwrap();
    assert_eq!(manifest.target_build_id.as_deref(), Some("26474880"));
    assert!(manifest.complete);
    assert!(!manifest.downloading_path_exists);
    // Status availability must not certify a newly acquired package as current.
    assert!(steam_depot::manifest::parse_acf(PREFETCHED.as_bytes(), 343050).is_err());
}

#[test]
fn prefetch_does_not_hide_downloads_staging_failures_or_missing_identity() {
    let mut fixture = Fixture::new();
    for (old, new) in [
        ("\"StateFlags\" \"6\"", "\"StateFlags\" \"2\""),
        ("\"StateFlags\" \"6\"", "\"StateFlags\" \"1030\""),
        ("\"BytesDownloaded\" \"0\"", "\"BytesDownloaded\" \"1\""),
        ("\"BytesToStage\" \"0\"", "\"BytesToStage\" \"100\""),
        ("\"BytesStaged\" \"0\"", "\"BytesStaged\" \"1\""),
        ("\"UpdateResult\" \"0\"", "\"UpdateResult\" \"8\""),
        ("\"buildid\" \"26335402\"", "\"buildid\" \"0\""),
        (
            "\"TargetBuildID\" \"26474880\"",
            "\"TargetBuildID\" \"26335402\"",
        ),
        ("\"InstalledDepots\"", "\"StagedDepots\""),
        ("\"manifest\" \"4099411213896431867\"", "\"manifest\" \"0\""),
    ] {
        fixture.manifest(&PREFETCHED.replace(old, new));
        assert_eq!(
            fixture.probe().install_state,
            InstallState::Incomplete,
            "{new}"
        );
    }
    fixture.manifest(&PREFETCHED.replacen(
        '{',
        "{\n\"StagedDepots\" { \"343051\" { \"manifest\" \"1\" } }",
        1,
    ));
    assert_eq!(fixture.probe().install_state, InstallState::Incomplete);
}

#[test]
fn prefetch_does_not_hide_pending_payload_or_missing_runtime_files() {
    let fixture = Fixture::new();
    let downloading = fixture.runtime().join("steamapps/downloading/343050");
    fs::create_dir_all(&downloading).unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Installed);
    fs::write(downloading.join("pending.bin"), b"pending").unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Incomplete);
    fs::remove_file(downloading.join("pending.bin")).unwrap();
    fs::remove_dir(&downloading).unwrap();
    fs::write(&downloading, b"unexpected entry").unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Incomplete);
    fs::remove_file(&downloading).unwrap();
    fs::remove_file(fixture.runtime().join(&fixture.process.executable)).unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Corrupted);
}

#[test]
fn prefetch_does_not_bypass_shared_dependency_completeness() {
    let fixture = Fixture::new();
    let dependency = fixture.runtime().join("steamapps/appmanifest_228980.acf");
    fs::write(
        &dependency,
        DEPENDENCY.replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"6\""),
    )
    .unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Incomplete);
    fs::remove_file(dependency).unwrap();
    assert_eq!(fixture.probe().install_state, InstallState::Incomplete);
}

#[test]
fn status_reads_top_level_acf_fields_independently_of_layout_and_case() {
    let mut fixture = Fixture::new();
    fixture.manifest(
        r#""APPSTATE" { "APPID" "343050" "NAME" "DST" "INSTALLDIR" "dontstarve" "STATEFLAGS" "4" "BUILDID" "17" "TARGETBUILDID" "18" "BYTESTODOWNLOAD" "10" "BYTESDOWNLOADED" "1" "UserConfig" { "StateFlags" "4" "BytesDownloaded" "10" "buildid" "99" } }"#,
    );
    let probe = fixture.probe();
    assert_eq!(probe.install_state, InstallState::Incomplete);
    let manifest = probe.steam_manifest.unwrap();
    assert_eq!(manifest.name.as_deref(), Some("DST"));
    assert_eq!(manifest.install_dir.as_deref(), Some("dontstarve"));
    assert_eq!(manifest.build_id.as_deref(), Some("17"));
    assert_eq!(manifest.target_build_id.as_deref(), Some("18"));
    assert_eq!(manifest.bytes_downloaded, Some(1));
    assert!(!manifest.complete);
}

#[test]
fn malformed_acf_or_progress_cannot_certify_an_existing_executable() {
    let mut fixture = Fixture::new();
    for contents in [
        r#""AppState" { "appid" "343050" "StateFlags" "4" "BytesDownloaded" "oops" }"#,
        r#""AppState" { "appid" "343050" "StateFlags" "4" "BytesToStage" { "value" "0" } }"#,
        r#""AppState" { "appid" "343050" }"#,
        r#""AppState" { "appid" "343050" "StateFlags" "oops" }"#,
        r#""AppState" { "appid" "343050" "StateFlags" "4" "StateFlags" "4" }"#,
        r#""AppState" { "appid" "7" "StateFlags" "4" }"#,
        r#""AppState" { "appid" "343050" "StateFlags" "4" "#,
    ] {
        fixture.manifest(contents);
        let probe = fixture.probe();
        assert!(probe.executable_exists);
        assert_eq!(probe.install_state, InstallState::Incomplete, "{contents}");
        assert!(!probe.steam_manifest.unwrap().complete);
    }
}
