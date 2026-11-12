use super::*;
use crate::{InstallDeadline, RETAINED_INSTALL_DATA_MARKER, output_excerpt, run_powershell};
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RECOVERY: &str = ".langame-archive-recovery-00000000-0000-0000-0000-000000000000";

struct Fixture {
    root: PathBuf,
    deep: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ip-{:x}-{stamp:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let mut deep = root.clone();
        while deep.as_os_str().encode_wide().count() < 310 {
            deep.push("install-".to_owned() + &"p".repeat(32));
        }
        fs::create_dir_all(&deep).unwrap();
        Self { root, deep }
    }

    fn paths(&self) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        (
            self.deep.join("program"),
            self.deep.join(".operation.stage"),
            self.deep.join(".operation.rollback"),
            self.deep.join(".operation.phase"),
        )
    }

    async fn archive(&self) -> PathBuf {
        let archive = self.deep.join("package.zip");
        let script = format!(
            "$ErrorActionPreference='Stop'\nAdd-Type -AssemblyName System.IO.Compression\n\
             Add-Type -TypeDefinition @'\n{}\n'@\n\
             $archiveOutput=[LanGameInstallFiles]::CreateNew({}); try {{\n\
             $zip=[IO.Compression.ZipArchive]::new($archiveOutput,[IO.Compression.ZipArchiveMode]::Create,$true); try {{\n\
             foreach ($name in @('bin/Server.exe','assets/nested/package.txt')) {{\n\
             $entry=$zip.CreateEntry($name); $entryOutput=$entry.Open(); try {{\
             $bytes=[Text.Encoding]::UTF8.GetBytes('official package bytes'); \
             $entryOutput.Write($bytes,0,$bytes.Length) }} finally {{ $entryOutput.Dispose() }}\n\
             }} }} finally {{ $zip.Dispose() }} }} finally {{ $archiveOutput.Dispose() }}",
            include_str!("install_files.cs"),
            ps_literal(&archive),
        );
        run_script(&script).await;
        archive
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this exclusively created direct child of managed test TEMP is
        // owned by the fixture, including any failed stage or rollback.
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new(
        "Long-path installation publication",
        Duration::from_secs(45),
    )
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

async fn run_script(script: &str) {
    assert!(
        script.encode_utf16().count() < 30_000,
        "publication script must fit CreateProcess command line"
    );
    let result = run_powershell(script, None, deadline()).await.unwrap();
    assert!(
        result.status.success(),
        "{}",
        output_excerpt(&result.stdout, &result.stderr)
    );
}

#[tokio::test]
async fn prepared_publication_preserves_deep_recovery_and_retained_data_beyond_max_path() {
    for preserve_retained_data in [false, true] {
        let fixture = Fixture::new();
        let (root, stage, rollback, phase) = fixture.paths();
        write(&root.join("retained/world.sentinel"), b"current world");
        write(&stage.join("bin/Server.exe"), b"new verified program");
        let recovery_source = if preserve_retained_data {
            &root
        } else {
            &stage
        };
        write(
            &recovery_source.join(RECOVERY).join("files/bin/damaged.dll"),
            b"exact original damaged bytes",
        );
        write(
            &recovery_source.join(RECOVERY).join("recovery.json"),
            b"exact recovery receipt",
        );
        if preserve_retained_data {
            write(&root.join(RETAINED_INSTALL_DATA_MARKER), b"retained");
        }
        let metadata = InstallPublicationMetadata {
            preserve_retained_data,
            ..Default::default()
        };
        let script = prepared_install_publish_script(
            &root,
            &stage,
            &rollback,
            &phase,
            &stage.join("bin/Server.exe"),
            &metadata,
        );
        run_script(&script).await;
        assert_eq!(
            fs::read(root.join("bin/Server.exe")).unwrap(),
            b"new verified program"
        );
        assert_eq!(
            fs::read(root.join(RECOVERY).join("files/bin/damaged.dll")).unwrap(),
            b"exact original damaged bytes"
        );
        assert_eq!(
            fs::read(root.join(RECOVERY).join("recovery.json")).unwrap(),
            b"exact recovery receipt"
        );
        assert_eq!(
            fs::read(rollback.join("retained/world.sentinel")).unwrap(),
            b"current world"
        );
        if preserve_retained_data {
            assert_eq!(
                fs::read(root.join("retained/world.sentinel")).unwrap(),
                b"current world"
            );
            assert_eq!(
                fs::read(rollback.join(RECOVERY).join("files/bin/damaged.dll")).unwrap(),
                b"exact original damaged bytes"
            );
        } else {
            assert!(!root.join("retained/world.sentinel").exists());
        }
        assert_eq!(fs::read(phase).unwrap(), b"rollback_pending");
        assert!(!stage.exists());
    }
}

#[tokio::test]
async fn archive_publication_extracts_and_atomically_replaces_beyond_max_path() {
    let fixture = Fixture::new();
    let (root, stage, rollback, phase) = fixture.paths();
    write(&root.join("previous.exe"), b"previous verified program");
    let archive = fixture.archive().await;
    let script = direct_download_publish_script(
        &root,
        &stage,
        &rollback,
        &phase,
        &stage.join("bin/Server.exe"),
        &archive,
        &Default::default(),
    );
    run_script(&script).await;
    for relative in ["bin/Server.exe", "assets/nested/package.txt"] {
        assert_eq!(
            fs::read(root.join(relative)).unwrap(),
            b"official package bytes"
        );
    }
    assert_eq!(
        fs::read(rollback.join("previous.exe")).unwrap(),
        b"previous verified program"
    );
    assert!(!root.join("previous.exe").exists());
    assert_eq!(fs::read(phase).unwrap(), b"rollback_pending");
    assert!(!stage.exists());
    assert!(!archive.exists());
}

#[tokio::test]
async fn prepared_publication_rejects_deep_junction_without_changing_target_or_original() {
    let fixture = Fixture::new();
    let (root, stage, rollback, phase) = fixture.paths();
    write(&root.join("previous.exe"), b"previous verified program");
    write(&stage.join("bin/Server.exe"), b"new verified program");
    let outside = fixture.root.join("outside");
    write(&outside.join("sentinel"), b"external data");
    // Create the junction at the short fixture root, then move that entry into
    // the deep stage. The setup must not depend on the legacy PS path provider.
    let junction = fixture.root.join("junction");
    let link = format!(
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path {} -Target {} | Out-Null",
        ps_literal(&junction),
        ps_literal(&outside),
    );
    run_script(&link).await;
    let deep_junction = stage.join("linked");
    fs::rename(&junction, &deep_junction).unwrap();
    let script = prepared_install_publish_script(
        &root,
        &stage,
        &rollback,
        &phase,
        &stage.join("bin/Server.exe"),
        &Default::default(),
    );
    let result = run_powershell(&script, None, deadline()).await.unwrap();
    assert!(!result.status.success());
    assert!(
        output_excerpt(&result.stdout, &result.stderr).contains("link or reparse point"),
        "{}",
        output_excerpt(&result.stdout, &result.stderr)
    );
    assert_eq!(
        fs::read(root.join("previous.exe")).unwrap(),
        b"previous verified program"
    );
    assert_eq!(
        fs::read(outside.join("sentinel")).unwrap(),
        b"external data"
    );
    assert!(!root.join("bin/Server.exe").exists());
    assert!(!rollback.exists());
    assert!(!phase.exists());
    // Remove just the junction entry before Drop, never its external target.
    fs::remove_dir(deep_junction).unwrap();
}

#[tokio::test]
async fn prepared_publication_rejects_deep_hardlink_before_retained_copy() {
    let fixture = Fixture::new();
    let (root, stage, rollback, phase) = fixture.paths();
    write(&root.join("previous.exe"), b"previous verified program");
    write(&root.join(RETAINED_INSTALL_DATA_MARKER), b"retained");
    write(&root.join("settings.ini"), b"retained user settings");
    write(&stage.join("bin/Server.exe"), b"new verified program");
    let outside = fixture.root.join("outside.sentinel");
    write(&outside, b"external data must never be overwritten");
    let stage_link = stage.join("settings.ini");
    fs::hard_link(&outside, &stage_link).unwrap();
    let metadata = InstallPublicationMetadata {
        preserve_retained_data: true,
        ..Default::default()
    };
    let script = prepared_install_publish_script(
        &root,
        &stage,
        &rollback,
        &phase,
        &stage.join("bin/Server.exe"),
        &metadata,
    );
    let result = run_powershell(&script, None, deadline()).await.unwrap();
    assert!(!result.status.success());
    assert!(
        output_excerpt(&result.stdout, &result.stderr).contains("link or reparse point"),
        "{}",
        output_excerpt(&result.stdout, &result.stderr)
    );
    assert_eq!(
        fs::read(root.join("previous.exe")).unwrap(),
        b"previous verified program"
    );
    assert_eq!(
        fs::read(root.join("settings.ini")).unwrap(),
        b"retained user settings"
    );
    assert_eq!(
        fs::read(&outside).unwrap(),
        b"external data must never be overwritten"
    );
    assert_eq!(
        fs::read(&stage_link).unwrap(),
        b"external data must never be overwritten"
    );
    assert!(!root.join("bin/Server.exe").exists());
    assert!(!rollback.exists());
    assert!(!phase.exists());
    // Remove only this fixture's directory entry, leaving its target intact.
    fs::remove_file(stage_link).unwrap();
}
