use super::*;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_fixture(source: &Path, target: &Path, remaining: &mut u64) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        if metadata.is_symlink() {
            return Err(std::io::Error::other(
                "The test source must not contain links.",
            ));
        }
        let destination = target.join(entry.file_name());
        if metadata.is_dir() {
            copy_fixture(&entry.path(), &destination, remaining)?;
        } else if metadata.is_file() {
            let size = entry.metadata()?.len();
            *remaining = remaining
                .checked_sub(size)
                .ok_or_else(|| std::io::Error::other("Runtime test fixture exceeds 1 GiB."))?;
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

/// Opt-in integration evidence: consumes an installed host, never downloads or
/// changes it. Normal unit tests must also work on machines without .NET.
#[tokio::test]
#[ignore = "requires an installed Windows x64 .NET runtime; copies one runtime into owned temporary data"]
async fn installed_runtime_loads_managed_probe_and_corrupt_coreclr_is_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Fixture(crate::tests::unique_test_root());
    fs::create_dir_all(&root.0)?;
    let deadline = InstallDeadline::new("local CLR probe integration", Duration::from_secs(120));
    let source = existing_roots(&root.0, &root.0)
        .into_iter()
        .find(|path| path.join("dotnet.exe").is_file())
        .ok_or("No installed dotnet.exe available for the integration test")?;
    let mut list = Command::new(source.join("dotnet.exe"));
    list.arg("--list-runtimes");
    let output = run_command_capture(list, deadline).await?;
    assert!(output.status.success());
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            if words.next() != Some("Microsoft.NETCore.App") {
                return None;
            }
            words
                .next()
                .and_then(stable_version)
                .filter(|version| version[0] >= 3)
        })
        .max()
        .ok_or("No stable installed Microsoft.NETCore.App runtime available")?;
    let host_version = fs::read_dir(source.join("host/fxr"))?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().and_then(stable_version))
        .max()
        .ok_or("No stable installed hostfxr available")?;
    let fixture = root.0.join("isolated-dotnet");
    fs::create_dir_all(&fixture)?;
    fs::copy(source.join("dotnet.exe"), fixture.join("dotnet.exe"))?;
    let mut remaining = 1024 * 1024 * 1024;
    let runtime_relative =
        PathBuf::from("shared/Microsoft.NETCore.App").join(display_version(version));
    let host_relative = PathBuf::from("host/fxr").join(display_version(host_version));
    copy_fixture(
        &source.join(&runtime_relative),
        &fixture.join(&runtime_relative),
        &mut remaining,
    )?;
    copy_fixture(
        &source.join(&host_relative),
        &fixture.join(&host_relative),
        &mut remaining,
    )?;
    let owned = root.0.join("owned-probe-cache");
    let _guard = acquire_runtime_root(
        crate::test_install_lock_context(),
        &owned,
        "clr-probe-integration",
        deadline,
    )
    .await?;
    let required = Requirement {
        version,
        exact: true,
        prefer_lowest: false,
    };
    assert!(
        valid_runtime(&fixture, required, &owned, deadline).await?,
        "a healthy copied runtime must execute the owned managed payload"
    );
    fs::write(
        fixture.join(&runtime_relative).join("coreclr.dll"),
        b"nonempty corrupt coreclr fixture",
    )?;
    let mut list = Command::new(fixture.join("dotnet.exe"));
    list.arg("--list-runtimes");
    assert!(
        run_command_capture(list, deadline).await?.status.success(),
        "enumeration alone must still accept this broken runtime, proving why the managed probe is needed"
    );
    assert!(
        !valid_runtime(&fixture, required, &owned, deadline).await?,
        "nonempty corrupted coreclr must fail the actual managed probe"
    );
    Ok(())
}
