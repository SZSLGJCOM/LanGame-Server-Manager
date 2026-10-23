use super::*;

const NONCE: &str = "1234567890abcdef1234567890abcdef";

fn response(preset: &str) -> String {
    format!(
        "Unknown command \"LGM_PLAYER_QUERY_BEGIN_{NONCE}\"!\n\n> difficulty\nPreset: {preset}\nCombat Difficulty: Default\n\n> LGM_PLAYER_QUERY_END_{NONCE}\nUnknown command \"LGM_PLAYER_QUERY_END_{NONCE}\"!\n"
    )
}

#[test]
fn moria_content_requires_fresh_native_preset_not_ready_or_configuration() {
    assert_eq!(
        parse_difficulty(&response("Normal"), NONCE).unwrap(),
        Difficulty::Normal
    );
    assert_eq!(
        parse_difficulty(&response("Hard"), NONCE).unwrap(),
        Difficulty::Hard
    );
    for invalid in [
        "world_ready=true\nPreset: Hard".to_owned(),
        "[World.Create]\nDifficulty.Preset=hard".to_owned(),
        response("Hard").replace("> difficulty", "> players"),
        response("Hard").replace("Preset: Hard", "Preset: Hard\nPreset: Normal"),
        response("Custom"),
        response("Hard").replace(
            "Preset: Hard",
            "The difficulty system has not yet been initialized. Please retry this command later.",
        ),
        response("Hard").replace(NONCE, "00000000000000000000000000000000"),
    ] {
        assert!(parse_difficulty(&invalid, NONCE).is_err());
    }
}

#[test]
fn moria_readback_cannot_pass_by_recreating_a_hard_world_from_configuration() {
    require_normal_create_config("[World.Create]\nDifficulty.Preset=normal\n").unwrap();
    for invalid in [
        "[World.Create]\nDifficulty.Preset=hard\n",
        "[World]\nDifficulty.Preset=normal\n",
        "[World.Create]\nDifficulty.Preset=normal\nDifficulty.Preset=hard\n",
        "[World.Create]\nDifficulty.Preset=normal\n[World.Create]\nDifficulty.Preset=normal\n",
    ] {
        assert!(require_normal_create_config(invalid).is_err());
    }
}

#[test]
fn moria_world_binding_rejects_missing_nested_and_ambiguous_worlds() {
    let world = PathBuf::from("MW_28CD8DA4415A8C8A9F18B9BE601D53F0.sav");
    let mut manifest = Manifest::new();
    assert!(world_file(&manifest).is_err());
    manifest.insert(world.clone(), (42, "digest".into()));
    assert_eq!(world_file(&manifest).unwrap(), world);
    manifest.insert(
        PathBuf::from("MW_11111111111111111111111111111111.sav"),
        (42, "other".into()),
    );
    assert!(world_file(&manifest).is_err());
    manifest.clear();
    manifest.insert(PathBuf::from("nested").join(world), (42, "digest".into()));
    assert!(world_file(&manifest).is_err());
}
