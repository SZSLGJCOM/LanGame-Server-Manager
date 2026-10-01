#![cfg(windows)]

use super::{Game, probe_support::run_owned_empty_probe};
use std::path::PathBuf;

#[test]
#[ignore = "requires LGSM_SCUM_PROBE_ROOT with a marked isolated SCUM server and the verified query loader"]
fn isolated_scum_reads_current_empty_snapshots_and_rejects_stopped_process() {
    let root = PathBuf::from(std::env::var_os("LGSM_SCUM_PROBE_ROOT").unwrap());
    assert!(root.is_absolute());
    let config =
        std::fs::read_to_string(root.join("SCUM/Saved/Config/WindowsServer/ServerSettings.ini"))
            .unwrap();
    let passwords: Vec<_> = config
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "scum.ServerPassword").then_some(value.trim())
        })
        .collect();
    assert_eq!(passwords.len(), 1);
    assert!(
        passwords[0].len() >= 16,
        "isolated server requires its private password"
    );
    run_owned_empty_probe(
        Game::Scum,
        root,
        [
            "-MULTIHOME=127.0.0.1",
            "-Port=35910",
            "-QueryPort=35912",
            "-unattended",
            "-NoCrashDialog",
        ]
        .map(str::to_owned)
        .to_vec(),
    );
}
