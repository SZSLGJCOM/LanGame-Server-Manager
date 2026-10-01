use super::response::parse;
use super::transport::*;
use super::*;
use app_core::RuntimePlayerIdentityKind;
use std::fs::{self, OpenOptions};
use std::time::Instant;

const NONCE: &str = "1234567890abcdef1234567890abcdef";

#[test]
fn recorded_empty_and_constructed_populated_contract_fixtures_parse() {
    for body in [
        include_bytes!("../../test-data/live-players/windrose/empty.json").as_slice(),
        include_bytes!("../../test-data/live-players/windrose/normal.json").as_slice(),
    ] {
        let value: serde_json::Value = serde_json::from_slice(body).unwrap();
        let snapshot = parse(
            Game::Windrose,
            "instance",
            value["request_id"].as_str().unwrap(),
            value["timestamp"].as_u64().unwrap() * 1000,
            body,
        )
        .unwrap();
        assert_eq!(
            snapshot.public_snapshot.entries.len(),
            value["current_players"].as_u64().unwrap() as usize
        );
        assert!(snapshot.public_snapshot.complete);
    }
}

fn response(players: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"protocol": 1, "request_id": NONCE,
        "boot_id": "abcdef1234567890abcdef1234567890", "timestamp": 100,
        "complete": true, "source": "net_driver_client_connections",
        "current_players": players.as_array().unwrap().len(), "max_players": 8,
        "players": players})
}

fn parsed(value: &serde_json::Value) -> LivePlayerCollectionResult {
    parse(
        Game::Windrose,
        "instance",
        NONCE,
        100_000,
        &serde_json::to_vec(value).unwrap(),
    )
}

#[test]
fn current_complete_connections_keep_names_and_session_identity_read_only() {
    let value = response(serde_json::json!([
        {"name": "玩家一 <script>", "session_id": "1"},
        {"name": "玩家一 <script>", "session_id": "2"}
    ]));
    let result = parsed(&value).unwrap();
    assert_eq!(result.public_snapshot.current_players, Some(2));
    assert!(result.public_snapshot.complete);
    assert!(result.private_action_bindings.is_empty());
    let entries = result.public_snapshot.entries;
    assert_eq!(entries[0].display_name, "玩家一 <script>");
    assert!(
        entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty()
                && entry.identifiers[0].kind == RuntimePlayerIdentityKind::SessionId
                && !entry.identifiers[0].stable)
    );
    assert_ne!(entries[0].player_key, entries[1].player_key);
    assert!(
        parsed(&response(serde_json::json!([])))
            .unwrap()
            .public_snapshot
            .entries
            .is_empty()
    );
}

#[test]
fn cached_partial_fallback_or_mismatched_snapshot_cannot_prove_an_empty_server() {
    for (key, value) in [
        ("protocol", serde_json::json!(2)),
        ("request_id", serde_json::json!("old")),
        ("boot_id", serde_json::json!("")),
        ("timestamp", serde_json::json!(99)),
        (
            "boot_id",
            serde_json::json!("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"),
        ),
        ("timestamp", serde_json::json!(107)),
        ("complete", serde_json::json!(false)),
        ("error", serde_json::json!("enumeration_failed")),
        ("source", serde_json::json!("r5_character_fallback")),
        ("current_players", serde_json::json!(1)),
        ("current_players", serde_json::json!(-1)),
        ("max_players", serde_json::json!(257)),
        ("degraded", serde_json::json!(true)),
    ] {
        let mut invalid = response(serde_json::json!([]));
        invalid[key] = value;
        let failure = parsed(&invalid).unwrap_err();
        assert!(!failure.complete);
        assert_eq!(
            failure.issue.unwrap().code,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        );
    }
}

