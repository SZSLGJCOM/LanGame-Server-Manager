use super::*;

#[rustfmt::skip]
const V97_SETTINGS: &[(&str, &str, i64)] = &[
    ("num_gachas_per_tribe_for_penalty", "NumGachasPerTribeForPenalty", -1),
    ("gachas_over_tribe_limit_penalty", "GachasOverTribeLimitPenalty", 3),
    ("limit_replicators_per_saddle", "LimitReplicatorsPerSaddle", 0),
    ("limit_cloning_chambers_range", "LimitCloningChambersRange", 10000),
    ("limit_cloning_chambers_in_range_num", "LimitCloningChambersInRangeNum", 0),
    ("limit_cloning_chambers_in_tribe_num", "LimitCloningChambersInTribeNum", -1),
];

#[test]
fn asa_v97_limits_preserve_native_values_until_managed_and_remove_cleared_overrides() {
    let fixture = Fixture::new("arksurvivalascended");
    for (directory, previous) in [(&fixture.config, 5), (&fixture.live, 7)] {
        let mut content = String::from("[/script/shootergame.shootergamemode]\n");
        for &(_, native, _) in V97_SETTINGS {
            content.push_str(&format!("{native}={previous}\n"));
        }
        content.push_str("[Mod.Custom]\nKeep=existing\n");
        fs::write(directory.join("Game.ini"), content).unwrap();
    }

    // Repeated unrelated saves must not claim the newly modeled native keys.
    for _ in 0..2 {
        fixture.write(&json!({})).unwrap().commit();
        for (directory, previous) in [(&fixture.config, 5), (&fixture.live, 7)] {
            let content = fs::read_to_string(directory.join("Game.ini")).unwrap();
            for &(_, native, _) in V97_SETTINGS {
                let entries = content
                    .lines()
                    .filter(|line| line.starts_with(&format!("{native}=")))
                    .collect::<Vec<_>>();
                assert_eq!(entries, [format!("{native}={previous}")], "{content}");
            }
        }
    }

    let configured = V97_SETTINGS
        .iter()
        .map(|&(key, _, value)| (key.to_owned(), json!(value)))
        .collect::<Map<String, Value>>();
    fixture.write(&Value::Object(configured)).unwrap().commit();
    for directory in [&fixture.config, &fixture.live] {
        let content = fs::read_to_string(directory.join("Game.ini")).unwrap();
        for &(_, native, value) in V97_SETTINGS {
            let entries = content
                .lines()
                .filter(|line| line.starts_with(&format!("{native}=")))
                .collect::<Vec<_>>();
            assert_eq!(entries, [format!("{native}={value}")], "{content}");
        }
        assert!(content.contains("Keep=existing"));
    }

    fixture.write(&json!({})).unwrap().commit();
    for directory in [&fixture.config, &fixture.live] {
        let content = fs::read_to_string(directory.join("Game.ini")).unwrap();
        for &(_, native, _) in V97_SETTINGS {
            assert!(!content.contains(&format!("{native}=")), "{content}");
        }
        assert!(content.contains("Keep=existing"));
    }
}

#[test]
fn asa_v97_raw_extra_remains_the_final_override_and_can_be_cleared() {
    let fixture = Fixture::new("arksurvivalascended");
    for (settings, expected) in [
        (
            json!({
                "limit_replicators_per_saddle": 0,
                "game_ini_extra": "LimitReplicatorsPerSaddle=7"
            }),
            Some("LimitReplicatorsPerSaddle=7"),
        ),
        (
            json!({"limit_replicators_per_saddle": 0, "game_ini_extra": ""}),
            Some("LimitReplicatorsPerSaddle=0"),
        ),
        (json!({"game_ini_extra": ""}), None),
    ] {
        fixture.write(&settings).unwrap().commit();
        for directory in [&fixture.config, &fixture.live] {
            let content = fs::read_to_string(directory.join("Game.ini")).unwrap();
            let entries = content
                .lines()
                .filter(|line| line.starts_with("LimitReplicatorsPerSaddle="))
                .collect::<Vec<_>>();
            assert_eq!(
                entries,
                expected.into_iter().collect::<Vec<_>>(),
                "{content}"
            );
        }
    }
}
