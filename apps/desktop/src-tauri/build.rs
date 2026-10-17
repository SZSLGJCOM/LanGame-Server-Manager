fn main() {
    println!("cargo:rerun-if-env-changed=STATIC_VCRUNTIME");
    println!("cargo:rerun-if-changed=icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
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
