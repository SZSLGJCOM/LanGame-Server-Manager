#![cfg(windows)]
use super::Game;
use super::probe_support::run_owned_empty_probe;
use std::path::PathBuf;

#[test]
#[ignore = "requires LGSM_WINDROSE_PROBE_ROOT pointing to a marked isolated Windrose installation with the verified player-query extension"]
fn isolated_windrose_bootstrap_reads_two_current_empty_snapshots_and_rejects_stopped_process() {
    let root = PathBuf::from(std::env::var_os("LGSM_WINDROSE_PROBE_ROOT").unwrap());
    assert!(root.is_absolute());
    assert!(root.join(".lgsm-isolated-player-probe").is_file());
    assert!(root.join("langame_player_query").is_dir());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("R5/ServerDescription.json")).unwrap())
            .unwrap();
    let settings = &config["ServerDescription_Persistent"];
    assert_eq!(settings["P2pProxyAddress"], "127.0.0.1");
    assert_eq!(settings["DirectConnectionProxyAddress"], "127.0.0.1");
    assert_eq!(settings["IsPasswordProtected"], true);
    assert!(
        settings["Password"]
            .as_str()
            .is_some_and(|value| value.len() >= 16)
    );
    assert!(
        root.join("R5/Binaries/Win64/ue4ss/Mods/LgsmPlayerQuery/enabled.txt")
            .is_file()
    );
    run_owned_empty_probe(
        Game::Windrose,
        root,
        [
            "-unattended",
            "-NoSplash",
            "-stdout",
            "-FullStdOutLogOutput",
            "-MULTIHOME=127.0.0.1",
        ]
        .map(str::to_owned)
        .to_vec(),
    );
}
