use super::*;

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
        let root = std::env::temp_dir().join(format!("jx-{:x}-{stamp:x}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let mut deep = root.clone();
        while deep.as_os_str().encode_wide().count() < 310 {
            deep.push("java-".to_owned() + &"p".repeat(35));
        }
        fs::create_dir_all(&deep).unwrap();
        Self { root, deep }
    }

    async fn archive(&self, entries: &[&str]) -> PathBuf {
        let archive = self.deep.join("runtime.zip");
        let mut script = format!(
            "$ErrorActionPreference='Stop'\nAdd-Type -AssemblyName System.IO.Compression\nAdd-Type -TypeDefinition @'\n{}\n'@\n\
             $output=[LanGameInstallFiles]::CreateNew({}); try {{\n\
             $zip=[IO.Compression.ZipArchive]::new($output,[IO.Compression.ZipArchiveMode]::Create,$true); try {{\n",
            include_str!("install_files.cs"),
            ps_literal(&archive),
        );
        for name in entries {
            script.push_str(&format!(
                "$entry=$zip.CreateEntry({}); $stream=$entry.Open(); try {{ \
                 $bytes=[Text.Encoding]::UTF8.GetBytes('verified fixture payload'); \
                 $stream.Write($bytes,0,$bytes.Length) }} finally {{ $stream.Dispose() }}\n",
                ps_literal(Path::new(name)),
            ));
        }
        script.push_str("} finally { $zip.Dispose() } } finally { $output.Dispose() }");
        let output = run_powershell(&script, None, deadline()).await.unwrap();
        assert!(
            output.status.success(),
            "{}",
            output_excerpt(&output.stdout, &output.stderr)
        );
        archive
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn deadline() -> InstallDeadline {
    InstallDeadline::new("Java long-path extraction fixture", Duration::from_secs(45))
}

#[tokio::test]
async fn minecraft_java_extracts_beyond_legacy_windows_path_limits_without_overwriting_a_stage() {
    let fixture = Fixture::new();
    let archive = fixture
        .archive(&["jdk/bin/java.exe", "jdk/lib/security/default.policy"])
        .await;
    let stage = fixture.deep.join("stage");
    assert!(archive.as_os_str().encode_wide().count() > 260);
    assert!(stage.as_os_str().encode_wide().count() > 248);
    let script = minecraft_jre_extraction_script(&archive, &stage);
    assert!(
        script.encode_utf16().count() < 30_000,
        "extraction script must fit CreateProcess command line"
    );
    let output = run_powershell(&script, None, deadline()).await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    for relative in ["jdk/bin/java.exe", "jdk/lib/security/default.policy"] {
        assert_eq!(
            fs::read(stage.join(relative)).unwrap(),
            b"verified fixture payload"
        );
    }
    fs::write(stage.join("jdk/bin/java.exe"), b"retained existing stage").unwrap();
    let retry = run_powershell(&script, None, deadline()).await.unwrap();
    assert!(!retry.status.success());
    assert_eq!(
        fs::read(stage.join("jdk/bin/java.exe")).unwrap(),
        b"retained existing stage"
    );
}

#[tokio::test]
async fn minecraft_java_long_path_extraction_rejects_escape_before_writing_package_files() {
    let fixture = Fixture::new();
    let archive = fixture
        .archive(&["jdk/bin/java.exe", "../outside.txt"])
        .await;
    let outside = fixture.deep.join("outside.txt");
    fs::write(&outside, b"operator-owned bytes").unwrap();
    let stage = fixture.deep.join("stage");
    let output = run_powershell(
        &minecraft_jre_extraction_script(&archive, &stage),
        None,
        deadline(),
    )
    .await
    .unwrap();
    assert!(!output.status.success());
    assert!(!stage.join("jdk/bin/java.exe").exists());
    assert_eq!(fs::read(outside).unwrap(), b"operator-owned bytes");
}
