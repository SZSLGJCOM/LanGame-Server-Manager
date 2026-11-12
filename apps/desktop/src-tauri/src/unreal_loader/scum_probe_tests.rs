//! Explicit preparation of an owned SCUM contract fixture. Never starts a game.
#![cfg(test)]

use super::*;
use std::path::{Component, PathBuf};

#[tokio::test]
#[ignore = "requires a stopped, explicitly marked disposable SCUM program at LGSM_SCUM_CONTRACT_ROOT"]
async fn prepare_owned_scum_contract_loader() {
    let root = PathBuf::from(std::env::var_os("LGSM_SCUM_CONTRACT_ROOT").unwrap());
    assert!(root.is_absolute());
    assert!(
        !root
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    );
    assert_eq!(root.file_name().unwrap(), "program");
    let fixture_name = root
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(fixture_name.starts_with("vanilla-") || fixture_name.starts_with("contract-"));
    let _pins = pin(&root, false).unwrap();
    assert_eq!(
        read(&root.join(".langame-scum-contract-probe"), 128).unwrap(),
        b"isolated-scum-native-contract\n"
    );
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../modules/scum/reference-fixtures/2026-11-12-native_contract_build_26593814.json"
    ))
    .unwrap();
    let expected = evidence["package"]["executable_sha256"].as_str().unwrap();
    let profile = Profile::get("scum").unwrap();
    let executable = profile.win64(&root).join(profile.executable);
    let _binary_pins = pin(executable.parent().unwrap(), false).unwrap();
    assert_eq!(
        hash(&read(&executable, 320 * 1024 * 1024).unwrap()),
        expected
    );
    let prepared = prepare("scum").await.unwrap();
    let outcome = crate::unreal_loader::install(&root, "scum", &prepared).unwrap();
    assert!(installed(&root, "scum").unwrap());
    println!(
        "SCUM_NATIVE_CONTRACT loader={outcome:?} executable_sha256={expected} server_started=false save_invoked=false"
    );
}
