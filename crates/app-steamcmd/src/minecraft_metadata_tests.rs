use super::*;
use crate::http_download::fetch_text_from_candidates;
use crate::{
    AppSettings, InstallSource, InstallSpec, InstallState, MinecraftJavaInstallSpec,
    MinecraftVersionManifest, ModuleDetails, ProcessSpec, install_or_update_minecraft_java_module,
    minecraft_metadata_path,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const VERSION: &str = "1.21.1";
const DETAILS: &str = concat!(
    "{\r\n  \"id\": \"1.21.1\",\r\n",
    "  \"downloads\": {\"server\": {\"sha1\": \"0123456789abcdef0123456789abcdef01234567\",",
    " \"size\": 7, \"url\": \"https://example.invalid/server.jar\"}},\r\n",
    "  \"javaVersion\": {\"majorVersion\": 21}\r\n}\r\n"
);

struct Source {
    url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Source {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn source(body: &str) -> Source {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/metadata.json", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::Relaxed);
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    Source {
        url,
        requests,
        task,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("Minecraft metadata fixture", Duration::from_secs(5))
}

fn checksum(text: &str) -> String {
    Sha1::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn entry(url: &str, body: &str) -> MinecraftVersionManifestEntry {
    MinecraftVersionManifestEntry {
        id: VERSION.into(),
        version_type: "release".into(),
        url: url.into(),
        sha1: checksum(body),
    }
}

fn manifest(url: &str, sha1: &str) -> String {
    serde_json::json!({
        "latest": {"release": VERSION, "snapshot": VERSION},
        "versions": [{"id": VERSION, "type": "release", "url": url, "sha1": sha1}]
    })
    .to_string()
}

#[test]
fn manifest_requires_a_complete_sha1_for_every_version() {
    let valid = manifest("https://example.invalid/version.json", &checksum(DETAILS));
    let mut missing: serde_json::Value = serde_json::from_str(&valid).unwrap();
    missing["versions"][0]
        .as_object_mut()
        .unwrap()
        .remove("sha1");
    assert!(serde_json::from_value::<MinecraftVersionManifest>(missing).is_err());
    for digest in ["", "1234", "gggggggggggggggggggggggggggggggggggggggg"] {
        assert!(
            serde_json::from_str::<MinecraftVersionManifest>(&manifest(
                "https://example.invalid/version.json",
                digest
            ))
            .is_err()
        );
    }
    assert!(serde_json::from_str::<MinecraftVersionManifest>(&valid).is_ok());
}

#[tokio::test]
async fn matching_raw_metadata_hash_and_id_are_accepted() {
    let source = source(DETAILS).await;
    let mut selected = entry(&source.url, DETAILS);
    selected.sha1.make_ascii_uppercase();
    let details = fetch_minecraft_version_details(&client(), &selected, deadline())
        .await
        .unwrap();
    assert_eq!(details.id, VERSION);
    assert_eq!(details.java_version.major_version, 21);
    assert_eq!(details.downloads.server.unwrap().size, 7);
    assert_eq!(source.requests.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn reserialized_equivalent_json_fails_the_raw_body_hash() {
    let normalized = serde_json::from_str::<serde_json::Value>(DETAILS)
        .unwrap()
        .to_string();
    let source = source(&normalized).await;
    let selected = entry(&source.url, DETAILS);
    let error = fetch_minecraft_version_details(&client(), &selected, deadline())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("SHA1 mismatch"));
    assert_eq!(source.requests.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn valid_hash_cannot_authorize_a_different_version_id() {
    let body = DETAILS.replace(VERSION, "1.21.2");
    let source = source(&body).await;
    let selected = entry(&source.url, &body);
    let error = fetch_minecraft_version_details(&client(), &selected, deadline())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not match requested version")
    );
    assert_eq!(source.requests.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn every_fallback_candidate_must_match_the_selected_digest() {
    let wrong_body = DETAILS.replace(VERSION, "1.21.2");
    for fallback_body in [wrong_body.as_str(), DETAILS] {
        let primary = source(&wrong_body).await;
        let alternate = source(fallback_body).await;
        let selected = entry(&primary.url, DETAILS);
        let result = fetch_text_from_candidates(
            &client(),
            &[primary.url.clone(), alternate.url.clone()],
            deadline(),
            4 * 1024 * 1024,
            |text| parse_version_details(text, &selected),
        )
        .await;
        if fallback_body == DETAILS {
            let (body, details) = result.unwrap();
            assert_eq!(body, DETAILS);
            assert_eq!(details.id, VERSION);
        } else {
            assert!(result.unwrap_err().to_string().contains("SHA1 mismatch"));
        }
        assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
        assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn fallback_does_not_accept_matching_hash_with_wrong_version_id() {
    let body = DETAILS.replace(VERSION, "1.21.2");
    let primary = source(&body).await;
    let alternate = source(&body).await;
    let selected = entry(&primary.url, &body);
    let error = fetch_text_from_candidates(
        &client(),
        &[primary.url.clone(), alternate.url.clone()],
        deadline(),
        4 * 1024 * 1024,
        |text| parse_version_details(text, &selected),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not match requested version")
    );
    assert_eq!(primary.requests.load(Ordering::Relaxed), 1);
    assert_eq!(alternate.requests.load(Ordering::Relaxed), 1);
}

struct InstallFixture {
    root: std::path::PathBuf,
    settings: AppSettings,
    module: ModuleDetails,
}

impl InstallFixture {
    fn new(manifest_url: &str) -> Self {
        let root = crate::tests::unique_test_root();
        let settings = AppSettings {
            games_root: root.join("games").to_string_lossy().into_owned(),
            servers_root: root.join("servers").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let module = ModuleDetails {
            summary: app_core::ModuleSummary {
                id: "minecraft".into(),
                name: "Minecraft fixture".into(),
                version: "1".into(),
                description: None,
                steam_app_id: None,
                install_state: InstallState::Installed,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec!["windows".into()],
            },
            schema_json: None,
            default_ports: vec![],
            install: Some(InstallSpec {
                shared_game_dir: "minecraft".into(),
                source: Some(InstallSource::MinecraftJava),
                verification_path: Some("server.jar".into()),
                download_url_windows: None,
                download_integrity_windows: None,
                minecraft: Some(MinecraftJavaInstallSpec {
                    version: VERSION.into(),
                    manifest_url: Some(manifest_url.into()),
                    server_jar: "server.jar".into(),
                    java_policy: "mojang_version_metadata".into(),
                    default_distribution: "vanilla".into(),
                    distributions: vec![],
                }),
            }),
            process: Some(ProcessSpec {
                executable: "jre/bin/java.exe".into(),
                args_template: vec![],
                environment_template: Default::default(),
                working_directory_template: None,
                window_policy: Default::default(),
                host_surface: Default::default(),
                host_notes: None,
            }),
            workshop: None,
            mods: None,
            runtime: Default::default(),
        };
        Self {
            root,
            settings,
            module,
        }
    }
}

impl Drop for InstallFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn current_program_check_verifies_remote_release_without_changing_shared_files() {
    let jar = b"official fixture jar";
    let jar_hash = checksum(std::str::from_utf8(jar).unwrap());
    let details_body = DETAILS.replace("0123456789abcdef0123456789abcdef01234567", &jar_hash);
    let details = source(&details_body).await;
    let manifest_source = source(&manifest(&details.url, &checksum(&details_body))).await;
    let fixture = InstallFixture::new(&manifest_source.url);
    let root = fixture.root.join("games/minecraft");
    std::fs::create_dir_all(root.join("jre/bin")).unwrap();
    std::fs::create_dir_all(root.join(".langame")).unwrap();
    std::fs::write(root.join("jre/bin/java.exe"), b"fixture java").unwrap();
    std::fs::write(root.join("server.jar"), jar).unwrap();
    let metadata = serde_json::json!({
        "version_id": VERSION, "version_type": "release", "manifest_url": manifest_source.url,
        "version_url": details.url, "server_url": "https://example.invalid/server.jar",
        "server_sha1": jar_hash, "server_size": jar.len(), "server_jar": "server.jar",
        "required_java_major": 21, "downloaded_at_unix_ms": 1
    })
    .to_string();
    std::fs::write(minecraft_metadata_path(&root), &metadata).unwrap();
    std::fs::write(
        root.join(".langame-clean-package.json"),
        b"retained inventory",
    )
    .unwrap();
    let guard = crate::acquire_game_install_lifecycle("minecraft", std::slice::from_ref(&root))
        .await
        .unwrap();
    let token = crate::InstallCancellation::new();
    for expected in [Some(VERSION.to_owned()), None] {
        assert_eq!(
            crate::program_update_check::current_program_version_with_java_probe(
                &fixture.settings,
                &fixture.module,
                &root,
                &guard,
                &token,
                |_, _| async { Ok(Some(21)) }
            )
            .await
            .unwrap(),
            expected
        );
        if expected.is_some() {
            std::fs::write(root.join("server.jar"), b"modified jar").unwrap();
        }
    }
    assert_eq!(
        std::fs::read_to_string(minecraft_metadata_path(&root)).unwrap(),
        metadata
    );
    assert_eq!(
        std::fs::read(root.join("server.jar")).unwrap(),
        b"modified jar"
    );
    assert_eq!(
        std::fs::read(root.join(".langame-clean-package.json")).unwrap(),
        b"retained inventory"
    );
    assert_eq!(
        crate::read_program_install_revision(
            std::path::Path::new(&fixture.settings.servers_root),
            "minecraft",
            &root
        )
        .unwrap(),
        0
    );
    assert_eq!(
        manifest_source.requests.load(Ordering::Relaxed),
        2,
        "every attempt checks the source"
    );
    std::fs::write(root.join("server.jar"), jar).unwrap();
    assert_eq!(
        crate::current_program_version(&fixture.settings, &fixture.module, &root, &guard, &token)
            .await
            .unwrap(),
        None,
        "a damaged Java executable is not current even with a verified jar"
    );
    assert_eq!(
        std::fs::read(root.join("jre/bin/java.exe")).unwrap(),
        b"fixture java"
    );
    token.cancel();
    assert!(matches!(
        crate::current_program_version(&fixture.settings, &fixture.module, &root, &guard, &token)
            .await,
        Err(crate::SteamCmdError::InstallCancelled { .. })
    ));
    assert_eq!(manifest_source.requests.load(Ordering::Relaxed), 3);

    let servers_root = std::path::Path::new(&fixture.settings.servers_root);
    let revision_key = crate::package_revision::program_revision_key("minecraft", &root).unwrap();
    crate::begin_game_install_revision(servers_root, &revision_key).unwrap();
    assert_eq!(
        crate::program_update_check::current_program_version_with_java_probe(
            &fixture.settings,
            &fixture.module,
            &root,
            &guard,
            &crate::InstallCancellation::new(),
            |_, _| async { panic!("pending updates must enter installer recovery") },
        )
        .await
        .unwrap(),
        None,
    );
    assert!(matches!(
        crate::read_program_install_revision(servers_root, "minecraft", &root),
        Err(crate::SteamCmdError::PackageRevisionPending { .. }),
    ));
    assert_eq!(manifest_source.requests.load(Ordering::Relaxed), 3);
    assert_eq!(std::fs::read(root.join("server.jar")).unwrap(), jar);
}

#[tokio::test]
async fn current_program_check_rejects_untrusted_remote_metadata_before_touching_local_files() {
    let details_source = source(DETAILS).await;
    let manifest_source = source(&manifest(&details_source.url, &checksum("tampered"))).await;
    let fixture = InstallFixture::new(&manifest_source.url);
    let root = fixture.root.join("games/minecraft");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("server.jar"), b"retained jar").unwrap();
    let guard = crate::acquire_game_install_lifecycle("minecraft", std::slice::from_ref(&root))
        .await
        .unwrap();
    let error = crate::current_program_version(
        &fixture.settings,
        &fixture.module,
        &root,
        &guard,
        &crate::InstallCancellation::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("SHA1 mismatch"), "{error}");
    assert_eq!(
        std::fs::read(root.join("server.jar")).unwrap(),
        b"retained jar"
    );
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[tokio::test]
async fn rejected_metadata_never_prepares_java_or_publishes_installation_files() {
    let wrong_id = DETAILS.replace(VERSION, "1.21.2");
    for (body, expected_body, message) in [
        (wrong_id.as_str(), DETAILS, "SHA1 mismatch"),
        (
            wrong_id.as_str(),
            wrong_id.as_str(),
            "does not match requested version",
        ),
    ] {
        let details = source(body).await;
        let manifest_source = source(&manifest(&details.url, &checksum(expected_body))).await;
        let fixture = InstallFixture::new(&manifest_source.url);
        let install_root = fixture.root.join("games/minecraft");
        let metadata_path = minecraft_metadata_path(&install_root);
        std::fs::create_dir_all(metadata_path.parent().unwrap()).unwrap();
        std::fs::write(&metadata_path, b"previous metadata").unwrap();
        std::fs::write(install_root.join("server.jar"), b"previous jar").unwrap();
        let error = install_or_update_minecraft_java_module(
            &fixture.settings,
            &fixture.module,
            fixture.module.install.as_ref().unwrap(),
            fixture.module.process.as_ref().unwrap(),
            "update",
            deadline(),
            &mut |update| {
                // Stop a regression before it can request an external JRE.
                assert!(!update.detail.starts_with("Preparing Temurin"));
                assert_eq!(
                    update.install_progress.unwrap().phase,
                    app_core::InstallPhase::Preparing
                );
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert_eq!(std::fs::read(&metadata_path).unwrap(), b"previous metadata");
        assert_eq!(
            std::fs::read(install_root.join("server.jar")).unwrap(),
            b"previous jar"
        );
        assert_eq!(std::fs::read_dir(&install_root).unwrap().count(), 2);
        assert!(!install_root.join("jre").exists());
        assert_eq!(manifest_source.requests.load(Ordering::Relaxed), 1);
        assert_eq!(details.requests.load(Ordering::Relaxed), 1);
    }
}
