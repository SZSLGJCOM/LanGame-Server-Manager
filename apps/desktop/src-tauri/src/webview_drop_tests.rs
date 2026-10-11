use super::*;
use serde_json::json;

const REQUEST_ID: &str = "920ea0a6-7ae7-4467-a9af-ea26ecc4ff6c";

#[test]
fn dropped_file_request_only_accepts_its_bounded_uuid_protocol() {
    let request = json!({"type": REQUEST_TYPE, "requestId": REQUEST_ID}).to_string();
    assert_eq!(
        parse_request(&request).unwrap(),
        (REQUEST_ID.to_owned(), Ok(()))
    );
    assert!(parse_request(&json!({"type":"other", "requestId":REQUEST_ID}).to_string()).is_none());
    assert!(
        parse_request(&json!({"type":REQUEST_TYPE, "requestId":"not-a-uuid"}).to_string())
            .is_none()
    );
    assert!(parse_request(&" ".repeat(MAX_REQUEST_BYTES + 1)).is_none());
    let supplied_path = json!({"type":REQUEST_TYPE, "requestId":REQUEST_ID, "paths":["C:/not-an-additional-object"]});
    assert_eq!(
        parse_request(&supplied_path.to_string()).unwrap().1,
        Err(DropError::InvalidRequest)
    );
}

#[test]
fn dropped_file_origin_policy_rejects_remote_lookalikes_credentials_and_origin_changes() {
    let policy = OriginPolicy::local("http://127.0.0.1:43170/app").unwrap();
    assert!(policy.accepts("http://127.0.0.1:43170/view", "http://127.0.0.1:43170/app"));
    for source in [
        "https://example.com/",
        "http://127.0.0.1:43171/",
        "http://127.0.0.1.evil.test:43170/",
        "http://user@127.0.0.1:43170/",
    ] {
        assert!(!policy.accepts(source, "http://127.0.0.1:43170/app"));
    }
    assert!(!policy.accepts("http://127.0.0.1:43170/app", "https://example.com/"));
    assert!(OriginPolicy::local("https://example.com/").is_err());
    let assets = OriginPolicy::assets(false);
    assert!(assets.accepts(
        "http://tauri.localhost/app",
        "http://tauri.localhost/index.html"
    ));
    assert!(!assets.accepts(
        "http://tauri.localhost.evil.test/",
        "http://tauri.localhost/"
    ));
    assert!(!assets.accepts("https://tauri.localhost/", "http://tauri.localhost/"));
    assert!(assets.accepts_frame(
        "http://tauri.localhost/lgsm-file-drop.html",
        "http://tauri.localhost/"
    ));
    for source in [
        "http://tauri.localhost/other.html",
        "http://tauri.localhost/lgsm-file-drop.html?q=1",
        "http://tauri.localhost/lgsm-file-drop.html#fragment",
    ] {
        assert!(!assets.accepts_frame(source, "http://tauri.localhost/"));
    }
}

#[cfg(windows)]
#[test]
fn dropped_file_paths_keep_directories_and_unicode_without_enumerating_them() {
    let paths = validate_paths(
        2,
        [
            Ok("D:\\drop fixture\\folder".into()),
            Ok("D:\\drop fixture\\中文.zip".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        paths,
        ["D:\\drop fixture\\folder", "D:\\drop fixture\\中文.zip"]
    );
    assert!(matches!(
        validate_paths(1, [Ok("relative.zip".into())]),
        Err(DropError::InvalidPath)
    ));
    assert!(matches!(
        validate_paths(1, [Ok("D:\\a\\..\\file.zip".into())]),
        Err(DropError::InvalidPath)
    ));
    assert!(matches!(
        validate_paths(1, [Ok(String::new())]),
        Err(DropError::PathUnavailable)
    ));
    assert!(matches!(
        validate_paths(0, std::iter::empty()),
        Err(DropError::InvalidRequest)
    ));
    assert!(matches!(
        validate_paths(257, std::iter::empty()),
        Err(DropError::TooManyFiles)
    ));
    assert!(matches!(
        validate_paths(1, [Err(DropError::UnsupportedObject)]),
        Err(DropError::UnsupportedObject)
    ));
}

#[cfg(windows)]
#[test]
fn dropped_file_path_budget_is_bounded_and_no_partial_results_escape() {
    let paths = (0..MAX_OBJECTS).map(|index| Ok(format!("D:\\drop\\{index}.zip")));
    assert_eq!(
        validate_paths(MAX_OBJECTS, paths).unwrap().len(),
        MAX_OBJECTS as usize
    );
    let paths = (0..MAX_OBJECTS).map(|_| Ok(format!("D:\\{}", "x".repeat(8192))));
    assert_eq!(
        validate_paths(MAX_OBJECTS, paths).unwrap_err(),
        DropError::InvalidPath
    );
    let reply = reply_json(REQUEST_ID, Err(DropError::UnsupportedObject));
    let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["paths"], json!([]));
    assert_eq!(reply["error"], "unsupported_object");
    assert_eq!(reply["requestId"], REQUEST_ID);
    assert_eq!(reply["type"], RESPONSE_TYPE);
}
