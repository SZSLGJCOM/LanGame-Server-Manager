#![cfg(test)]

use super::*;

#[test]
fn frm_loading_evidence_requires_initialized_exact_native_versions_and_real_endpoint_registration()
{
    let lines = [
        "[2026.10.27-00.00.01]LogSatisfactoryModLoader: Display: Satisfactory Mod Loader v.3.12.0+827 pre-initializing...",
        "LogSatisfactoryModLoader: Display: Initialization finished!",
        "LogSatisfactoryModLoader: Display: SML: 3.12.0",
        "LogSatisfactoryModLoader: Display: FicsitRemoteMonitoring: 1.5.4",
        "LogHttpServer: Registered API Endpoint: getPlayer - Current number of endpoints registered: 42",
    ];
    let mut complete = LoadEvidence::default();
    for line in lines {
        complete.observe(line);
    }
    assert!(complete.complete());
    for missing in 0..lines.len() {
        let mut incomplete = LoadEvidence::default();
        for (_, line) in lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != missing)
        {
            incomplete.observe(line);
        }
        assert!(!incomplete.complete());
    }
    let mut wrong = LoadEvidence::default();
    for line in lines {
        wrong.observe(&line.replace("1.5.4", "1.5.3"));
    }
    assert!(!wrong.complete());
    let mut files_only = LoadEvidence::default();
    for line in [
        "SML.uplugin version=3.12.0",
        "FicsitRemoteMonitoring.uplugin version=1.5.4",
        "Installed /api/getPlayer",
    ] {
        files_only.observe(line);
    }
    assert!(!files_only.complete());
    assert!(!version("3.12.01", "3.12.0"));
    assert!(!version("1.5.40", "1.5.4"));
}

#[test]
fn frm_zero_players_requires_successful_array_response_and_complete_online_roster() {
    assert!(require_empty_array(b"[]").is_ok());
    assert!(require_empty_array(b" \n[]\r\n").is_ok());
    for invalid in [
        b"null".as_slice(),
        b"{}",
        b"{\"error\":\"unavailable\"}",
        b"[{}]",
        b"",
        b"<html>not found</html>",
    ] {
        assert!(require_empty_array(invalid).is_err());
    }
    let mut snapshot: RuntimeLivePlayerSnapshot = serde_json::from_value(serde_json::json!({
        "snapshot_id":"native-fragment", "instance_id":"native-fixture", "status":"ready", "source":"http_api",
        "observed_at_unix_ms":123, "expires_at_unix_ms":null, "complete":true, "truncated":false,
        "stale":false, "current_players":0, "max_players":null, "entries":[], "issue":null
    })).unwrap();
    assert!(empty_roster(&snapshot).is_ok());
    snapshot.status = RuntimeLivePlayerStatus::Failed;
    assert!(empty_roster(&snapshot).is_err());
    snapshot.status = RuntimeLivePlayerStatus::Ready;
    snapshot.current_players = None;
    assert!(empty_roster(&snapshot).is_err());
    snapshot.current_players = Some(0);
    snapshot.complete = false;
    assert!(empty_roster(&snapshot).is_err());
    snapshot.complete = true;
    snapshot.stale = true;
    assert!(empty_roster(&snapshot).is_err());
}

#[test]
fn frm_fixture_archive_members_cannot_escape_or_select_windows_alternate_streams() {
    assert_eq!(
        staging::member_path("Binaries/Win64/server.dll").unwrap(),
        PathBuf::from("Binaries/Win64/server.dll")
    );
    for invalid in [
        "",
        "../outside.dll",
        "/absolute.dll",
        "C:/outside.dll",
        "Binaries\\outside.dll",
        "Binaries/server.dll:payload",
    ] {
        assert!(staging::member_path(invalid).is_err());
    }
}

#[test]
fn frm_fixture_seeds_native_maps_once_without_replacing_owned_game_options() {
    let existing = "[/Script/FactoryGame.FGGameUserSettings]\nmIntValues=((\"FG.DSAutoPause\",1))\n[Other]\nKeep=original\n";
    let seeded = staging::settings(existing, WEB_PORT).unwrap();
    assert!(seeded.contains("mIntValues=((\"FG.DSAutoPause\",1),(\"FicsitRemoteMonitoring.Server.uWS.Autostart\",1),(\"FicsitRemoteMonitoring.Server.uWS.Port\",18080))"));
    assert!(seeded.contains("mStringValues=((\"FicsitRemoteMonitoring.Server.uWS.AuthenticationToken\",\"synthetic-test-only\"))\n[Other]\nKeep=original"));
    assert!(staging::settings(&seeded, WEB_PORT).is_err());
    for invalid in [
        "[/Script/FactoryGame.FGGameUserSettings]\n[/Script/FactoryGame.FGGameUserSettings]\n",
        "[/Script/FactoryGame.FGGameUserSettings]\nmIntValues=()\nmIntValues=()\n",
        "[/Script/FactoryGame.FGGameUserSettings]\n+mIntValues=()\n",
        "[/Script/FactoryGame.FGGameUserSettings]\nmIntValues=not-a-map\n",
    ] {
        assert!(staging::settings(invalid, WEB_PORT).is_err());
    }
    assert!(staging::settings(&"x".repeat(256 * 1024 + 1), WEB_PORT).is_err());
}