#[test]
fn missing_names_or_duplicate_session_ids_reject_the_whole_list() {
    for players in [
        serde_json::json!([{"name": " ", "session_id": "1"}]),
        serde_json::json!([{"name": "a\nb", "session_id": "1"}]),
        serde_json::json!([{"name": "Alice", "session_id": "not-an-id"}]),
        serde_json::json!([{"name": "Alice", "session_id": "player:1"}]),
        serde_json::json!([{"name": "Alice", "session_id": 1}]),
        serde_json::json!([{"name": "Alice", "session_id": "1"}, {"name": "Bob", "session_id": "1"}]),
    ] {
        assert!(parsed(&response(players)).is_err());
    }
}

#[test]
fn root_resolution_requires_the_verified_game_image_layout() {
    for image in [
        "C:/games/windrose/WindroseServer.exe",
        "C:/games/windrose/R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe",
    ] {
        assert_eq!(
            Game::Windrose.install_root(Path::new(image)).unwrap(),
            PathBuf::from("C:/games/windrose")
        );
    }
    assert!(
        Game::Windrose
            .install_root(Path::new(
                "C:/games/other/WindroseServer-Win64-Shipping.exe"
            ))
            .is_err()
    );
    assert!(
        Game::Windrose
            .install_root(Path::new("C:/Windows/cmd.exe"))
            .is_err()
    );
}

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-windrose-ipc-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fresh_requests_replace_the_existing_slot_and_remove_owned_temporary_files() {
    let root = FixtureDirectory::new();
    let deadline = Instant::now() + TIMEOUT;
    publish_request(&root.0, &root.0, NONCE, 100_000, deadline).unwrap();
    let next = "abcdefabcdefabcdefabcdefabcdefab";
    publish_request(&root.0, &root.0, next, 101_000, deadline).unwrap();
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(root.0.join("request.json")).unwrap()).unwrap();
    assert_eq!(
        request,
        serde_json::json!({"request_id": next, "requested_at": 101})
    );
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
}

#[test]
fn failed_temporary_creation_preserves_the_preexisting_file() {
    let root = FixtureDirectory::new();
    let temporary = root.0.join(format!("request_{NONCE}.json"));
    fs::write(&temporary, "owned by another operation").unwrap();
    assert_eq!(
        publish_request(&root.0, &root.0, NONCE, 100_000, Instant::now() + TIMEOUT),
        Err(BridgeError::Io)
    );
    assert_eq!(
        fs::read_to_string(&temporary).unwrap(),
        "owned by another operation"
    );
    assert!(!root.0.join("request.json").exists());
    assert!(
        publish_request(
            &root.0,
            &root.0,
            "../escape",
            100_000,
            Instant::now() + TIMEOUT
        )
        .is_err()
    );
}

#[test]
fn missing_partial_and_old_responses_wait_for_a_complete_matching_document() {
    let root = FixtureDirectory::new();
    let path = root.0.join("response.json");
    assert_eq!(response_for_request(&root.0, &path, NONCE).unwrap(), None);
    for partial in [
        "",
        "{",
        "{\"request_id\":\"1234567890abcdef1234567890abcdef\",\"players\":[",
    ] {
        fs::write(&path, partial).unwrap();
        assert_eq!(response_for_request(&root.0, &path, NONCE).unwrap(), None);
    }
    let mut old = response(serde_json::json!([]));
    old["request_id"] = serde_json::json!("abcdefabcdefabcdefabcdefabcdefab");
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(response_for_request(&root.0, &path, NONCE).unwrap(), None);
    fs::remove_file(&path).unwrap();
    assert_eq!(response_for_request(&root.0, &path, NONCE).unwrap(), None);
    let current = serde_json::to_vec(&response(serde_json::json!([]))).unwrap();
    let temporary = root.0.join("response.pending");
    fs::write(&temporary, &current).unwrap();
    fs::rename(&temporary, &path).unwrap();
    assert_eq!(
        response_for_request(&root.0, &path, NONCE).unwrap(),
        Some(current)
    );
}

