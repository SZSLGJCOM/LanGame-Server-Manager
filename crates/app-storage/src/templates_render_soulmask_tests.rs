use super::*;
use std::fs;
use uuid::Uuid;

fn test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "langame-soulmask-render-{}",
        Uuid::new_v4().as_simple()
    ));
    fs::create_dir_all(&root).expect("create root");
    root
}

#[test]
fn soulmask_profiles_combine_into_one_valid_document() {
    let root = test_root();
    for (id, value) in [("0", 10), ("1", 20), ("2", 30)] {
        fs::write(
            root.join(format!("GameXishu.profile-{id}.json")),
            format!(r#"{{"{id}":{{"value":{value}}}}}"#),
        )
        .expect("write profile");
    }

    combine_soulmask_profile_templates(&root).expect("combine profiles");

    let combined: Value = serde_json::from_slice(
        &fs::read(root.join(SOULMASK_GAME_XISHU_FILE)).expect("combined document"),
    )
    .expect("parse combined document");
    assert_eq!(combined["0"]["value"], 10);
    assert_eq!(combined["1"]["value"], 20);
    assert_eq!(combined["2"]["value"], 30);
    assert!(!root.join("GameXishu.profile-0.json").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_soulmask_profile_does_not_replace_output_or_delete_staging_files() {
    let root = test_root();
    let original = b"{\"existing\":true}\n";
    fs::write(root.join(SOULMASK_GAME_XISHU_FILE), original).expect("existing output");
    fs::write(root.join("GameXishu.profile-0.json"), r#"{"0":{}}"#).expect("profile zero");
    fs::write(root.join("GameXishu.profile-1.json"), r#"{"wrong":{}}"#).expect("profile one");
    fs::write(root.join("GameXishu.profile-2.json"), r#"{"2":{}}"#).expect("profile two");

    combine_soulmask_profile_templates(&root).expect_err("invalid profile must fail");

    assert_eq!(
        fs::read(root.join(SOULMASK_GAME_XISHU_FILE)).expect("existing output"),
        original
    );
    for id in ["0", "1", "2"] {
        assert!(root.join(format!("GameXishu.profile-{id}.json")).exists());
    }
    let _ = fs::remove_dir_all(root);
}
