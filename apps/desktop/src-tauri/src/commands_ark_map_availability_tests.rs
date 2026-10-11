use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-installed-map-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn plugin(&self, relative: &str, id: &str, entry: Option<&str>) -> PathBuf {
        let plugin = self.0.join(relative);
        fs::create_dir_all(plugin.join("Preview")).unwrap();
        fs::create_dir_all(plugin.join("Content/Paks/WindowsServer")).unwrap();
        fs::write(
            plugin.join("MapPlugin.uplugin"),
            serde_json::json!({
                "Category":"UGC", "CanContainContent":true, "Installed":false,
                "Description":format!("Author map description&cf_ugcID={id}"),
                "FriendlyName":"Author's installed map"
            })
            .to_string(),
        )
        .unwrap();
        if let Some(entry) = entry {
            fs::write(plugin.join("Preview/entry_map.txt"), entry).unwrap();
        }
        for suffix in ["pak", "utoc", "ucas"] {
            fs::write(
                plugin.join(format!(
                    "Content/Paks/WindowsServer/MapPluginShooterGame-WindowsServer.{suffix}"
                )),
                b"external cooked payload fixture",
            )
            .unwrap();
        }
        plugin
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn budget() -> ScanBudget {
    ScanBudget {
        entries: MAX_SCAN_ENTRIES,
        metadata_bytes: MAX_TOTAL_METADATA_BYTES,
        deadline: Instant::now() + Duration::from_secs(10),
    }
}

#[test]
fn ark_installed_maps_accept_native_and_manual_author_metadata_without_title_inference() {
    let fixture = Fixture::new();
    // Native CF wrapper, as observed for real 928708/6601211; Installed=false
    // does not negate the successful CF package installation.
    let native = fixture.plugin(
        "893657_6581527/Svartalfheim",
        "893657",
        Some("\u{feff}Svartalfheim_WP\r\n"),
    );
    let manual = fixture.plugin(
        "Downloaded map folder/OtherPlugin",
        "1081192",
        Some("Atlantis_WP"),
    );
    fixture.plugin("928708_6601211/LevelTestingMod", "928708", None);
    fixture.plugin(
        ".temp/893657_6581527/Installing",
        "893657",
        Some("NotPublished_WP"),
    );
    let native_before = fs::read(native.join("MapPlugin.uplugin")).unwrap();
    let manual_before = fs::read(manual.join("Preview/entry_map.txt")).unwrap();
    let maps = discover_installed_maps(&fixture.0).unwrap();
    assert_eq!(
        maps.iter()
            .map(|map| (&*map.map_name, &*map.mod_id))
            .collect::<Vec<_>>(),
        [("Atlantis_WP", "1081192"), ("Svartalfheim_WP", "893657")]
    );
    assert!(maps.iter().all(|map| map.label == "Author's installed map"));
    assert_eq!(
        fs::read(native.join("MapPlugin.uplugin")).unwrap(),
        native_before
    );
    assert_eq!(
        fs::read(manual.join("Preview/entry_map.txt")).unwrap(),
        manual_before
    );
}

#[test]
fn ark_installed_maps_require_safe_entry_owner_and_complete_matching_server_containers() {
    let fixture = Fixture::new();
    let partial = fixture.plugin("partial", "1", Some("Partial_WP"));
    fs::remove_file(
        partial.join("Content/Paks/WindowsServer/MapPluginShooterGame-WindowsServer.ucas"),
    )
    .unwrap();
    fs::write(
        partial.join("Content/Paks/WindowsServer/Unrelated.ucas"),
        b"another container",
    )
    .unwrap();
    let client_only = fixture.plugin("client", "2", Some("ClientOnly_WP"));
    fs::remove_dir_all(client_only.join("Content/Paks/WindowsServer")).unwrap();
    fixture.plugin("invalid_map", "3", Some("Map_WP?SessionName=injected"));
    fixture.plugin("invalid_owner", "0", Some("NoOwner_WP"));
    let too_large = fixture.plugin("large", "4", Some("Large_WP"));
    fs::write(
        too_large.join("Preview/entry_map.txt"),
        "x".repeat(MAX_ENTRY_MAP_BYTES as usize + 1),
    )
    .unwrap();
    assert!(discover_installed_maps(&fixture.0).unwrap().is_empty());
    assert!(
        discover_installed_maps(&fixture.0.join("not-installed"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ark_installed_maps_reject_ambiguous_mod_ownership_and_deduplicate_same_mod() {
    let fixture = Fixture::new();
    fixture.plugin("first", "893657", Some("Shared_WP"));
    fixture.plugin("second", "893657", Some("Shared_WP"));
    assert_eq!(discover_installed_maps(&fixture.0).unwrap().len(), 1);
    fixture.plugin("conflicting", "1081192", Some("Shared_WP"));
    let error = discover_installed_maps(&fixture.0).unwrap_err();
    assert!(error.contains("Shared_WP") && error.contains("multiple Mods"));
    assert!(error.contains("893657") && error.contains("1081192"));
}

#[test]
fn ark_installed_map_scan_bounds_descriptor_entries_payload_entries_and_total_metadata() {
    let fixture = Fixture::new();
    let plugin = fixture.plugin("plugin", "893657", Some("Svartalfheim_WP"));
    for index in 0..12 {
        fs::write(plugin.join(format!("extra-{index}")), b"x").unwrap();
    }
    let mut descriptor_budget = budget();
    descriptor_budget.entries = 3;
    assert!(
        inspect_plugin(&plugin, &mut descriptor_budget)
            .unwrap_err()
            .contains("entry budget")
    );
    let mut payload_budget = budget();
    // entry metadata + 16 descriptor directory entries + descriptor read;
    // no work remains for inspecting the first payload container.
    payload_budget.entries = 2 + fs::read_dir(&plugin).unwrap().count();
    assert!(
        inspect_plugin(&plugin, &mut payload_budget)
            .unwrap_err()
            .contains("entry budget")
    );
    let mut bytes = budget();
    bytes.metadata_bytes = 8;
    assert!(
        discover_with_budget(&fixture.0, bytes)
            .unwrap_err()
            .contains("metadata byte budget")
    );
    let mut timed_out = budget();
    timed_out.deadline = Instant::now();
    assert!(
        discover_with_budget(&fixture.0, timed_out)
            .unwrap_err()
            .contains("time budget")
    );
}

#[test]
fn ark_map_content_guard_reports_missing_or_installed_but_disabled_map_before_start() {
    let fixture = Fixture::new();
    let schema = serde_json::json!({"properties":{"map_name":{"x-lsgm-suggestions":[{"value":"TheIsland_WP"}]}}}).to_string();
    assert!(check_map_content(&schema, &["theisland_wp".into()], &fixture.0, false).is_ok());
    let error = check_map_content(&schema, &["Atlantis_WP".into()], &fixture.0, false).unwrap_err();
    assert!(
        error.contains("Atlantis_WP") && error.contains("Mod ID") && error.contains("preserved")
    );
    fixture.plugin("manual/AuthorMap", "1081192", Some("Atlantis_WP"));
    let error = check_map_content(&schema, &["Atlantis_WP".into()], &fixture.0, false).unwrap_err();
    assert!(
        error.contains("installed") && error.contains("not enabled") && error.contains("1081192")
    );
    assert!(check_map_content(&schema, &["Atlantis_WP".into()], &fixture.0, true).is_ok());
}

#[test]
fn ark_installed_map_label_cannot_be_empty_after_control_filtering() {
    let fixture = Fixture::new();
    let plugin = fixture.plugin("plugin", "893657", Some("Svartalfheim_WP"));
    let path = plugin.join("MapPlugin.uplugin");
    let mut descriptor: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    descriptor["FriendlyName"] = serde_json::json!("\u{0000}\r\n\t  ");
    fs::write(path, descriptor.to_string()).unwrap();
    assert_eq!(
        discover_installed_maps(&fixture.0).unwrap()[0].label,
        "Svartalfheim_WP"
    );
}

#[test]
fn ark_map_content_guard_preserves_first_download_custom_paths_and_mapmodid() {
    let fixture = Fixture::new();
    let schema = serde_json::json!({"properties":{"map_name":{"x-lsgm-suggestions":[{"value":"TheIsland_WP"}]}}}).to_string();
    for arguments in [
        vec!["-mods=1081192"],
        vec!["-passivemods", "1081192"],
        vec!["-MAPMODID=1081192"],
    ] {
        let arguments = arguments.into_iter().map(String::from).collect::<Vec<_>>();
        assert!(has_mod_content_source(&arguments));
        assert!(
            check_map_content(
                &schema,
                &["Atlantis_WP".into()],
                &fixture.0,
                has_mod_content_source(&arguments)
            )
            .is_ok()
        );
    }
    assert!(!has_mod_content_source(&[
        "TheIsland_WP?SessionName=-mods=1081192".into()
    ]));
    assert!(!has_mod_content_source(&["-mods".into(), "-log".into()]));
    assert!(check_map_content(&schema, &["/Game/Custom/MyMap".into()], &fixture.0, false).is_ok());
    assert!(check_map_content("{}", &["CustomModuleMap".into()], &fixture.0, false).is_ok());
}
