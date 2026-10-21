fn main() {
    println!("cargo:rerun-if-env-changed=STATIC_VCRUNTIME");
    println!("cargo:rerun-if-env-changed=LANGAME_ARK_ARCHIVE_DIR");
    println!("cargo:rerun-if-env-changed=LANGAME_PROXY_SOURCE_DIR");
    println!("cargo:rerun-if-changed=icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = project_root();
        println!(
            "cargo:rerun-if-changed={}",
            root.join("scripts/build_control_output.ps1").display()
        );
        let script = root.join("scripts/build_ark_tools.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/ark-tools").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("ark-tools");
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
            .expect("failed to start the ARK extension builder");
        if !result.status.success() {
            panic!(
                "ARK extension build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let script = root.join("scripts/build_theforest_control.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/theforest/control").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("theforest-control");
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
            .arg(script)
            .arg("-OutDir")
            .arg(output);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command
            .output()
            .expect("failed to start The Forest control builder");
        if !result.status.success() {
            panic!(
                "The Forest control build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let script = root.join("scripts/build_enshrouded_control.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/enshrouded/control").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("enshrouded-control");
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
            .arg(script)
            .arg("-OutDir")
            .arg(output);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command
            .output()
            .expect("failed to start the Enshrouded control builder");
        if !result.status.success() {
            panic!(
                "Enshrouded control build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let script = root.join("scripts/build_valheim_control.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/valheim/control").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("valheim-control");
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
            .arg(script)
            .arg("-OutDir")
            .arg(output);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command
            .output()
            .expect("failed to start the Valheim control builder");
        if !result.status.success() {
            panic!(
                "Valheim control build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let script = root.join("scripts/build_dragonwilds_player_query.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/runescapedragonwilds/extensions")
                .display()
        );
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/runescapedragonwilds/control").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("dragonwilds-player-query");
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
            .arg(script)
            .arg("-OutDir")
            .arg(output);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command
            .output()
            .expect("failed to start the Dragonwilds player query builder");
        if !result.status.success() {
            panic!(
                "Dragonwilds player query build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let script = root.join("scripts/build_windrose_save_control.ps1");
        println!("cargo:rerun-if-changed={}", script.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/windrose/control").display()
        );
        let output = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
        )
        .join("windrose-save-control");
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
            .arg(script)
            .arg("-OutDir")
            .arg(output);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let result = command
            .output()
            .expect("failed to start the Windrose save control builder");
        if !result.status.success() {
            panic!(
                "Windrose save control build failed:\n{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }

    #[cfg(windows)]
    {
        let root = project_root();
        println!(
            "cargo:rerun-if-changed={}",
            root.join("modules/returntomoria/control").display()
        );
        for (script, leaf) in [
            ("build_unreal_save_control.ps1", "native-world-save"),
            ("build_scum_save_control.ps1", "scum-save-control"),
            ("build_unreal_loader_proxies.ps1", "unreal-loader-proxies"),
            ("build_vrising_save_control.ps1", "vrising-save-control"),
        ] {
            let source = root.join("scripts").join(script);
            println!("cargo:rerun-if-changed={}", source.display());
            let output = std::path::PathBuf::from(
                std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR is missing"),
            )
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
                .arg(source)
                .arg("-OutDir")
                .arg(output);
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
            let result = command
                .output()
                .expect("failed to start native persistence adapter builder");
            if !result.status.success() {
                panic!(
                    "Native persistence adapter build failed:\n{}\n{}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
        for directory in [
            "modules/nightingale/control",
            "modules/conanexiles/control",
            "modules/scum/control",
            "modules/vrising/control",
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

fn project_root() -> std::path::PathBuf {
    // The same cached build executable can run for a snapshot or the live tree.
    // Resolve the source location supplied by this Cargo invocation at runtime.
    std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo CARGO_MANIFEST_DIR is missing"),
    )
    .join("../../..")
}
