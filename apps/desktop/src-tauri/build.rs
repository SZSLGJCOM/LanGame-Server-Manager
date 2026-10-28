fn main() {
    println!("cargo:rerun-if-env-changed=STATIC_VCRUNTIME");
    println!("cargo:rerun-if-env-changed=LANGAME_ARK_ARCHIVE_DIR");
    println!("cargo:rerun-if-env-changed=LANGAME_PROXY_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=LANGAME_UE4SS_ARCHIVE");
    println!("cargo:rerun-if-changed=icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = project_root();
        println!(
            "cargo:rerun-if-changed={}",
            root.join("scripts/build_control_output.ps1").display()
        );
        // MSVC counts unnormalized SDK includes against MAX_PATH; retain the
        // short ARK output leaf in deeply nested release build directories.
        for (script, leaf, sources) in [
            ("build_ark_tools.ps1", "ark", &["modules/ark-tools"][..]),
            (
                "build_theforest_control.ps1",
                "theforest-control",
                &["modules/theforest/control"][..],
            ),
            (
                "build_enshrouded_control.ps1",
                "enshrouded-control",
                &["modules/enshrouded/control"][..],
            ),
            (
                "build_valheim_control.ps1",
                "valheim-control",
                &["modules/valheim/control"][..],
            ),
            (
                "build_dragonwilds_player_query.ps1",
                "dragonwilds-player-query",
                &[
                    "modules/runescapedragonwilds/extensions",
                    "modules/runescapedragonwilds/control",
                ][..],
            ),
            (
                "build_windrose_save_control.ps1",
                "windrose-save-control",
                &["modules/windrose/control"][..],
            ),
        ] {
            build_control(&root, script, leaf, sources);
        }
    }

    #[cfg(windows)]
    {
        let root = project_root();
        for (script, leaf) in [
            ("build_ue4ss_runtime.ps1", "ue4ss-runtime"),
            ("build_unreal_save_control.ps1", "native-world-save"),
            ("build_scum_save_control.ps1", "scum-save-control"),
            ("build_unreal_loader_proxies.ps1", "unreal-loader-proxies"),
        ] {
            build_control(&root, script, leaf, &[]);
        }
        for directory in [
            "modules/nightingale/control",
            "modules/conanexiles/control",
            "modules/scum/control",
            "apps/desktop/src-tauri/native",
        ] {
            println!("cargo:rerun-if-changed={}", root.join(directory).display());
        }
    }

    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    // Generate included assets before rustc starts so they do not invalidate the
    // finished compilation on the next Cargo invocation.
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(windows)
        .codegen(tauri_build::CodegenContext::new());

    tauri_build::try_build(attributes).expect("failed to run Tauri build script")
}

fn build_control(root: &std::path::Path, script: &str, leaf: &str, sources: &[&str]) {
    let script = root.join("scripts").join(script);
    println!("cargo:rerun-if-changed={}", script.display());
    for source in sources {
        println!("cargo:rerun-if-changed={}", root.join(source).display());
    }
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"))
            .join(leaf);
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-OutDir")
        .arg(output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let result = command
        .output()
        .unwrap_or_else(|error| panic!("failed to start {}: {error}", script.display()));
    if !result.status.success() {
        panic!(
            "{} failed:\n{}\n{}",
            script.display(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

fn project_root() -> std::path::PathBuf {
    // The same cached build executable can run for a snapshot or the live tree.
    // Resolve the source location supplied by this Cargo invocation at runtime.
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo CARGO_MANIFEST_DIR is missing"),
    )
    .join("../../..")
}