#[test]
fn response_file_type_root_and_size_limits_are_enforced() {
    let root = FixtureDirectory::new();
    let outside = FixtureDirectory::new();
    let path = root.0.join("response.json");
    fs::create_dir(&path).unwrap();
    assert_eq!(read_response(&root.0, &path), Err(BridgeError::Io));
    fs::remove_dir(&path).unwrap();
    fs::write(&path, vec![b' '; MAX_BYTES]).unwrap();
    assert_eq!(read_response(&root.0, &path).unwrap().len(), MAX_BYTES);
    fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    assert_eq!(read_response(&root.0, &path), Err(BridgeError::Limit));
    let other_path = outside.0.join("response.json");
    fs::write(&other_path, "{}").unwrap();
    assert_eq!(read_response(&root.0, &other_path), Err(BridgeError::Io));
    assert!(open_directory(&root.0, &path).is_err());
}

#[cfg(windows)]
#[test]
fn windows_directory_is_pinned_while_response_files_can_be_replaced() {
    let root = FixtureDirectory::new();
    let directory = root.0.join("langame_player_query");
    fs::create_dir(&directory).unwrap();
    let pin = open_directory(&root.0, &directory).unwrap();
    let moved = root.0.join("moved");
    assert!(fs::rename(&directory, &moved).is_err());
    publish_request(
        &root.0,
        &directory,
        NONCE,
        100_000,
        Instant::now() + TIMEOUT,
    )
    .unwrap();
    publish_request(
        &root.0,
        &directory,
        NONCE,
        101_000,
        Instant::now() + TIMEOUT,
    )
    .unwrap();
    let response_path = directory.join("response.json");
    fs::write(&response_path, "old response").unwrap();
    let next_response = directory.join("response.pending");
    fs::write(&next_response, "new response").unwrap();
    fs::rename(&next_response, &response_path).unwrap();
    assert_eq!(
        read_response(&root.0, &response_path).unwrap(),
        b"new response"
    );
    drop(pin);
    fs::rename(&directory, &moved).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_busy_files_are_pending_and_failed_publication_cleans_only_its_temporary() {
    use std::os::windows::fs::OpenOptionsExt;
    assert_eq!(
        file_error(std::io::Error::from_raw_os_error(5)),
        BridgeError::Io
    );
    let root = FixtureDirectory::new();
    let response_path = root.0.join("response.json");
    fs::write(
        &response_path,
        serde_json::to_vec(&response(serde_json::json!([]))).unwrap(),
    )
    .unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&response_path)
        .unwrap();
    assert_eq!(
        response_for_request(&root.0, &response_path, NONCE).unwrap(),
        None
    );
    drop(lock);
    assert!(
        response_for_request(&root.0, &response_path, NONCE)
            .unwrap()
            .is_some()
    );
    let request_path = root.0.join("request.json");
    fs::write(&request_path, "old request").unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&request_path)
        .unwrap();
    assert_eq!(
        publish_request(&root.0, &root.0, NONCE, 100_000, Instant::now()),
        Err(BridgeError::Timeout)
    );
    drop(lock);
    assert_eq!(fs::read_to_string(&request_path).unwrap(), "old request");
    assert!(!root.0.join(format!("request_{NONCE}.json")).exists());
    publish_request(&root.0, &root.0, NONCE, 100_000, Instant::now() + TIMEOUT).unwrap();
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(&request_path).unwrap()).unwrap();
    assert_eq!(request["request_id"], NONCE);
    assert!(!root.0.join(format!("request_{NONCE}.json")).exists());

    // A directory replacing the slot is a hard failure, even though Windows
    // also reports access denied for that rename.
    fs::remove_file(&request_path).unwrap();
    fs::create_dir(&request_path).unwrap();
    let source = root.0.join("request.pending");
    fs::write(&source, "keep source").unwrap();
    assert_eq!(
        replace_request(&source, &request_path),
        Err(BridgeError::Io)
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), "keep source");
}
