use super::render_palworld_option_settings;
use serde_json::{Map, Value};

#[test]
fn documented_palworld_options_preserve_game_defaults_until_selected() {
    let output = render_palworld_option_settings(&Map::new(), &[]);
    assert!(!output.contains("FishingDifficultyRate="));
    assert!(!output.contains("bAllowEnemyCampSpawnNearBaseCamp="));

    let settings = Map::from_iter([
        (String::from("fishing_difficulty_rate"), Value::from(0.4)),
        (
            String::from("enemy_camp_spawn_near_base"),
            Value::from("prevent"),
        ),
    ]);
    let output = render_palworld_option_settings(&settings, &[]);
    assert_eq!(output.matches("FishingDifficultyRate=0.4").count(), 1);
    assert_eq!(
        output
            .matches("bAllowEnemyCampSpawnNearBaseCamp=False")
            .count(),
        1
    );

    let settings = Map::from_iter([(
        String::from("enemy_camp_spawn_near_base"),
        Value::from("allow"),
    )]);
    let output = render_palworld_option_settings(&settings, &[]);
    assert!(output.contains("bAllowEnemyCampSpawnNearBaseCamp=True"));

    let settings = Map::from_iter([(
        String::from("enemy_camp_spawn_near_base"),
        Value::from("default"),
    )]);
    let output = render_palworld_option_settings(&settings, &[]);
    assert!(!output.contains("bAllowEnemyCampSpawnNearBaseCamp="));
}

#[test]
fn palworld_renders_independent_building_limits_once() {
    let settings = Map::from_iter([
        (String::from("max_building_limit_num"), Value::from(1200)),
        (
            String::from("max_building_limit_num_per_player"),
            Value::from(300),
        ),
    ]);
    let output = render_palworld_option_settings(&settings, &[]);
    assert_eq!(output.matches("MaxBuildingLimitNum=1200,").count(), 1);
    assert_eq!(
        output.matches("MaxBuildingLimitNumPerPlayer=300,").count(),
        1
    );
    let defaults = render_palworld_option_settings(&Map::new(), &[]);
    assert_eq!(
        defaults.matches("MaxBuildingLimitNumPerPlayer=0,").count(),
        1
    );
}
