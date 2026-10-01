#![cfg(windows)]

use std::path::PathBuf;

use super::Game;
use super::probe_support::run_owned_empty_probe;

#[test]
#[ignore = "requires LGSM_DRAGONWILDS_PROBE_ROOT pointing to a marked isolated Dragonwilds installation with the verified player-query extension"]
fn isolated_dragonwilds_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process() {
    let root = PathBuf::from(std::env::var_os("LGSM_DRAGONWILDS_PROBE_ROOT").unwrap());
    assert!(root.is_absolute());
    assert!(root.join(".lgsm-isolated-player-probe").is_file());
    let config = std::fs::read_to_string(
        root.join("RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"),
    )
    .unwrap();
    let section = config
        .split_once("[/Script/Dominion.DedicatedServerSettings]")
        .expect("dedicated server settings section")
        .1;
    let section = section.split("\n[").next().unwrap();
    for key in ["OwnerId", "AdminPassword", "WorldPassword"] {
        let values: Vec<_> = section
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once('=')?;
                (name.trim() == key).then(|| value.trim().trim_matches('"'))
            })
            .collect();
        assert_eq!(
            values.len(),
            1,
            "one explicit isolation credential is required"
        );
        assert!(
            values[0].len() >= 16,
            "nonempty isolation credential is required"
        );
    }
    run_owned_empty_probe(
        Game::Dragonwilds,
        root,
        [
            "-Port=17993",
            "-QueryPort=27993",
            "-MULTIHOME=127.0.0.1",
            "-unattended",
            "-NoSplash",
            "-stdout",
            "-FullStdOutLogOutput",
        ]
        .map(str::to_owned)
        .to_vec(),
    );
}
