use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-nexus-routes-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(path.join("runtime")).unwrap();
        Self(path)
    }

    fn target(&self, module: &str) -> ResolvedManualModTarget {
        let profile = layout::Profile::for_module(module).unwrap();
        ResolvedManualModTarget {
            instance_id: "nexus-route-fixture".into(),
            module_id: module.into(),
            source_label: "Nexus Mods".into(),
            target_label: profile.target().into(),
            target_path: self.0.join("runtime").join(profile.target()),
            accepts: vec!["zip".into(), "folder".into(), "pak".into()],
            id_strategy: None,
        }
    }

    fn zip(&self, files: &[(&str, &[u8])]) -> PathBuf {
        let path = self
            .0
            .join(format!("payload-{}.zip", uuid::Uuid::new_v4().simple()));
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, bytes) in files {
            archive
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    fn install(
        &self,
        module: &str,
        project: &str,
        files: &[(&str, &[u8])],
    ) -> Result<ManualModStageResult, String> {
        crate::commands::commands_mod_staging::stage_website_mod_source(
            self.target(module),
            self.zip(files),
            ManualModImportPresentation {
                display_name: Some("Verified project".into()),
                icon_url: Some(
                    "https://staticdelivery.nexusmods.com/mods/5192/images/73/73-1.png".into(),
                ),
            },
            Some(project),
        )
    }

    fn inventory(&self, module: &str) -> ManualModInventoryResult {
        crate::commands::commands_mods::mod_inventory::read_inventory(self.target(module)).unwrap()
    }

    fn manage(
        &self,
        module: &str,
        project: &str,
        action: &str,
    ) -> Result<ManagedModActionResult, String> {
        management::manage_package(
            self.target(module),
            &format!("nexus-{module}-{project}"),
            action,
        )
    }

    fn management_state(&self, module: &str) -> PathBuf {
        let target = self.target(module);
        for parent in target.target_path.ancestors() {
            if !parent.starts_with(self.0.join("runtime")) {
                break;
            }
            if let Ok(entries) = fs::read_dir(parent)
                && let Some(state) = entries
                    .map(Result::unwrap)
                    .map(|entry| entry.path())
                    .find(|path| path.join("owner.json").is_file())
            {
                return state;
            }
        }
        panic!("Mod management state was not created");
    }

    fn write_disable_journal(
        &self,
        module: &str,
        project: &str,
        protected_routed_files: &BTreeMap<String, String>,
    ) -> PathBuf {
        let state = self.management_state(module);
        let original =
            read_manifest(&self.target(module).target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        let record = serde_json::json!({
            "schema": 1, "key": format!("nexus-{module}-{project}"), "action": "disable",
            "retained_name": "interrupted", "original": original, "adopted": null,
            "had_payload": true, "payload_files": {},
            "protected_routed_files": protected_routed_files,
        });
        fs::write(
            state.join("pending.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        state
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nexus_route_layout_maps_only_known_server_containers() {
    for (module, input, expected) in [
        (
            "abioticfactor",
            "DistributionPad.pak",
            "AbioticFactor/Content/Paks/DistributionPad.pak",
        ),
        (
            "abioticfactor",
            "Release/Content/Paks/~mods/Example.pak",
            "AbioticFactor/Content/Paks/~mods/Example.pak",
        ),
        (
            "humanitz",
            "CraftingRecipes_Server_P.pak",
            "HumanitZServer/Content/Paks/~mods/CraftingRecipes_Server_P.pak",
        ),
        (
            "humanitz",
            "HumanitZServer/Content/Paks/LogicMods/Example.pak",
            "HumanitZServer/Content/Paks/LogicMods/Example.pak",
        ),
        (
            "humanitz",
            "HumanitZ/Content/Paks/Mods/10x_EXP_P.pak",
            "HumanitZServer/Content/Paks/Mods/10x_EXP_P.pak",
        ),
        (
            "humanitz",
            "HumanitZ/Content/Paks/Client.pak",
            "HumanitZServer/Content/Paks/Client.pak",
        ),
        (
            "humanitz",
            "Release/humanitz/content/paks/LogicMods/Example.pak",
            "HumanitZServer/Content/Paks/LogicMods/Example.pak",
        ),
        (
            "humanitz",
            "ue4ss/Mods/Example/Scripts/main.lua",
            "HumanitZServer/Binaries/Win64/ue4ss/Mods/Example/Scripts/main.lua",
        ),
        (
            "abioticfactor",
            "Example/Scripts/main.lua",
            "AbioticFactor/Binaries/Win64/ue4ss/Mods/Example/Scripts/main.lua",
        ),
    ] {
        let payload = BTreeMap::from([(input.into(), "0".repeat(64))]);
        let planned =
            layout::plan(layout::Profile::for_module(module).unwrap(), &payload, "73").unwrap();
        assert_eq!(planned[0].destination.as_deref(), Some(expected), "{input}");
    }
    for input in [
        "main.lua",
        "AbioticFactor/Content/Paks/Foreign.pak",
        "UE4SS.dll",
        "version.dll",
        "ue4ss/UE4SS.dll",
        "Mods/LgsmWorldSave/Scripts/main.lua",
        "Something/only.lua",
        "Orphan.ucas",
    ] {
        let payload = BTreeMap::from([(input.into(), "0".repeat(64))]);
        assert!(
            layout::plan(layout::Profile::HumanitZ, &payload, "73").is_err(),
            "{input}"
        );
    }
}

#[test]
fn nexus_pak_routes_install_inventory_disable_enable_delete_as_one_project() {
    for (module, expected) in [
        ("abioticfactor", "AbioticFactor/Content/Paks/Example.pak"),
        ("humanitz", "HumanitZServer/Content/Paks/~mods/Example.pak"),
    ] {
        let fixture = Fixture::new();
        let installed = fixture
            .install(module, "73", &[("Example.pak", b"real payload")])
            .unwrap();
        let live = fixture.0.join("runtime").join(expected);
        assert_eq!(fs::read(&live).unwrap(), b"real payload");
        assert_eq!(installed.copied_file_count, 1);
        assert_eq!(installed.copied_total_bytes, 12);
        let inventory = fixture.inventory(module);
        assert_eq!(inventory.items.len(), 1);
        let item = &inventory.items[0];
        assert_eq!(item.display_name.as_deref(), Some("Verified project"));
        assert!(item.icon_url.is_some());
        assert_eq!(item.file_count, 1);
        assert_eq!(item.total_bytes, 12);
        assert_eq!(
            item.management.as_ref().unwrap().package_id,
            format!("nexus-{module}-73")
        );
        let disabled = fixture.manage(module, "73", "disable").unwrap();
        assert!(!live.exists());
        assert_eq!(disabled.inventory.items.len(), 1);
        assert!(
            !disabled.inventory.items[0]
                .management
                .as_ref()
                .unwrap()
                .enabled
        );
        assert_eq!(disabled.inventory.items[0].file_count, 1);
        let enabled = fixture.manage(module, "73", "enable").unwrap();
        assert!(
            enabled.inventory.items[0]
                .management
                .as_ref()
                .unwrap()
                .enabled
        );
        assert_eq!(fs::read(&live).unwrap(), b"real payload");
        let deleted = fixture.manage(module, "73", "delete").unwrap();
        assert_eq!(deleted.removed_file_count, 1);
        assert!(deleted.inventory.items.is_empty());
        assert!(!live.exists());
    }
}

#[test]
fn nexus_enshrouded_building_tools_installs_languages_and_preserves_runtime_data() {
    // Model the observed file1902 paths; these are fixture bytes, not the Nexus binary.
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    let retained = [
        ("dbghelp.dll", b"LanGame native control".as_slice()),
        ("BuildingTools.ini", b"[Keys]\nMenu=Home\n"),
        ("BuildingTools-Blueprints/Keep.json", b"existing blueprint"),
        ("BuildingTools-Languages/custom.txt", b"custom translation"),
    ];
    for (path, bytes) in retained {
        let target = runtime.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }
    let installed = fixture
        .install("enshrouded", "221", BUILDING_TOOLS_FILES)
        .unwrap();
    assert_eq!(installed.copied_file_count, 5);
    let inventory = fixture.inventory("enshrouded");
    assert_eq!(inventory.items.len(), 1);
    assert_eq!(inventory.items[0].file_count, 5);
    for (path, bytes) in BUILDING_TOOLS_FILES {
        assert_eq!(fs::read(runtime.join(path)).unwrap(), *bytes, "{path}");
    }
    for action in ["disable", "enable", "delete"] {
        let managed = fixture.manage("enshrouded", "221", action).unwrap();
        for (path, bytes) in BUILDING_TOOLS_FILES {
            if action == "enable" {
                assert_eq!(fs::read(runtime.join(path)).unwrap(), *bytes, "{path}");
            } else {
                assert!(!runtime.join(path).exists(), "{path}");
            }
        }
        for (path, bytes) in retained {
            assert_eq!(fs::read(runtime.join(path)).unwrap(), bytes, "{path}");
        }
        assert_eq!(managed.inventory.items.is_empty(), action == "delete");
    }
}

const BUILDING_TOOLS_FILES: &[(&str, &[u8])] = &[
    ("dinput8.dll", b"Building Tools loader fixture"),
    ("BuildingTools-Languages/de.txt", b"German fixture"),
    ("BuildingTools-Languages/en.txt", b"English fixture"),
    ("BuildingTools-Languages/es.txt", b"Spanish fixture"),
    ("BuildingTools-Languages/fr.txt", b"French fixture"),
];

#[test]
fn nexus_enshrouded_building_tools_rejects_partial_or_unsupported_files_and_preserves_owners() {
    let partial = Fixture::new();
    assert!(
        partial
            .install("enshrouded", "221", &[BUILDING_TOOLS_FILES[1]])
            .is_err()
    );
    assert!(partial.inventory("enshrouded").items.is_empty());
    for unsupported in [
        "BuildingTools-Languages/Unknown.dll",
        "BuildingTools-Languages/nested/en.txt",
        "BuildingTools-Languages/.hidden.txt",
        "Unrelated.ini",
        "unknown.dll",
    ] {
        let fixture = Fixture::new();
        let mut files = BUILDING_TOOLS_FILES.to_vec();
        files.push((unsupported, b"unsupported payload"));
        assert!(
            fixture.install("enshrouded", "221", &files).is_err(),
            "{unsupported}"
        );
        assert!(!fixture.0.join("runtime/dinput8.dll").exists());
        assert!(fixture.inventory("enshrouded").items.is_empty());
    }

    let unowned = Fixture::new();
    let language = unowned.0.join("runtime/BuildingTools-Languages/en.txt");
    fs::create_dir_all(language.parent().unwrap()).unwrap();
    fs::write(&language, b"operator translation").unwrap();
    assert!(
        unowned
            .install("enshrouded", "221", BUILDING_TOOLS_FILES)
            .unwrap_err()
            .contains("unowned")
    );
    assert_eq!(fs::read(language).unwrap(), b"operator translation");
    assert!(!unowned.0.join("runtime/dinput8.dll").exists());

    let fixture = Fixture::new();
    fixture
        .install("enshrouded", "221", BUILDING_TOOLS_FILES)
        .unwrap();
    assert!(
        fixture
            .install("enshrouded", "222", BUILDING_TOOLS_FILES)
            .unwrap_err()
            .contains("another Mod")
    );
    let language = fixture.0.join("runtime/BuildingTools-Languages/en.txt");
    fs::write(&language, b"operator translation").unwrap();
    assert!(
        fixture
            .install("enshrouded", "221", BUILDING_TOOLS_FILES)
            .unwrap_err()
            .contains("modified")
    );
    fixture.manage("enshrouded", "221", "disable").unwrap();
    assert!(!language.exists());
    fixture.manage("enshrouded", "221", "enable").unwrap();
    assert_eq!(fs::read(&language).unwrap(), b"operator translation");
    for (path, bytes) in BUILDING_TOOLS_FILES {
        if !path.ends_with("/en.txt") {
            assert_eq!(
                fs::read(fixture.0.join("runtime").join(path)).unwrap(),
                *bytes,
                "{path}"
            );
        }
    }
    assert_eq!(fixture.inventory("enshrouded").items.len(), 1);
    let deleted = fixture.manage("enshrouded", "221", "delete").unwrap();
    assert!(!language.exists());
    assert!(!fixture.0.join("runtime/dinput8.dll").exists());
    let preserved = PathBuf::from(deleted.preserved_path.unwrap());
    assert_eq!(
        fs::read(preserved.join("runtime/BuildingTools-Languages/en.txt")).unwrap(),
        b"operator translation"
    );
    assert!(fixture.inventory("enshrouded").items.is_empty());
}

#[test]
fn nexus_enshrouded_xhl_loader_uses_root_and_preserves_existing_control_and_owners() {
    let fixture = Fixture::new();
    let root = fixture.0.join("runtime");
    fs::write(root.join("dbghelp.dll"), b"native control").unwrap();
    let installed = fixture
        .install(
            "enshrouded",
            "250",
            &[
                ("dinput8.dll", b"xhl loader"),
                (
                    "mods/AZJ-LootMultiplier/AZJ-LootMultiplier.dll",
                    b"mod payload",
                ),
                ("mods/AZJ-LootMultiplier/mod.json", b"{}"),
                ("mods/AZJ-LootMultiplier/config.txt", b"multiplier=2"),
            ],
        )
        .unwrap();
    assert_eq!(installed.copied_file_count, 4);
    assert_eq!(fs::read(root.join("dinput8.dll")).unwrap(), b"xhl loader");
    assert_eq!(
        fs::read(root.join("mods/AZJ-LootMultiplier/config.txt")).unwrap(),
        b"multiplier=2"
    );
    assert_eq!(
        fs::read(root.join("dbghelp.dll")).unwrap(),
        b"native control"
    );
    assert!(
        fixture
            .install("enshrouded", "236", &[("dinput8.dll", b"different loader")])
            .is_err()
    );
    assert_eq!(fs::read(root.join("dinput8.dll")).unwrap(), b"xhl loader");

    let standalone = Fixture::new();
    standalone
        .install("enshrouded", "236", &[("DINPUT8.DLL", b"standalone XHL")])
        .unwrap();
    let proxy = standalone.0.join("runtime/dinput8.dll");
    assert_eq!(fs::read(&proxy).unwrap(), b"standalone XHL");
    standalone.manage("enshrouded", "236", "disable").unwrap();
    assert!(!proxy.exists());
    standalone.manage("enshrouded", "236", "enable").unwrap();
    assert_eq!(fs::read(&proxy).unwrap(), b"standalone XHL");
    standalone.manage("enshrouded", "236", "delete").unwrap();
    assert!(!proxy.exists());

    let unowned = Fixture::new();
    let proxy = unowned.0.join("runtime/dinput8.dll");
    fs::write(&proxy, b"operator loader").unwrap();
    assert!(
        unowned
            .install("enshrouded", "236", &[("dinput8.dll", b"incoming")])
            .is_err()
    );
    assert_eq!(fs::read(&proxy).unwrap(), b"operator loader");
}

#[test]
fn nexus_enshrouded_server_proxy_can_pair_with_explicit_mods_without_xpshare_config() {
    let fixture = Fixture::new();
    let installed = fixture
        .install(
            "enshrouded",
            "146",
            &[
                ("powrprof.dll", b"server proxy"),
                ("mods/creative/creative.dll", b"server mod"),
            ],
        )
        .unwrap();
    assert_eq!(installed.copied_file_count, 2);
    assert_eq!(
        fs::read(fixture.0.join("runtime/powrprof.dll")).unwrap(),
        b"server proxy"
    );
    assert_eq!(
        fs::read(fixture.0.join("runtime/mods/creative/creative.dll")).unwrap(),
        b"server mod"
    );
    assert!(!fixture.0.join("runtime/xpshare.ini").exists());
}

#[test]
fn nexus_payload_wrapper_allows_root_documentation_without_moving_it_into_runtime() {
    for (module, source, destination) in [
        (
            "abioticfactor",
            "Release/Example.pak",
            "AbioticFactor/Content/Paks/Example.pak",
        ),
        (
            "humanitz",
            "Release/Example.pak",
            "HumanitZServer/Content/Paks/~mods/Example.pak",
        ),
        (
            "enshrouded",
            "Release/shroudlink_mods/Example.dll",
            "shroudlink_mods/Example.dll",
        ),
    ] {
        let fixture = Fixture::new();
        let result = fixture
            .install(
                module,
                "73",
                &[(source, b"payload"), ("README.txt", b"instructions")],
            )
            .unwrap();
        assert_eq!(result.copied_file_count, 2);
        let root = fixture.0.join("runtime");
        assert_eq!(fs::read(root.join(destination)).unwrap(), b"payload");
        assert!(!root.join("README.txt").exists());
        assert_eq!(
            fs::read(
                fixture
                    .target(module)
                    .target_path
                    .join(format!("nexus-{module}-73/README.txt"))
            )
            .unwrap(),
            b"instructions"
        );
    }
    // An unrelated root file must not be discarded to make a wrapper fit.
    let payload = BTreeMap::from([
        ("Release/Example.pak".into(), "0".repeat(64)),
        ("RequiredConfig.txt".into(), "0".repeat(64)),
    ]);
    let error = layout::plan(layout::Profile::HumanitZ, &payload, "73").unwrap_err();
    assert!(error.contains("unrecognized entries"));
    assert!(error.contains("RequiredConfig.txt"));
}

#[test]
fn nexus_reviewed_server_proxy_keeps_all_bundled_license_notices() {
    // Public XP Share v1.1.3 archive: two runtime files and three documents.
    let fixture = Fixture::new();
    let installed = fixture
        .install(
            "enshrouded",
            "178",
            &[
                ("powrprof.dll", b"proxy"),
                ("xpshare.ini", b"enabled=true"),
                ("LICENSE", b"license terms"),
                ("README.txt", b"installation instructions"),
                ("THIRD_PARTY_NOTICES.md", b"dependency license notices"),
            ],
        )
        .unwrap();
    assert_eq!(installed.copied_file_count, 5);
    let runtime = fixture.0.join("runtime");
    assert_eq!(fs::read(runtime.join("powrprof.dll")).unwrap(), b"proxy");
    assert_eq!(
        fs::read(runtime.join("xpshare.ini")).unwrap(),
        b"enabled=true"
    );
    let anchor = fixture
        .target("enshrouded")
        .target_path
        .join("nexus-enshrouded-178");
    for name in ["LICENSE", "README.txt", "THIRD_PARTY_NOTICES.md"] {
        assert!(anchor.join(name).is_file());
        assert!(!runtime.join(name).exists());
    }
    assert_eq!(fixture.inventory("enshrouded").items[0].file_count, 5);
}

#[test]
fn nexus_server_pak_bundle_preserves_localized_readme_and_font_license() {
    // Author's published SHA256SUMS lists these six DedicatedServer ZIP entries:
    // https://github.com/Lionademais/humanitz-npc-expansion/blob/main/SHA256SUMS.txt
    let fixture = Fixture::new();
    let files: &[(&str, &[u8])] = &[
        (
            "HumanitZServer/Content/Paks/NPCExpansion_Server_P.pak",
            b"pak",
        ),
        (
            "HumanitZServer/Content/Paks/NPCExpansion_Server_P.ucas",
            b"ucas",
        ),
        (
            "HumanitZServer/Content/Paks/NPCExpansion_Server_P.utoc",
            b"utoc",
        ),
        (
            "HumanitZServer/Content/Paks/NPCExpansion_Server_P.sig",
            b"sig",
        ),
        ("LEIA-ME.txt", b"Author installation instructions"),
        ("OFL_Manrope.txt", b"Font license notice"),
    ];
    let installed = fixture.install("humanitz", "180", files).unwrap();
    assert_eq!(installed.copied_file_count, 6);
    let runtime = fixture.0.join("runtime");
    for (relative, bytes) in &files[..4] {
        assert_eq!(fs::read(runtime.join(relative)).unwrap(), *bytes);
    }
    let anchor = fixture
        .target("humanitz")
        .target_path
        .join("nexus-humanitz-180");
    for (name, bytes) in &files[4..] {
        assert_eq!(fs::read(anchor.join(name)).unwrap(), *bytes);
        assert!(!runtime.join(name).exists());
    }
    let manifest = read_manifest(
        &fixture
            .target("humanitz")
            .target_path
            .join(ONLINE_MOD_MANIFEST),
    )
    .unwrap();
    let owned = &manifest.packages["nexus-humanitz-180"];
    assert_eq!(owned.routed_files.len(), 4);
    assert_eq!(owned.files.len(), 2);
    assert_eq!(fixture.inventory("humanitz").items[0].file_count, 6);
    assert!(
        fixture
            .install(
                "humanitz",
                "181",
                &[
                    ("Other.pak", b"other"),
                    (
                        "RequiredConfig.txt",
                        b"Must not be mistaken for documentation"
                    ),
                ]
            )
            .is_err()
    );
    assert!(
        !fixture
            .target("humanitz")
            .target_path
            .join("Other.pak")
            .exists()
    );
    assert!(
        !fixture
            .target("humanitz")
            .target_path
            .join("nexus-humanitz-181")
            .exists()
    );
    assert_eq!(fixture.inventory("humanitz").items.len(), 1);
}

#[test]
fn nexus_humanitz_client_named_lua_container_rebases_without_adopting_its_loader() {
    let fixture = Fixture::new();
    let prefix = "HumanitZ/Binaries/Win64/UE4SS/Mods/AL_Ultimate_Ztack";
    let script = format!("{prefix}/Scripts/main.lua");
    let config = format!("{prefix}/config.lua");
    fixture
        .install(
            "humanitz",
            "61",
            &[
                (script.as_str(), b"return 61"),
                (config.as_str(), b"return {}"),
            ],
        )
        .unwrap();
    let runtime = fixture.0.join("runtime");
    let installed = runtime.join("HumanitZServer/Binaries/Win64/ue4ss/Mods/AL_Ultimate_Ztack");
    assert_eq!(
        fs::read(installed.join("Scripts/main.lua")).unwrap(),
        b"return 61"
    );
    assert_eq!(
        fs::read(installed.join("config.lua")).unwrap(),
        b"return {}"
    );
    assert!(installed.join("enabled.txt").is_file());
    assert!(!runtime.join("HumanitZ").exists());
    assert_eq!(fixture.inventory("humanitz").items.len(), 1);

    let with_loader = fixture.install(
        "humanitz",
        "62",
        &[
            (
                "HumanitZ/Binaries/Win64/ue4ss/Mods/Other/Scripts/main.lua",
                b"return 62",
            ),
            ("HumanitZ/Binaries/Win64/ue4ss/UE4SS.dll", b"foreign loader"),
        ],
    );
    assert!(with_loader.is_err());
    assert!(!installed.parent().unwrap().join("Other").exists());
    assert!(!runtime.join("HumanitZ").exists());
    assert_eq!(fixture.inventory("humanitz").items.len(), 1);
    let foreign = BTreeMap::from([(script, "0".repeat(64))]);
    assert!(layout::plan(layout::Profile::AbioticFactor, &foreign, "61").is_err());
}

#[test]
fn nexus_humanitz_client_pak_bundle_keeps_lua_and_sidecars_through_management() {
    // This is a complete local fixture, not evidence that either separate H107
    // website component is a complete install or that a client Pak works on a server.
    for wrapper in ["", "Release/"] {
        let fixture = Fixture::new();
        let entries = [
            (
                format!("{wrapper}HumanitZ/Content/Paks/ContainerCapacityUI_P.pak"),
                b"ui pak".as_slice(),
            ),
            (
                format!("{wrapper}HumanitZ/Content/Paks/ContainerCapacityUI_P.utoc"),
                b"ui toc",
            ),
            (
                format!("{wrapper}HumanitZ/Content/Paks/ContainerCapacityUI_P.ucas"),
                b"ui data",
            ),
            (
                format!("{wrapper}HumanitZ/Content/Paks/ContainerCapacityUI_P.sig"),
                b"ui signature",
            ),
            (
                format!(
                    "{wrapper}HumanitZ/Binaries/Win64/ue4ss/Mods/ContainerCapacityCustomizer/Scripts/main.lua"
                ),
                b"return 1",
            ),
            (
                format!(
                    "{wrapper}HumanitZ/Binaries/Win64/ue4ss/Mods/ContainerCapacityCustomizer/settings.json"
                ),
                br#"{"SlotsPerRow":5}"#,
            ),
        ];
        let borrowed = entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), *bytes))
            .collect::<Vec<_>>();
        let installed = fixture.install("humanitz", "107", &borrowed).unwrap();
        assert_eq!(installed.copied_file_count, 7);
        let root = fixture.0.join("runtime");
        let paths = [
            (
                "HumanitZServer/Content/Paks/ContainerCapacityUI_P.pak",
                b"ui pak".as_slice(),
            ),
            (
                "HumanitZServer/Content/Paks/ContainerCapacityUI_P.utoc",
                b"ui toc",
            ),
            (
                "HumanitZServer/Content/Paks/ContainerCapacityUI_P.ucas",
                b"ui data",
            ),
            (
                "HumanitZServer/Content/Paks/ContainerCapacityUI_P.sig",
                b"ui signature",
            ),
            (
                "HumanitZServer/Binaries/Win64/ue4ss/Mods/ContainerCapacityCustomizer/Scripts/main.lua",
                b"return 1",
            ),
            (
                "HumanitZServer/Binaries/Win64/ue4ss/Mods/ContainerCapacityCustomizer/enabled.txt",
                b"",
            ),
        ];
        let config =
            "HumanitZServer/Binaries/Win64/ue4ss/Mods/ContainerCapacityCustomizer/settings.json";
        for (path, bytes) in paths {
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes, "{path}");
        }
        assert_eq!(
            fs::read(root.join(config)).unwrap(),
            br#"{"SlotsPerRow":5}"#
        );
        assert!(!root.join("HumanitZ").exists());
        assert!(!root.join("Release").exists());
        assert!(
            !fixture
                .target("humanitz")
                .target_path
                .join("ContainerCapacityUI_P.pak")
                .exists()
        );
        let inventory = fixture.inventory("humanitz");
        assert_eq!(inventory.items.len(), 1);
        assert_eq!(inventory.items[0].file_count, 7);
        assert_eq!(
            inventory.items[0].display_name.as_deref(),
            Some("Verified project")
        );
        assert!(inventory.items[0].icon_url.is_some());

        // Lua code retains its exact-hash contract. Only recognized mutable
        // configuration participates in the existing configuration retention flow.
        let script = root.join(paths[4].0);
        fs::write(&script, b"operator script").unwrap();
        assert!(
            fixture
                .manage("humanitz", "107", "disable")
                .unwrap_err()
                .contains("modified")
        );
        assert_eq!(fs::read(&script).unwrap(), b"operator script");
        assert_eq!(fs::read(root.join(paths[0].0)).unwrap(), b"ui pak");
        fs::write(&script, b"return 1").unwrap();

        fs::write(root.join(config), br#"{"SlotsPerRow":7}"#).unwrap();
        fixture.manage("humanitz", "107", "disable").unwrap();
        for (path, _) in paths {
            assert!(!root.join(path).exists(), "{path}");
        }
        assert!(!root.join(config).exists());
        fixture.manage("humanitz", "107", "enable").unwrap();
        for (path, bytes) in paths {
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes, "{path}");
        }
        assert_eq!(
            fs::read(root.join(config)).unwrap(),
            br#"{"SlotsPerRow":7}"#
        );
        let deleted = fixture.manage("humanitz", "107", "delete").unwrap();
        for (path, _) in paths {
            assert!(!root.join(path).exists(), "{path}");
        }
        assert!(!root.join(config).exists());
        let preserved = PathBuf::from(deleted.preserved_path.unwrap());
        assert_eq!(
            fs::read(preserved.join("runtime").join(config)).unwrap(),
            br#"{"SlotsPerRow":7}"#
        );
        assert!(fixture.inventory("humanitz").items.is_empty());
    }
}

#[test]
fn nexus_humanitz_client_paks_in_bare_lua_wrapper_keep_their_own_container() {
    let fixture = Fixture::new();
    fixture
        .install(
            "humanitz",
            "46",
            &[
                ("Release/Scripts/main.lua", b"return 1"),
                ("Release/HumanitZ/Content/Paks/Mods/10x_EXP_P.pak", b"pak"),
                (
                    "Release/HumanitZ/Content/Paks/Mods/10x_EXP_P.sig",
                    b"signature",
                ),
            ],
        )
        .unwrap();
    let root = fixture.0.join("runtime/HumanitZServer");
    assert_eq!(
        fs::read(root.join("Content/Paks/Mods/10x_EXP_P.pak")).unwrap(),
        b"pak"
    );
    assert_eq!(
        fs::read(root.join("Content/Paks/Mods/10x_EXP_P.sig")).unwrap(),
        b"signature"
    );
    assert_eq!(
        fs::read(root.join("Binaries/Win64/ue4ss/Mods/Nexus_46/Scripts/main.lua")).unwrap(),
        b"return 1"
    );
    assert!(!root.join("Binaries/Win64/ue4ss/Mods/Release").exists());
    assert_eq!(fixture.inventory("humanitz").items[0].file_count, 4);
}

#[test]
fn nexus_humanitz_client_paks_preserve_complete_layout_and_existing_ownership() {
    let source = "HumanitZ/Content/Paks/Mods/10x_EXP_P.pak";
    let destination = "HumanitZServer/Content/Paks/Mods/10x_EXP_P.pak";
    for unsupported in [
        "HumanitZ/Content/Paks/Mods/Orphan.ucas",
        "HumanitZ/Content/Paks/Mods/RequiredConfig.txt",
        "HumanitZ/Binaries/Win64/ue4ss/UE4SS.dll",
        "AbioticFactor/Content/Paks/Foreign.pak",
        destination,
    ] {
        let fixture = Fixture::new();
        assert!(
            fixture
                .install(
                    "humanitz",
                    "46",
                    &[(source, b"incoming"), (unsupported, b"unsupported")]
                )
                .is_err(),
            "{unsupported}"
        );
        assert!(!fixture.0.join("runtime").join(destination).exists());
        assert!(fixture.inventory("humanitz").items.is_empty());
    }
    let foreign = BTreeMap::from([(source.to_owned(), "0".repeat(64))]);
    assert!(layout::plan(layout::Profile::AbioticFactor, &foreign, "46").is_err());

    let unowned = Fixture::new();
    let live = unowned.0.join("runtime").join(destination);
    fs::create_dir_all(live.parent().unwrap()).unwrap();
    fs::write(&live, b"operator pak").unwrap();
    assert!(
        unowned
            .install("humanitz", "46", &[(source, b"incoming")])
            .unwrap_err()
            .contains("unowned")
    );
    assert_eq!(fs::read(live).unwrap(), b"operator pak");

    let fixture = Fixture::new();
    fixture
        .install("humanitz", "46", &[(source, b"observed H46 layout")])
        .unwrap();
    let live = fixture.0.join("runtime").join(destination);
    assert_eq!(fs::read(&live).unwrap(), b"observed H46 layout");
    assert!(
        fixture
            .install("humanitz", "47", &[(destination, b"other project")])
            .unwrap_err()
            .contains("another Mod")
    );
    fs::write(&live, b"operator edit").unwrap();
    assert!(
        fixture
            .install("humanitz", "46", &[(source, b"replacement")])
            .unwrap_err()
            .contains("modified")
    );
    assert!(fixture.manage("humanitz", "46", "disable").is_err());
    assert_eq!(fs::read(live).unwrap(), b"operator edit");
    assert_eq!(fixture.inventory("humanitz").items.len(), 1);
}

#[test]
fn nexus_lua_and_paks_share_ownership_without_overwriting_native_control() {
    let fixture = Fixture::new();
    let module = "abioticfactor";
    let native = fixture
        .target(module)
        .target_path
        .join("LgsmWorldSave/Scripts/main.lua");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&native, b"owned native control").unwrap();
    fixture
        .install(
            module,
            "66",
            &[
                ("Content/Paks/LogicMods/Example.pak", b"pak"),
                ("ue4ss/Mods/Example/Scripts/main.lua", b"return 1"),
                ("ue4ss/Mods/Example/settings.json", b"{}"),
            ],
        )
        .unwrap();
    let runtime = fixture.0.join("runtime");
    let script = runtime.join("AbioticFactor/Binaries/Win64/ue4ss/Mods/Example/Scripts/main.lua");
    assert_eq!(fs::read(&script).unwrap(), b"return 1");
    let inventory = fixture.inventory(module);
    assert_eq!(inventory.items.len(), 2); // Existing native control and one Nexus package.
    assert_eq!(
        inventory
            .items
            .iter()
            .find(|item| item.management.is_some())
            .unwrap()
            .file_count,
        4
    );
    fixture.manage(module, "66", "disable").unwrap();
    assert!(!script.exists());
    assert!(!fixture.target(module).target_path.join("Example").exists());
    fixture.manage(module, "66", "enable").unwrap();
    let deleted = fixture.manage(module, "66", "delete").unwrap();
    assert!(!script.exists());
    assert_eq!(fs::read(native).unwrap(), b"owned native control");
    let preserved = PathBuf::from(deleted.preserved_path.unwrap());
    assert!(
        preserved
            .join("runtime/AbioticFactor/Binaries/Win64/ue4ss/Mods/Example/settings.json")
            .exists()
    );
    assert_eq!(deleted.inventory.items.len(), 1);
}

#[test]
fn nexus_route_updates_retire_old_files_and_refuse_modified_or_other_owned_files() {
    let fixture = Fixture::new();
    fixture
        .install("humanitz", "91", &[("Old.pak", b"old")])
        .unwrap();
    fixture
        .install("humanitz", "91", &[("New.pak", b"new")])
        .unwrap();
    let target = fixture.target("humanitz").target_path;
    assert!(!target.join("Old.pak").exists());
    assert_eq!(fs::read(target.join("New.pak")).unwrap(), b"new");
    assert!(
        fixture
            .install("humanitz", "92", &[("New.pak", b"other")])
            .unwrap_err()
            .contains("another Mod")
    );
    fs::write(target.join("New.pak"), b"operator").unwrap();
    assert!(
        fixture
            .install("humanitz", "91", &[("New.pak", b"replace")])
            .unwrap_err()
            .contains("modified")
    );
    assert!(fixture.manage("humanitz", "91", "disable").is_err());
    assert_eq!(fs::read(target.join("New.pak")).unwrap(), b"operator");
}

#[test]
fn nexus_routes_refuse_unowned_files_frameworks_wrong_targets_and_foreign_route_records() {
    let fixture = Fixture::new();
    let target = fixture.target("abioticfactor");
    let live = fixture
        .0
        .join("runtime/AbioticFactor/Content/Paks/Example.pak");
    fs::create_dir_all(live.parent().unwrap()).unwrap();
    fs::write(&live, b"unowned").unwrap();
    assert!(
        fixture
            .install("abioticfactor", "73", &[("Example.pak", b"incoming")])
            .unwrap_err()
            .contains("unowned")
    );
    assert_eq!(fs::read(live).unwrap(), b"unowned");
    assert!(
        fixture
            .install(
                "abioticfactor",
                "73",
                &[("ue4ss/UE4SS.dll", b"unknown loader")]
            )
            .is_err()
    );
    let mut wrong = target.clone();
    wrong.target_path = fixture.0.join("runtime/Mods");
    assert!(
        stage_nexus_routed_source(
            wrong,
            fixture.zip(&[("Example.pak", b"incoming")]),
            ManualModImportPresentation {
                display_name: None,
                icon_url: None
            },
            "73"
        )
        .is_err()
    );
    let package = InstalledPackage {
        display_name: None,
        icon_url: None,
        thunderstore_intent: None,
        target_name: "nexus-abioticfactor-73".into(),
        files: BTreeMap::new(),
        routed_files: BTreeMap::from([("BepInEx/patchers/Injected.dll".into(), "0".repeat(64))]),
        version: None,
        dependencies: None,
        enabled: true,
    };
    assert!(
        management::validate_management_manifest(&InstalledPackages {
            packages: [("nexus-abioticfactor-73".into(), package)].into()
        })
        .is_err()
    );
    assert!(routed::runtime_root(&target.target_path).is_err()); // Thunderstore remains BepInEx-only.
}

#[test]
fn nexus_route_management_rolls_back_every_file_when_manifest_publication_fails() {
    let fixture = Fixture::new();
    fixture
        .install(
            "humanitz",
            "91",
            &[("One.pak", b"one"), ("LogicMods/Two.pak", b"two")],
        )
        .unwrap();
    let before = fingerprint_tree(&fixture.0.join("runtime")).unwrap();
    management::storage::fail_next_publication();
    assert!(
        fixture
            .manage("humanitz", "91", "disable")
            .unwrap_err()
            .contains("rolled back")
    );
    let after = fingerprint_tree(&fixture.0.join("runtime")).unwrap();
    for (path, hash) in &before {
        assert_eq!(after.get(path), Some(hash), "{path}");
    }
    assert!(
        fixture.inventory("humanitz").items[0]
            .management
            .as_ref()
            .unwrap()
            .enabled
    );
}

#[test]
fn nexus_route_management_recovers_a_partially_moved_project() {
    let fixture = Fixture::new();
    let module = "humanitz";
    let key = "nexus-humanitz-91";
    fixture
        .install(
            module,
            "91",
            &[("One.pak", b"one"), ("LogicMods/Two.pak", b"two")],
        )
        .unwrap();
    fixture.manage(module, "91", "disable").unwrap();
    fixture.manage(module, "91", "enable").unwrap();
    let target = fixture.target(module);
    let parent = fixture.0.join("runtime/HumanitZServer/Content");
    let state = fs::read_dir(&parent)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .find(|path| path.join("owner.json").exists())
        .unwrap();
    let original = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    let record = serde_json::json!({
        "schema": 1, "key": key, "action": "disable", "retained_name": "interrupted",
        "original": original, "adopted": null, "had_payload": true, "payload_files": {},
    });
    fs::write(
        state.join("pending.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let relative = "HumanitZServer/Content/Paks/~mods/One.pak";
    let retained = state.join("disabled-routes").join(key).join(relative);
    fs::create_dir_all(retained.parent().unwrap()).unwrap();
    fs::rename(fixture.0.join("runtime").join(relative), retained).unwrap();
    management::recover_management(&target).unwrap();
    assert!(!state.join("pending.json").exists());
    let inventory = fixture.inventory(module);
    assert_eq!(inventory.items.len(), 1);
    assert_eq!(inventory.items[0].file_count, 2);
    assert!(!inventory.items[0].management.as_ref().unwrap().enabled);
    fixture.manage(module, "91", "enable").unwrap();
    assert_eq!(
        fs::read(fixture.0.join("runtime").join(relative)).unwrap(),
        b"one"
    );
}

#[test]
fn nexus_lua_enable_marker_is_owned_but_explicit_mod_list_entries_are_preserved() {
    for explicit in [None, Some("Example : 0\r\n")] {
        let fixture = Fixture::new();
        let target = fixture.target("abioticfactor");
        if let Some(content) = explicit {
            fs::create_dir_all(&target.target_path).unwrap();
            fs::write(target.target_path.join("mods.txt"), content).unwrap();
        }
        fixture
            .install(
                "abioticfactor",
                "66",
                &[("Example/Scripts/main.lua", b"return 1")],
            )
            .unwrap();
        let marker = target.target_path.join("Example/enabled.txt");
        assert_eq!(marker.exists(), explicit.is_none());
        if let Some(content) = explicit {
            assert_eq!(
                fs::read_to_string(target.target_path.join("mods.txt")).unwrap(),
                content
            );
        } else {
            let record = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
            assert!(
                record.packages["nexus-abioticfactor-66"]
                    .routed_files
                    .contains_key("AbioticFactor/Binaries/Win64/ue4ss/Mods/Example/enabled.txt")
            );
        }
        fixture.manage("abioticfactor", "66", "disable").unwrap();
        assert!(!marker.exists());
        fixture.manage("abioticfactor", "66", "enable").unwrap();
        assert_eq!(marker.exists(), explicit.is_none());
        let deleted = fixture.manage("abioticfactor", "66", "delete").unwrap();
        assert!(!marker.exists());
        assert!(deleted.preserved_path.is_none());
    }
}

#[test]
fn nexus_lua_dot_data_keeps_runtime_changes_through_management() {
    for (module, project) in [
        ("abioticfactor", "AbioticFactor"),
        ("humanitz", "HumanitZServer"),
    ] {
        for delete_disabled in [false, true] {
            let fixture = Fixture::new();
            let entries: &[(&str, &[u8])] = &[
                ("RandomDay/Scripts/main.lua", b"return 1"),
                ("RandomDay/.randomday_mode", b"day"),
            ];
            fixture.install(module, "212", entries).unwrap();
            let config = fixture.0.join(format!(
                "runtime/{project}/Binaries/Win64/ue4ss/Mods/RandomDay/.randomday_mode"
            ));
            fs::write(&config, b"night").unwrap();
            assert!(fixture.install(module, "212", entries).is_err());
            assert_eq!(fs::read(&config).unwrap(), b"night");
            fixture.manage(module, "212", "disable").unwrap();
            assert!(!config.exists());
            fixture.manage(module, "212", "enable").unwrap();
            assert_eq!(fs::read(&config).unwrap(), b"night");
            if delete_disabled {
                fixture.manage(module, "212", "disable").unwrap();
            }
            let deleted = fixture.manage(module, "212", "delete").unwrap();
            let relative = config.strip_prefix(fixture.0.join("runtime")).unwrap();
            let retained = PathBuf::from(deleted.preserved_path.unwrap());
            assert_eq!(
                fs::read(retained.join("runtime").join(relative)).unwrap(),
                b"night"
            );
            assert!(!config.exists());
            assert!(deleted.inventory.items.is_empty());
        }
    }
}

#[test]
fn nexus_bare_lua_package_keeps_paks_and_named_mods_in_their_own_containers() {
    for (module, project) in [
        ("abioticfactor", "AbioticFactor"),
        ("humanitz", "HumanitZServer"),
    ] {
        for wrapper in ["", "Release/"] {
            let fixture = Fixture::new();
            let entries = [
                (format!("{wrapper}Scripts/main.lua"), b"return 1".as_slice()),
                (format!("{wrapper}Scripts/config.lua"), b"return {}"),
                (
                    format!("{wrapper}Content/Paks/LogicMods/Example.pak"),
                    b"pak",
                ),
                (
                    format!("{wrapper}Content/Paks/LogicMods/Example.ucas"),
                    b"data",
                ),
                (format!("{wrapper}Other/Scripts/main.lua"), b"return 2"),
                ("README.txt".into(), b"instructions"),
            ];
            let borrowed = entries
                .iter()
                .map(|(name, bytes)| (name.as_str(), *bytes))
                .collect::<Vec<_>>();
            fixture.install(module, "39", &borrowed).unwrap();
            let target = fixture
                .0
                .join(format!("runtime/{project}/Binaries/Win64/ue4ss/Mods"));
            let scripts = target.join("Nexus_39/Scripts/main.lua");
            assert_eq!(fs::read(&scripts).unwrap(), b"return 1");
            assert_eq!(fs::read(target.join("Nexus_39/enabled.txt")).unwrap(), b"");
            assert_eq!(
                fs::read(target.join("Other/Scripts/main.lua")).unwrap(),
                b"return 2"
            );
            let pak = fixture.0.join(format!(
                "runtime/{project}/Content/Paks/LogicMods/Example.pak"
            ));
            assert_eq!(fs::read(&pak).unwrap(), b"pak");
            assert!(!target.join("Nexus_39/Content").exists());
            let inventory = fixture.inventory(module);
            assert_eq!(inventory.items.len(), 1);
            assert_eq!(
                inventory.items[0].display_name.as_deref(),
                Some("Verified project")
            );
            assert!(inventory.items[0].icon_url.is_some());
            fixture.manage(module, "39", "disable").unwrap();
            assert!(!scripts.exists());
            assert!(!pak.exists());
            fixture.manage(module, "39", "enable").unwrap();
            assert_eq!(fs::read(&scripts).unwrap(), b"return 1");
            assert_eq!(fs::read(&pak).unwrap(), b"pak");
            fixture.manage(module, "39", "delete").unwrap();
            assert!(!scripts.exists());
            assert!(!pak.exists());
            assert!(fixture.inventory(module).items.is_empty());
        }
    }
}

#[test]
fn nexus_bare_lua_folders_are_stable_and_distinct_per_project() {
    let fixture = Fixture::new();
    fixture
        .install("abioticfactor", "39", &[("Scripts/main.lua", b"return 1")])
        .unwrap();
    fixture
        .install("abioticfactor", "40", &[("Scripts/main.lua", b"return 2")])
        .unwrap();
    fixture
        .install("abioticfactor", "39", &[("Scripts/main.lua", b"return 3")])
        .unwrap();
    let target = fixture.target("abioticfactor").target_path;
    assert_eq!(
        fs::read(target.join("Nexus_39/Scripts/main.lua")).unwrap(),
        b"return 3"
    );
    assert_eq!(
        fs::read(target.join("Nexus_40/Scripts/main.lua")).unwrap(),
        b"return 2"
    );
    assert_eq!(fixture.inventory("abioticfactor").items.len(), 2);
}

#[test]
fn nexus_named_lua_mod_retains_its_name_and_non_container_pak_resources() {
    let fixture = Fixture::new();
    fixture
        .install(
            "abioticfactor",
            "39",
            &[
                ("NameDependent/Scripts/main.lua", b"return 1"),
                ("NameDependent/Data/Cache.pak", b"internal data"),
            ],
        )
        .unwrap();
    let target = fixture.target("abioticfactor").target_path;
    assert_eq!(
        fs::read(target.join("NameDependent/Scripts/main.lua")).unwrap(),
        b"return 1"
    );
    assert_eq!(
        fs::read(target.join("NameDependent/Data/Cache.pak")).unwrap(),
        b"internal data"
    );
    assert!(!target.join("Nexus_39").exists());
    assert!(
        !fixture
            .0
            .join("runtime/AbioticFactor/Content/Paks/Cache.pak")
            .exists()
    );
    fixture.manage("abioticfactor", "39", "disable").unwrap();
    fixture.manage("abioticfactor", "39", "enable").unwrap();
    assert_eq!(
        fs::read(target.join("NameDependent/Data/Cache.pak")).unwrap(),
        b"internal data"
    );
}

#[test]
fn nexus_bare_lua_does_not_reinterpret_frameworks_foreign_paks_or_protected_mods() {
    for unsupported in [
        "UE4SS.dll",
        "ue4ss/UE4SS.dll",
        "HumanitZ/Content/Paks/Client.pak",
        "Orphan.ucas",
        ".private/settings",
        "Mods/LgsmWorldSave/Scripts/main.lua",
        "LgsmWorldSave/Scripts/main.lua",
        "nexus-abioticfactor-99/payload",
    ] {
        let fixture = Fixture::new();
        assert!(
            fixture
                .install(
                    "abioticfactor",
                    "39",
                    &[("Scripts/main.lua", b"return 1"), (unsupported, b"unknown")],
                )
                .is_err(),
            "{unsupported}"
        );
        assert!(fixture.inventory("abioticfactor").items.is_empty());
    }
}

#[test]
fn nexus_lua_dot_files_do_not_admit_hidden_directories_or_change_code_ownership() {
    for file in ["Example/.private/settings", "Example/.loader.dll"] {
        let fixture = Fixture::new();
        assert!(
            fixture
                .install(
                    "abioticfactor",
                    "212",
                    &[
                        ("Example/Scripts/main.lua", b"return 1"),
                        (file, b"payload")
                    ],
                )
                .is_err()
        );
        assert!(fixture.inventory("abioticfactor").items.is_empty());
    }
    let fixture = Fixture::new();
    fixture
        .install(
            "abioticfactor",
            "212",
            &[
                ("Example/Scripts/main.lua", b"return 1"),
                ("Example/Scripts/.module.lua", b"return 2"),
            ],
        )
        .unwrap();
    let code = fixture
        .target("abioticfactor")
        .target_path
        .join("Example/Scripts/.module.lua");
    fs::write(&code, b"operator code").unwrap();
    assert!(fixture.manage("abioticfactor", "212", "disable").is_err());
    assert_eq!(fs::read(code).unwrap(), b"operator code");
}

#[test]
fn nexus_routes_migrate_only_complete_unchanged_local_ownership() {
    for modified in [false, true] {
        let fixture = Fixture::new();
        let target = fixture.target("abioticfactor");
        let local = fixture.0.join("Example");
        fs::create_dir_all(local.join("Scripts")).unwrap();
        fs::write(local.join("Scripts/main.lua"), b"return 1").unwrap();
        fs::write(local.join("enabled.txt"), b"").unwrap();
        crate::commands::commands_mod_staging::stage_manual_mod_sources(
            target.clone(),
            vec![local],
        )
        .unwrap();
        let script = target.target_path.join("Example/Scripts/main.lua");
        if modified {
            fs::write(&script, b"operator").unwrap();
        }
        let result = fixture.install(
            "abioticfactor",
            "41",
            &[("Example/Scripts/main.lua", b"return 1")],
        );
        let record = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        if modified {
            assert!(result.unwrap_err().contains("every recorded byte"));
            assert!(record.packages.contains_key("local-Example"));
            assert!(!record.packages.contains_key("nexus-abioticfactor-41"));
            assert_eq!(fs::read(script).unwrap(), b"operator");
        } else {
            result.unwrap();
            assert!(!record.packages.contains_key("local-Example"));
            assert_eq!(
                record.packages["nexus-abioticfactor-41"].routed_files.len(),
                2
            );
            assert_eq!(fixture.inventory("abioticfactor").items.len(), 1);
            fixture.manage("abioticfactor", "41", "delete").unwrap();
            assert!(!script.exists());
        }
    }
}

#[test]
fn nexus_routes_migrate_a_byte_identical_old_pak_from_the_wrong_module_container() {
    let fixture = Fixture::new();
    let target = fixture.target("abioticfactor");
    let local = fixture.0.join("Example.pak");
    fs::write(&local, b"pak bytes").unwrap();
    crate::commands::commands_mod_staging::stage_manual_mod_sources(target.clone(), vec![local])
        .unwrap();
    assert!(target.target_path.join("Example.pak").exists());
    fixture
        .install("abioticfactor", "73", &[("Example.pak", b"pak bytes")])
        .unwrap();
    assert!(!target.target_path.join("Example.pak").exists());
    assert_eq!(
        fs::read(
            fixture
                .0
                .join("runtime/AbioticFactor/Content/Paks/Example.pak")
        )
        .unwrap(),
        b"pak bytes"
    );
    let record = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    assert_eq!(record.packages.len(), 1);
    assert!(record.packages.contains_key("nexus-abioticfactor-73"));
}

#[test]
fn nexus_enshrouded_reviewed_proxy_pair_has_full_lifecycle_without_installing_selftest() {
    let fixture = Fixture::new();
    let result = fixture
        .install(
            "enshrouded",
            "178",
            &[
                ("XPShare/powrprof.dll", b"reviewed proxy payload"),
                ("XPShare/xpshare.ini", b"enabled=true"),
                (
                    "XPShare/xpshare_selftest.exe",
                    b"optional executable must not be installed",
                ),
            ],
        )
        .unwrap();
    let runtime = fixture.0.join("runtime");
    assert_eq!(result.copied_file_count, 2);
    assert_eq!(
        fs::read(runtime.join("powrprof.dll")).unwrap(),
        b"reviewed proxy payload"
    );
    assert!(
        !fingerprint_tree(&runtime)
            .unwrap()
            .keys()
            .any(|path| path.ends_with(".exe"))
    );
    let item = fixture.inventory("enshrouded").items.remove(0);
    assert_eq!(item.file_count, 2);
    assert_eq!(item.total_bytes, 34);
    fixture.manage("enshrouded", "178", "disable").unwrap();
    assert!(!runtime.join("powrprof.dll").exists());
    assert!(!runtime.join("xpshare.ini").exists());
    fixture.manage("enshrouded", "178", "enable").unwrap();
    assert_eq!(
        fs::read(runtime.join("xpshare.ini")).unwrap(),
        b"enabled=true"
    );
    let deleted = fixture.manage("enshrouded", "178", "delete").unwrap();
    assert!(!runtime.join("powrprof.dll").exists());
    assert!(!runtime.join("xpshare.ini").exists());
    assert!(deleted.inventory.items.is_empty());
    assert_eq!(
        fs::read(PathBuf::from(deleted.preserved_path.unwrap()).join("runtime/xpshare.ini"))
            .unwrap(),
        b"enabled=true"
    );
}

#[test]
fn nexus_enshrouded_root_routes_require_known_pairs_and_preserve_native_control() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fs::write(runtime.join("dbghelp.dll"), b"native control").unwrap();
    for files in [
        vec![("dbghelp.dll", b"replacement".as_slice())],
        vec![("version.dll", b"unknown root proxy".as_slice())],
        vec![("powrprof.dll", b"missing config".as_slice())],
        vec![("xpshare.ini", b"missing proxy".as_slice())],
    ] {
        assert!(fixture.install("enshrouded", "226", &files).is_err());
    }
    assert_eq!(
        fs::read(runtime.join("dbghelp.dll")).unwrap(),
        b"native control"
    );
    fixture
        .install(
            "enshrouded",
            "315",
            &[("mods/Example/Example.dll", b"explicit mod payload")],
        )
        .unwrap();
    assert_eq!(
        fs::read(runtime.join("mods/Example/Example.dll")).unwrap(),
        b"explicit mod payload"
    );
    assert_eq!(fixture.inventory("enshrouded").items.len(), 1);
    fixture.manage("enshrouded", "315", "delete").unwrap();
    assert!(!runtime.join("mods/Example").exists());
    fs::write(runtime.join("powrprof.dll"), b"existing framework").unwrap();
    assert!(
        fixture
            .install(
                "enshrouded",
                "178",
                &[("powrprof.dll", b"proxy"), ("xpshare.ini", b"config")]
            )
            .unwrap_err()
            .contains("unowned")
    );
    assert_eq!(
        fs::read(runtime.join("powrprof.dll")).unwrap(),
        b"existing framework"
    );
}

#[test]
fn nexus_enshrouded_eml_directory_preserves_the_observed_189_layout_and_legacy_ownership() {
    // Public Mod189's installed shape: mod.json, config.ini, README.md, src/mod.lua.
    for legacy in [false, true] {
        let fixture = Fixture::new();
        let target = fixture.target("enshrouded");
        let files: [(&str, &[u8]); 4] = [
            (
                "mod.json",
                br#"{"id":"quick-stack-range","version":"1.0.0"}"#,
            ),
            ("config.ini", b"Range=10"),
            ("README.md", b"Public package instructions"),
            ("src/mod.lua", b"return {name='quick-stack-range'}"),
        ];
        if legacy {
            let local = fixture.0.join("quick-stack-range");
            for (relative, bytes) in &files {
                let path = local.join(relative);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            crate::commands::commands_mod_staging::stage_manual_mod_sources(
                target.clone(),
                vec![local],
            )
            .unwrap();
        }
        let entries = files
            .iter()
            .map(|(relative, bytes)| (format!("release/quick-stack-range/{relative}"), *bytes))
            .collect::<Vec<_>>();
        fixture
            .install(
                "enshrouded",
                "189",
                &entries
                    .iter()
                    .map(|(name, bytes)| (name.as_str(), *bytes))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let live = target.target_path.join("quick-stack-range");
        for (relative, expected) in &files {
            assert_eq!(fs::read(live.join(relative)).unwrap(), *expected);
        }
        let inventory = fixture.inventory("enshrouded");
        assert_eq!(inventory.items.len(), 1);
        assert_eq!(inventory.items[0].file_count, 4);
        assert_eq!(
            inventory.items[0].management.as_ref().unwrap().package_id,
            "nexus-enshrouded-189"
        );
        let record = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        assert_eq!(record.packages.len(), 1);
        assert!(!record.packages.contains_key("local-quick-stack-range"));
        fixture.manage("enshrouded", "189", "disable").unwrap();
        assert!(!live.exists());
        fixture.manage("enshrouded", "189", "enable").unwrap();
        assert_eq!(fs::read(live.join("config.ini")).unwrap(), b"Range=10");
        let deleted = fixture.manage("enshrouded", "189", "delete").unwrap();
        assert_eq!(deleted.removed_file_count, 3);
        assert!(deleted.inventory.items.is_empty());
        assert!(!live.exists());
        assert_eq!(
            fs::read(
                PathBuf::from(deleted.preserved_path.unwrap())
                    .join("runtime/mods/quick-stack-range/config.ini")
            )
            .unwrap(),
            b"Range=10"
        );
    }
}

#[test]
fn nexus_enshrouded_eml_requires_a_manifest_entry_pair_and_valid_metadata() {
    let fixture = Fixture::new();
    for files in [
        vec![("Example/src/mod.lua", b"return 1".as_slice())],
        vec![("Example/mod.json", b"{}".as_slice())],
        vec![
            ("Example/mod.json", b"[]".as_slice()),
            ("Example/src/mod.lua", b"return 1".as_slice()),
        ],
    ] {
        assert!(fixture.install("enshrouded", "189", &files).is_err());
    }
    assert!(
        fingerprint_tree(&fixture.0.join("runtime"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn nexus_route_modified_configuration_survives_disable_enable_and_delete() {
    for delete_disabled in [false, true] {
        for eml in [false, true] {
            let fixture = Fixture::new();
            let (project, entries, config): (&str, Vec<(&str, &[u8])>, &str) = if eml {
                (
                    "189",
                    vec![
                        ("Example/mod.json", br#"{"id":"example"}"#),
                        ("Example/src/mod.lua", b"return 1"),
                        ("Example/config.ini", b"Range=10"),
                    ],
                    "mods/Example/config.ini",
                )
            } else {
                (
                    "178",
                    vec![("powrprof.dll", b"proxy"), ("xpshare.ini", b"enabled=true")],
                    "xpshare.ini",
                )
            };
            fixture.install("enshrouded", project, &entries).unwrap();
            let key = format!("nexus-enshrouded-{project}");
            let runtime = fixture.0.join("runtime");
            let manifest_path = fixture
                .target("enshrouded")
                .target_path
                .join(ONLINE_MOD_MANIFEST);
            let original = read_manifest(&manifest_path)
                .unwrap()
                .packages
                .remove(&key)
                .unwrap();
            fs::write(runtime.join(config), b"operator change while enabled").unwrap();
            fixture.manage("enshrouded", project, "disable").unwrap();
            let state = fixture.management_state("enshrouded");
            let disabled = state.join("disabled-routes").join(&key).join(config);
            assert_eq!(
                fs::read(&disabled).unwrap(),
                b"operator change while enabled"
            );
            assert!(!runtime.join(config).exists());
            fs::write(disabled, b"operator change while disabled").unwrap();
            fixture.manage("enshrouded", project, "enable").unwrap();
            assert_eq!(
                fs::read(runtime.join(config)).unwrap(),
                b"operator change while disabled"
            );
            let current = read_manifest(&manifest_path).unwrap();
            assert_eq!(current.packages[&key].routed_files, original.routed_files);
            fs::write(runtime.join(config), b"latest operator configuration").unwrap();
            if delete_disabled {
                fixture.manage("enshrouded", project, "disable").unwrap();
            }
            let deleted = fixture.manage("enshrouded", project, "delete").unwrap();
            let retained = PathBuf::from(deleted.preserved_path.unwrap()).join("runtime");
            assert_eq!(
                fs::read(retained.join(config)).unwrap(),
                b"latest operator configuration"
            );
            for relative in original.routed_files.keys() {
                assert!(!runtime.join(relative).exists(), "{relative}");
                if relative != config {
                    assert!(!retained.join(relative).exists(), "{relative}");
                }
            }
            assert_eq!(deleted.removed_file_count, original.routed_files.len() - 1);
            assert!(deleted.inventory.items.is_empty());
            assert!(read_manifest(&manifest_path).unwrap().packages.is_empty());
            assert!(!state.join("pending.json").exists());
        }
    }
}

#[test]
fn nexus_route_modified_configuration_rolls_back_with_its_current_bytes() {
    for action in ["disable", "enable", "delete"] {
        let fixture = Fixture::new();
        fixture
            .install(
                "enshrouded",
                "178",
                &[("powrprof.dll", b"proxy"), ("xpshare.ini", b"original")],
            )
            .unwrap();
        fixture.manage("enshrouded", "178", "disable").unwrap();
        if action != "enable" {
            fixture.manage("enshrouded", "178", "enable").unwrap();
        }
        let state = fixture.management_state("enshrouded");
        let source = if action == "enable" {
            state.join("disabled-routes/nexus-enshrouded-178")
        } else {
            fixture.0.join("runtime")
        };
        fs::write(
            source.join("xpshare.ini"),
            b"configuration at operation start",
        )
        .unwrap();
        let manifest_path = fixture
            .target("enshrouded")
            .target_path
            .join(ONLINE_MOD_MANIFEST);
        let original = serde_json::to_value(read_manifest(&manifest_path).unwrap()).unwrap();
        management::storage::fail_next_publication();
        assert!(
            fixture
                .manage("enshrouded", "178", action)
                .unwrap_err()
                .contains("rolled back")
        );
        assert_eq!(
            fs::read(source.join("xpshare.ini")).unwrap(),
            b"configuration at operation start"
        );
        assert_eq!(fs::read(source.join("powrprof.dll")).unwrap(), b"proxy");
        assert_eq!(
            serde_json::to_value(read_manifest(&manifest_path).unwrap()).unwrap(),
            original
        );
        assert!(!state.join("pending.json").exists());
        assert!(
            fixture.inventory("enshrouded").items[0]
                .management
                .as_ref()
                .unwrap()
                .enabled
                == (action != "enable")
        );
    }
}

#[test]
fn nexus_route_modified_configuration_recovers_from_either_side_of_a_partial_move() {
    for moved in ["xpshare.ini", "powrprof.dll"] {
        let fixture = Fixture::new();
        fixture
            .install(
                "enshrouded",
                "178",
                &[("powrprof.dll", b"proxy"), ("xpshare.ini", b"original")],
            )
            .unwrap();
        fixture.manage("enshrouded", "178", "disable").unwrap();
        fixture.manage("enshrouded", "178", "enable").unwrap();
        let runtime = fixture.0.join("runtime");
        fs::write(
            runtime.join("xpshare.ini"),
            b"configuration before interruption",
        )
        .unwrap();
        let snapshot = BTreeMap::from([(
            "xpshare.ini".into(),
            fingerprint_tree(&runtime.join("xpshare.ini"))
                .unwrap()
                .remove("")
                .unwrap(),
        )]);
        let state = fixture.write_disable_journal("enshrouded", "178", &snapshot);
        let retained = state.join("disabled-routes/nexus-enshrouded-178");
        fs::create_dir_all(&retained).unwrap();
        fs::rename(runtime.join(moved), retained.join(moved)).unwrap();
        management::recover_management(&fixture.target("enshrouded")).unwrap();
        assert_eq!(
            fs::read(retained.join("xpshare.ini")).unwrap(),
            b"configuration before interruption"
        );
        assert_eq!(fs::read(retained.join("powrprof.dll")).unwrap(), b"proxy");
        assert!(!state.join("pending.json").exists());
        assert!(
            !fixture.inventory("enshrouded").items[0]
                .management
                .as_ref()
                .unwrap()
                .enabled
        );
        fixture.manage("enshrouded", "178", "enable").unwrap();
        assert_eq!(
            fs::read(runtime.join("xpshare.ini")).unwrap(),
            b"configuration before interruption"
        );
    }
}

#[test]
fn nexus_route_configuration_snapshot_rejects_new_paths_payload_overrides_and_late_changes() {
    for (relative, hash) in [
        ("unowned.ini", "0".repeat(64)),
        ("powrprof.dll", "0".repeat(64)),
        ("xpshare.ini", "invalid".into()),
    ] {
        let fixture = Fixture::new();
        fixture
            .install(
                "enshrouded",
                "178",
                &[("powrprof.dll", b"proxy"), ("xpshare.ini", b"original")],
            )
            .unwrap();
        fixture.manage("enshrouded", "178", "disable").unwrap();
        fixture.manage("enshrouded", "178", "enable").unwrap();
        let runtime = fixture.0.join("runtime");
        let before = fingerprint_tree(&runtime).unwrap();
        let state = fixture.write_disable_journal(
            "enshrouded",
            "178",
            &BTreeMap::from([(relative.into(), hash)]),
        );
        assert!(
            management::recover_management(&fixture.target("enshrouded"))
                .unwrap_err()
                .contains("Invalid protected Mod configuration snapshot")
        );
        assert!(state.join("pending.json").exists());
        let after = fingerprint_tree(&runtime).unwrap();
        for (path, hash) in before {
            assert_eq!(after.get(&path), Some(&hash), "{path}");
        }
    }
    let fixture = Fixture::new();
    fixture
        .install(
            "enshrouded",
            "178",
            &[("powrprof.dll", b"proxy"), ("xpshare.ini", b"original")],
        )
        .unwrap();
    fixture.manage("enshrouded", "178", "disable").unwrap();
    fixture.manage("enshrouded", "178", "enable").unwrap();
    let config = fixture.0.join("runtime/xpshare.ini");
    let snapshot = BTreeMap::from([(
        "xpshare.ini".into(),
        fingerprint_tree(&config).unwrap().remove("").unwrap(),
    )]);
    let state = fixture.write_disable_journal("enshrouded", "178", &snapshot);
    fs::write(&config, b"changed after the durable snapshot").unwrap();
    assert!(
        management::recover_management(&fixture.target("enshrouded"))
            .unwrap_err()
            .contains("modified")
    );
    assert_eq!(
        fs::read(config).unwrap(),
        b"changed after the durable snapshot"
    );
    assert!(state.join("pending.json").exists());
}

#[test]
fn nexus_route_enable_markers_and_eml_metadata_keep_strict_payload_hashes() {
    let abiotic_entries: &[(&str, &[u8])] = &[
        ("Example/Scripts/main.lua", b"return 1"),
        ("Example/enabled.txt", b""),
    ];
    let enshrouded_entries: &[(&str, &[u8])] = &[
        ("Example/mod.json", br#"{"id":"example"}"#),
        ("Example/src/mod.lua", b"return 1"),
    ];
    let cases = [
        (
            "abioticfactor",
            "66",
            abiotic_entries,
            "AbioticFactor/Binaries/Win64/ue4ss/Mods/Example/enabled.txt",
        ),
        (
            "enshrouded",
            "189",
            enshrouded_entries,
            "mods/Example/mod.json",
        ),
    ];
    for (module, project, entries, relative) in cases {
        let fixture = Fixture::new();
        fixture.install(module, project, entries).unwrap();
        let live = fixture.0.join("runtime").join(relative);
        let original = fs::read(&live).unwrap();
        fs::write(&live, br#"{"id":"operator"}"#).unwrap();
        assert!(fixture.manage(module, project, "disable").is_err());
        assert!(fixture.manage(module, project, "delete").is_err());
        assert_eq!(fs::read(&live).unwrap(), br#"{"id":"operator"}"#);
        fs::write(live, original).unwrap();
        fixture.manage(module, project, "disable").unwrap();
        let disabled = fixture
            .management_state(module)
            .join("disabled-routes")
            .join(format!("nexus-{module}-{project}"))
            .join(relative);
        fs::write(&disabled, br#"{"id":"operator"}"#).unwrap();
        assert!(fixture.manage(module, project, "enable").is_err());
        assert_eq!(fs::read(disabled).unwrap(), br#"{"id":"operator"}"#);
        assert!(
            !fixture.inventory(module).items[0]
                .management
                .as_ref()
                .unwrap()
                .enabled
        );
    }
}

#[test]
fn nexus_route_shroudlink_plugins_keep_separate_identity_and_lifecycle() {
    let fixture = Fixture::new();
    let install = |id: &str, file: &str, title: &str| {
        crate::commands::commands_mod_staging::stage_website_mod_source(
            fixture.target("enshrouded"),
            fixture.zip(&[
                (file, title.as_bytes()),
                ("README-plugin.txt", b"Author instructions"),
            ]),
            ManualModImportPresentation {
                display_name: Some(title.into()),
                icon_url: Some(format!(
                    "https://staticdelivery.nexusmods.com/mods/5811/images/{id}/{id}-1.png"
                )),
            },
            Some(id),
        )
        .unwrap()
    };
    install("279", "shroudlink_mods/chestsort.dll", "Chestsort");
    install("280", "shroudlink_mods/assistants.dll", "Assistants");
    let runtime = fixture.0.join("runtime");
    let first = runtime.join("shroudlink_mods/chestsort.dll");
    let second = runtime.join("shroudlink_mods/assistants.dll");
    assert_eq!(fs::read(&first).unwrap(), b"Chestsort");
    assert_eq!(fs::read(&second).unwrap(), b"Assistants");
    assert!(!runtime.join("mods/shroudlink_mods").exists());
    let inventory = fixture.inventory("enshrouded");
    assert_eq!(inventory.items.len(), 2);
    for (id, title) in [("279", "Chestsort"), ("280", "Assistants")] {
        let item = inventory
            .items
            .iter()
            .find(|item| item.name == format!("nexus-enshrouded-{id}"))
            .unwrap();
        assert_eq!(item.display_name.as_deref(), Some(title));
        assert!(
            item.icon_url
                .as_deref()
                .unwrap()
                .contains(&format!("/{id}/"))
        );
        assert_eq!(item.file_count, 2);
    }
    fixture.manage("enshrouded", "279", "disable").unwrap();
    assert!(!first.exists());
    assert_eq!(fs::read(&second).unwrap(), b"Assistants");
    fixture.manage("enshrouded", "279", "enable").unwrap();
    assert_eq!(fs::read(&first).unwrap(), b"Chestsort");
    fixture.manage("enshrouded", "279", "delete").unwrap();
    assert!(!first.exists());
    assert_eq!(fs::read(&second).unwrap(), b"Assistants");
    let after = fixture.inventory("enshrouded");
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].display_name.as_deref(), Some("Assistants"));
    fixture.manage("enshrouded", "280", "delete").unwrap();
    assert!(!second.exists());
    assert!(fixture.inventory("enshrouded").items.is_empty());
}

#[test]
fn nexus_route_shroudlink_does_not_claim_core_or_unknown_runtime_files() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fs::write(runtime.join("steam_api64.dll"), b"official Steam library").unwrap();
    fs::write(runtime.join("dbghelp.dll"), b"LanGame native control").unwrap();
    for path in [
        "steam_api64.dll",
        "core/steam_api64.dll",
        "unknown.dll",
        "shroudlink_mods/nested/Plugin.dll",
        "shroudlink_mods/Plugin.exe",
        "shroudlink_mods/../steam_api64.dll",
    ] {
        assert!(
            fixture
                .install("enshrouded", "278", &[(path, b"not installed")])
                .is_err(),
            "{path}"
        );
        assert_eq!(
            fs::read(runtime.join("steam_api64.dll")).unwrap(),
            b"official Steam library"
        );
        assert_eq!(
            fs::read(runtime.join("dbghelp.dll")).unwrap(),
            b"LanGame native control"
        );
        assert!(fixture.inventory("enshrouded").items.is_empty());
    }
}

#[test]
fn nexus_emf_standalone_container_cannot_be_reinterpreted_as_generic_mods() {
    for wrapper in ["", "Release/"] {
        let fixture = Fixture::new();
        let manifest = format!("{wrapper}emf/mods/Example/mod.json");
        let plugin = format!("{wrapper}emf/mods/Example/Example.dll");
        let error = fixture
            .install(
                "enshrouded",
                "999",
                &[
                    (
                        manifest.as_str(),
                        br#"{"id":"Example","dll":"Example.dll"}"#,
                    ),
                    (plugin.as_str(), b"independent EMF plugin"),
                ],
            )
            .unwrap_err();
        assert!(error.contains("emf/mods/Example"), "{error}");
        let runtime = fixture.0.join("runtime");
        assert!(!runtime.join("mods/Example").exists());
        assert!(!runtime.join("emf/mods/Example").exists());
        assert!(fixture.inventory("enshrouded").items.is_empty());
    }
}

#[test]
fn nexus_named_lua_emf_mod_keeps_its_name_through_management() {
    for (module, project) in [
        ("abioticfactor", "AbioticFactor"),
        ("humanitz", "HumanitZServer"),
    ] {
        for wrapper in ["", "Release/"] {
            let fixture = Fixture::new();
            let source = format!("{wrapper}emf/Scripts/main.lua");
            fixture
                .install(module, "999", &[(source.as_str(), b"return 1")])
                .unwrap();
            let mods = fixture
                .0
                .join(format!("runtime/{project}/Binaries/Win64/ue4ss/Mods"));
            let script = mods.join("emf/Scripts/main.lua");
            assert_eq!(fs::read(&script).unwrap(), b"return 1");
            assert!(mods.join("emf/enabled.txt").is_file());
            assert!(!mods.join("Nexus_999").exists());
            assert_eq!(fixture.inventory(module).items.len(), 1);
            fixture.manage(module, "999", "disable").unwrap();
            assert!(!script.exists());
            fixture.manage(module, "999", "enable").unwrap();
            assert_eq!(fs::read(&script).unwrap(), b"return 1");
            fixture.manage(module, "999", "delete").unwrap();
            assert!(!mods.join("emf").exists());
            assert!(fixture.inventory(module).items.is_empty());
        }
    }
}

fn emf_release_files() -> Vec<(String, Vec<u8>)> {
    // Model the author's v0.6.6 package tree, including all 22 bundled Mods.
    // These are staging fixtures, not the downloaded author's binary bytes.
    let mut files = Vec::new();
    for path in [
        "dinput8.dll",
        "emf/emf_core.dll",
        "emf/emf_content.dll",
        "emf/emf_frametime.dll",
        "emf/known_tools.txt",
        "emf/fonts/Alegreya-Regular.ttf",
        "emf/fonts/Alegreya-Bold.ttf",
        "emf/fonts/CascadiaMono-Regular.ttf",
        "emf/vk/VkLayer_EMF_console.json",
        "emf/vk/VkLayer_EMF_frametime.json",
    ] {
        files.push((
            format!("EMF-0.6.6/game/{path}"),
            b"fixture runtime payload".to_vec(),
        ));
    }
    for id in [
        "companion",
        "quick_stack",
        "clock",
        "big_stacks",
        "loot_scale",
        "lua",
        "loot_magnet",
        "sleep",
        "herald",
        "teleports",
        "bulk_craft",
        "blessings",
        "villager_jobs",
        "raids",
    ] {
        files.push((
            format!("EMF-0.6.6/game/emf/mods/{id}/{id}.dll"),
            b"fixture Mod payload".to_vec(),
        ));
        files.push((
            format!("EMF-0.6.6/game/emf/mods/{id}/mod.json"),
            format!(r#"{{"id":"{id}","version":"1.0.0","enabled":false}}"#).into_bytes(),
        ));
    }
    for id in [
        "kill_counter",
        "life_skills",
        "champion",
        "bounty",
        "grave",
        "handover",
        "beastmaster",
        "necromancer",
    ] {
        files.push((
            format!("EMF-0.6.6/game/emf/mods/{id}/main.lua"),
            b"return {}".to_vec(),
        ));
        files.push((
            format!("EMF-0.6.6/game/emf/mods/{id}/mod.json"),
            format!(r#"{{"id":"{id}","version":"1.0.0","main":"main.lua","enabled":false}}"#)
                .into_bytes(),
        ));
    }
    for path in [
        "README.txt",
        "LICENSE.txt",
        "licenses/EML-LICENSE.txt",
        "docs/modding/README.md",
        "examples/lua/test/mod.json",
        "examples/lua/test/main.lua",
        "mods/clock/mod.json",
        "mods/clock/clock.cpp",
        "tools/emf_sigcheck.exe",
        "install.ps1",
        "game/emf/setup/install.ps1",
        "game/emf/tools/emf_mcp.py",
        "game/emf/EMF.exe",
        "game/emf/ui/index.html",
        "future-notes.bin",
    ] {
        files.push((
            format!("EMF-0.6.6/{path}"),
            b"auxiliary data, never run".to_vec(),
        ));
    }
    files
}

#[test]
fn nexus_emf_release_installs_all_bundled_mods_and_archives_auxiliary_with_full_lifecycle() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    for (path, bytes) in [
        ("dbghelp.dll", b"LanGame native control".as_slice()),
        (
            "emf/config/quick_stack.ini",
            b"operator settings".as_slice(),
        ),
        ("emf/profiles/operator.json", b"operator profile".as_slice()),
        ("emf/mods/Operator/Operator.dll", b"another Mod".as_slice()),
    ] {
        let path = runtime.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let files = emf_release_files();
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    let installed = fixture.install("enshrouded", "246", &entries).unwrap();
    let manifest_path = fixture
        .target("enshrouded")
        .target_path
        .join(ONLINE_MOD_MANIFEST);
    let manifest = read_manifest(&manifest_path).unwrap();
    let package = &manifest.packages["nexus-enshrouded-246"];
    assert_eq!(package.routed_files.len(), 54);
    assert_eq!(
        package
            .routed_files
            .keys()
            .filter(|path| path.ends_with("/mod.json"))
            .count(),
        22
    );
    assert_eq!(package.files.len(), 1);
    assert_eq!(installed.copied_file_count, 55);
    let anchor = fixture
        .target("enshrouded")
        .target_path
        .join("nexus-enshrouded-246");
    let auxiliary = anchor.join("emf-package-auxiliary.zip.lgsm");
    assert_eq!(fingerprint_tree(&anchor).unwrap(), package.files);
    for (path, hash) in &package.routed_files {
        assert_eq!(
            fingerprint_tree(&runtime.join(path)).unwrap().get(""),
            Some(hash),
            "{path}"
        );
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(&auxiliary).unwrap()).unwrap();
    assert_eq!(archive.len(), files.len() - 54);
    for path in [
        "docs/modding/README.md",
        "examples/lua/test/mod.json",
        "game/emf/setup/install.ps1",
        "game/emf/tools/emf_mcp.py",
        "future-notes.bin",
    ] {
        let mut bytes = Vec::new();
        archive
            .by_name(&format!("EMF-0.6.6/{path}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"auxiliary data, never run");
    }
    drop(archive);
    assert!(!runtime.join("emf/setup").exists());
    assert!(!runtime.join("emf/tools").exists());
    assert!(!runtime.join("emf/EMF.exe").exists());
    assert!(
        fingerprint_tree(&anchor)
            .unwrap()
            .keys()
            .all(|path| path.ends_with(".lgsm"))
    );
    assert_eq!(fixture.inventory("enshrouded").items[0].file_count, 55);
    fs::write(
        runtime.join("emf/config/quick_stack.ini"),
        b"latest operator settings",
    )
    .unwrap();
    fixture.install("enshrouded", "246", &entries).unwrap();
    fixture.manage("enshrouded", "246", "disable").unwrap();
    assert!(!runtime.join("dinput8.dll").exists());
    assert!(!runtime.join("emf/emf_core.dll").exists());
    assert!(!runtime.join("emf/mods/lua/lua.dll").exists());
    assert_eq!(
        fs::read(runtime.join("emf/config/quick_stack.ini")).unwrap(),
        b"latest operator settings"
    );
    fixture.manage("enshrouded", "246", "enable").unwrap();
    assert!(runtime.join("emf/mods/lua/lua.dll").is_file());
    assert!(auxiliary.is_file());
    let deleted = fixture.manage("enshrouded", "246", "delete").unwrap();
    assert!(deleted.inventory.items.is_empty());
    assert!(!anchor.exists());
    assert!(!runtime.join("dinput8.dll").exists());
    assert!(!runtime.join("emf/mods/lua").exists());
    assert_eq!(
        fs::read(runtime.join("emf/config/quick_stack.ini")).unwrap(),
        b"latest operator settings"
    );
    assert_eq!(
        fs::read(runtime.join("emf/profiles/operator.json")).unwrap(),
        b"operator profile"
    );
    assert_eq!(
        fs::read(runtime.join("emf/mods/Operator/Operator.dll")).unwrap(),
        b"another Mod"
    );
    assert_eq!(
        fs::read(runtime.join("dbghelp.dll")).unwrap(),
        b"LanGame native control"
    );
}

#[test]
fn nexus_emf_refuses_partial_payloads_unknown_game_files_and_missing_declared_entries() {
    for missing in [
        "/dinput8.dll",
        "/emf/emf_core.dll",
        "/emf/mods/lua/lua.dll",
        "/emf/mods/beastmaster/main.lua",
    ] {
        let fixture = Fixture::new();
        let files = emf_release_files()
            .into_iter()
            .filter(|(path, _)| !path.ends_with(missing))
            .collect::<Vec<_>>();
        let entries = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        assert!(
            fixture.install("enshrouded", "246", &entries).is_err(),
            "{missing}"
        );
        assert!(!fixture.0.join("runtime/dinput8.dll").exists());
        assert!(fixture.inventory("enshrouded").items.is_empty());
    }
    for unexpected in [
        "game/steam_api64.dll",
        "game/dbghelp.dll",
        "game/emf/unknown.dll",
        "game/emf/installed.txt",
        "game/emf/mods/lua/run.ps1",
    ] {
        let fixture = Fixture::new();
        let mut files = emf_release_files();
        files.push((format!("EMF-0.6.6/{unexpected}"), b"not installed".to_vec()));
        let entries = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        assert!(
            fixture.install("enshrouded", "246", &entries).is_err(),
            "{unexpected}"
        );
        assert!(!fixture.0.join("runtime/dinput8.dll").exists());
    }
}

#[test]
fn nexus_emf_preserves_manual_installations_configs_and_other_loader_ownership() {
    let files = emf_release_files();
    let entries = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    for conflict in [
        "emf/unknown.dll",
        "emf/installed.txt",
        "emf/moved.txt",
        "dinput8-chain.ini",
        "dinput8.dll",
    ] {
        let fixture = Fixture::new();
        let live = fixture.0.join("runtime").join(conflict);
        fs::create_dir_all(live.parent().unwrap()).unwrap();
        fs::write(&live, b"existing operator data").unwrap();
        assert!(
            fixture.install("enshrouded", "246", &entries).is_err(),
            "{conflict}"
        );
        assert_eq!(fs::read(&live).unwrap(), b"existing operator data");
        assert!(!fixture.0.join("runtime/emf/emf_core.dll").exists());
    }
    let fixture = Fixture::new();
    fixture
        .install(
            "enshrouded",
            "236",
            &[("dinput8.dll", b"another project's loader")],
        )
        .unwrap();
    assert!(
        fixture
            .install("enshrouded", "246", &entries)
            .unwrap_err()
            .contains("another Mod")
    );
    assert_eq!(
        fs::read(fixture.0.join("runtime/dinput8.dll")).unwrap(),
        b"another project's loader"
    );
    assert!(fixture.install("abioticfactor", "246", &entries).is_err());
}

#[test]
fn nexus_livemap_deploys_website_assets_and_preserves_configuration_and_discovered_data() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fs::create_dir_all(runtime.join("livemap")).unwrap();
    fs::write(runtime.join("dbghelp.dll"), b"native control").unwrap();
    fs::write(runtime.join("livemap.ini"), b"operator port and key").unwrap();
    fs::write(runtime.join("livemap/explored.json"), b"discovered world").unwrap();
    let entries: [(&str, &[u8]); 8] = [
        ("Release/iphlpapi.dll", b"server map proxy"),
        ("Release/livemap/index.html", b"<html>map</html>"),
        ("Release/livemap/maps.json", b"{\"maps\":[]}"),
        ("Release/livemap/pois.json.gz", b"compressed marker data"),
        ("Release/livemap/tiles/0_1.webp", b"map tile"),
        ("Release/livemap/fonts/alegreya.woff2", b"font"),
        ("Release/livemap/fonts/OFL.txt", b"font license"),
        ("Release/THIRD_PARTY_NOTICES.md", b"license notices"),
    ];
    let result = fixture.install("enshrouded", "209", &entries).unwrap();
    assert_eq!(result.copied_file_count, 8);
    assert_eq!(
        fs::read(runtime.join("iphlpapi.dll")).unwrap(),
        b"server map proxy"
    );
    assert_eq!(
        fs::read(runtime.join("livemap/tiles/0_1.webp")).unwrap(),
        b"map tile"
    );
    assert_eq!(fixture.inventory("enshrouded").items[0].file_count, 8);
    fs::write(runtime.join("livemap/maps.json"), b"operator map styles").unwrap();
    assert!(fixture.install("enshrouded", "209", &entries).is_err());
    fixture.manage("enshrouded", "209", "disable").unwrap();
    assert!(!runtime.join("iphlpapi.dll").exists());
    assert!(!runtime.join("livemap/index.html").exists());
    assert_eq!(
        fs::read(runtime.join("livemap/explored.json")).unwrap(),
        b"discovered world"
    );
    fixture.manage("enshrouded", "209", "enable").unwrap();
    assert_eq!(
        fs::read(runtime.join("livemap/maps.json")).unwrap(),
        b"operator map styles"
    );
    let deleted = fixture.manage("enshrouded", "209", "delete").unwrap();
    assert!(!runtime.join("iphlpapi.dll").exists());
    assert!(!runtime.join("livemap/index.html").exists());
    assert_eq!(
        fs::read(runtime.join("livemap.ini")).unwrap(),
        b"operator port and key"
    );
    assert_eq!(
        fs::read(runtime.join("livemap/explored.json")).unwrap(),
        b"discovered world"
    );
    assert_eq!(
        fs::read(runtime.join("dbghelp.dll")).unwrap(),
        b"native control"
    );
    let retained = PathBuf::from(deleted.preserved_path.unwrap());
    assert_eq!(
        fs::read(retained.join("runtime/livemap/maps.json")).unwrap(),
        b"operator map styles"
    );
}

#[test]
fn nexus_livemap_requires_the_proxy_and_site_together_and_preserves_unowned_proxies() {
    for entries in [
        vec![("iphlpapi.dll", b"proxy".as_slice())],
        vec![
            ("livemap/index.html", b"site".as_slice()),
            ("livemap/maps.json", b"{}".as_slice()),
        ],
        vec![
            ("iphlpapi.dll", b"proxy".as_slice()),
            ("livemap/index.html", b"site".as_slice()),
        ],
        vec![
            ("iphlpapi.dll", b"proxy".as_slice()),
            ("livemap/index.html", b"site".as_slice()),
            ("livemap/maps.json", b"{}".as_slice()),
            ("livemap/run.exe", b"not an asset".as_slice()),
        ],
    ] {
        let fixture = Fixture::new();
        assert!(fixture.install("enshrouded", "209", &entries).is_err());
        assert!(!fixture.0.join("runtime/iphlpapi.dll").exists());
    }
    let fixture = Fixture::new();
    let proxy = fixture.0.join("runtime/iphlpapi.dll");
    fs::write(&proxy, b"operator proxy").unwrap();
    assert!(
        fixture
            .install(
                "enshrouded",
                "209",
                &[
                    ("iphlpapi.dll", b"new"),
                    ("livemap/index.html", b"site"),
                    ("livemap/maps.json", b"{}")
                ]
            )
            .is_err()
    );
    assert_eq!(fs::read(proxy).unwrap(), b"operator proxy");
}

fn autoloot_files() -> [(&'static str, &'static [u8]); 6] {
    [
        ("AutolootCoop/powrprof.dll", b"current plugin loader"),
        (
            "AutolootCoop/plugins/AutolootCoop.dll",
            b"current Autoloot plugin",
        ),
        ("AutolootCoop/AutolootCoop.ini", b"Radius=15"),
        ("AutolootCoop/AutolootCoop-LICENSE.txt", b"license terms"),
        (
            "AutolootCoop/AutolootCoop-README.txt",
            b"installation instructions",
        ),
        (
            "AutolootCoop/AutolootCoop-LootList.txt",
            b"blacklist reference",
        ),
    ]
}

#[test]
fn nexus_enshrouded_autoloot_deploys_current_plugin_trio_and_keeps_all_reference_texts() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fs::write(runtime.join("dbghelp.dll"), b"native control").unwrap();
    let result = fixture
        .install("enshrouded", "266", &autoloot_files())
        .unwrap();
    assert_eq!(result.copied_file_count, 6);
    assert_eq!(
        fs::read(runtime.join("powrprof.dll")).unwrap(),
        b"current plugin loader"
    );
    assert_eq!(
        fs::read(runtime.join("plugins/AutolootCoop.dll")).unwrap(),
        b"current Autoloot plugin"
    );
    assert_eq!(
        fs::read(runtime.join("AutolootCoop.ini")).unwrap(),
        b"Radius=15"
    );
    let anchor = runtime.join("mods/nexus-enshrouded-266");
    for name in [
        "AutolootCoop-LICENSE.txt",
        "AutolootCoop-README.txt",
        "AutolootCoop-LootList.txt",
    ] {
        assert!(anchor.join(name).is_file());
        assert!(!runtime.join(name).exists());
    }
    assert_eq!(fixture.inventory("enshrouded").items[0].file_count, 6);
    assert_eq!(
        fs::read(runtime.join("dbghelp.dll")).unwrap(),
        b"native control"
    );
}

#[test]
fn nexus_enshrouded_autoloot_rejects_incomplete_or_other_plugins_and_loader_conflicts() {
    for missing in [
        "powrprof.dll",
        "plugins/AutolootCoop.dll",
        "AutolootCoop.ini",
    ] {
        let fixture = Fixture::new();
        let entries = autoloot_files()
            .into_iter()
            .filter(|(path, _)| !path.ends_with(missing))
            .collect::<Vec<_>>();
        assert!(
            fixture.install("enshrouded", "266", &entries).is_err(),
            "{missing}"
        );
        assert!(!fixture.0.join("runtime/powrprof.dll").exists());
    }
    let fixture = Fixture::new();
    assert!(
        fixture
            .install(
                "enshrouded",
                "266",
                &[
                    ("powrprof.dll", b"loader"),
                    ("plugins/Other.dll", b"unknown plugin"),
                    ("AutolootCoop.ini", b"Radius=15"),
                ]
            )
            .is_err()
    );
    fixture
        .install(
            "enshrouded",
            "178",
            &[
                ("powrprof.dll", b"XP loader"),
                ("xpshare.ini", b"XP config"),
            ],
        )
        .unwrap();
    assert!(
        fixture
            .install("enshrouded", "266", &autoloot_files())
            .unwrap_err()
            .contains("another Mod")
    );
    assert_eq!(
        fs::read(fixture.0.join("runtime/powrprof.dll")).unwrap(),
        b"XP loader"
    );
    let unowned = Fixture::new();
    fs::write(unowned.0.join("runtime/powrprof.dll"), b"operator loader").unwrap();
    assert!(
        unowned
            .install("enshrouded", "266", &autoloot_files())
            .is_err()
    );
    assert_eq!(
        fs::read(unowned.0.join("runtime/powrprof.dll")).unwrap(),
        b"operator loader"
    );
}

#[test]
fn nexus_enshrouded_autoloot_preserves_config_changes_through_disable_enable_and_delete() {
    let fixture = Fixture::new();
    let runtime = fixture.0.join("runtime");
    fixture
        .install("enshrouded", "266", &autoloot_files())
        .unwrap();
    fs::write(runtime.join("AutolootCoop.ini"), b"Radius=25").unwrap();
    assert!(
        fixture
            .install("enshrouded", "266", &autoloot_files())
            .is_err()
    );
    fixture.manage("enshrouded", "266", "disable").unwrap();
    assert!(!runtime.join("powrprof.dll").exists());
    assert!(!runtime.join("plugins/AutolootCoop.dll").exists());
    fixture.manage("enshrouded", "266", "enable").unwrap();
    assert_eq!(
        fs::read(runtime.join("AutolootCoop.ini")).unwrap(),
        b"Radius=25"
    );
    let deleted = fixture.manage("enshrouded", "266", "delete").unwrap();
    assert!(deleted.inventory.items.is_empty());
    assert!(!runtime.join("powrprof.dll").exists());
    assert!(!runtime.join("plugins/AutolootCoop.dll").exists());
    assert!(!runtime.join("AutolootCoop.ini").exists());
    assert_eq!(
        fs::read(PathBuf::from(deleted.preserved_path.unwrap()).join("runtime/AutolootCoop.ini"))
            .unwrap(),
        b"Radius=25"
    );
}

#[test]
fn nexus_humanitz_compound_readme_stays_metadata_without_absorbing_root_configuration() {
    let fixture = Fixture::new();
    fixture
        .install(
            "humanitz",
            "171",
            &[
                ("ServerBundle/NPCMechanics_Server_P.pak", b"server payload"),
                ("LEIA-ME_README.txt", b"author instructions"),
                ("NPCMechanics-LICENSE.md", b"license terms"),
            ],
        )
        .unwrap();
    let target = fixture.target("humanitz").target_path;
    assert_eq!(
        fs::read(target.join("NPCMechanics_Server_P.pak")).unwrap(),
        b"server payload"
    );
    let anchor = target.join("nexus-humanitz-171");
    assert_eq!(
        fs::read(anchor.join("LEIA-ME_README.txt")).unwrap(),
        b"author instructions"
    );
    assert_eq!(
        fs::read(anchor.join("NPCMechanics-LICENSE.md")).unwrap(),
        b"license terms"
    );
    assert!(!target.join("LEIA-ME_README.txt").exists());
    assert_eq!(fixture.inventory("humanitz").items[0].file_count, 3);
    for configuration in ["RequiredConfig.txt", "Config/Ordinary_README.txt"] {
        assert!(
            fixture
                .install(
                    "humanitz",
                    "172",
                    &[
                        ("Other_Server_P.pak", b"not published"),
                        (configuration, b"required unknown configuration"),
                    ]
                )
                .is_err(),
            "{configuration}"
        );
        assert!(!target.join("Other_Server_P.pak").exists());
    }
    assert_eq!(
        fs::read(target.join("NPCMechanics_Server_P.pak")).unwrap(),
        b"server payload"
    );
}

#[test]
fn nexus_owned_metadata_update_replaces_client_payload_and_unchanged_readme() {
    // Native H171's previous selection owns this client Pak and compound README.
    // Synthetic server bytes exercise real extraction, replacement and ownership.
    let fixture = Fixture::new();
    fixture
        .install(
            "humanitz",
            "171",
            &[
                ("NPCCommunityFix_Client_P.pak", b"previous client payload"),
                ("LEIA-ME_README.txt", b"previous author instructions"),
            ],
        )
        .unwrap();
    let target = fixture.target("humanitz").target_path;
    let client = target.join("NPCCommunityFix_Client_P.pak");
    let anchor = target.join("nexus-humanitz-171");
    let server = [
        (
            "HumanitZServer/Content/Paks/NPCCommunityFix_Server_P.pak",
            b"server pak".as_slice(),
        ),
        (
            "HumanitZServer/Content/Paks/NPCCommunityFix_Server_P.utoc",
            b"server toc".as_slice(),
        ),
        (
            "HumanitZServer/Content/Paks/NPCCommunityFix_Server_P.ucas",
            b"server data".as_slice(),
        ),
        (
            "LEIA-ME_README.txt",
            b"updated server installation instructions".as_slice(),
        ),
    ];
    let result = fixture.install("humanitz", "171", &server).unwrap();
    assert_eq!(result.copied_file_count, 4);
    assert!(!client.exists());
    assert_eq!(
        fs::read(anchor.join("LEIA-ME_README.txt")).unwrap(),
        server[3].1
    );
    let root = fixture.0.join("runtime");
    for (relative, bytes) in &server[..3] {
        assert_eq!(fs::read(root.join(relative)).unwrap(), *bytes);
    }
    let manifest = read_manifest(&target.join(ONLINE_MOD_MANIFEST)).unwrap();
    let owned = &manifest.packages["nexus-humanitz-171"];
    assert_eq!(owned.files, fingerprint_tree(&anchor).unwrap());
    assert_eq!(owned.routed_files.len(), 3);
    for (relative, _) in &server[..3] {
        assert_eq!(
            fingerprint_tree(&root.join(relative)).unwrap().get(""),
            owned.routed_files.get(*relative)
        );
    }
    assert!(
        !owned
            .routed_files
            .keys()
            .any(|path| path.contains("Client"))
    );
    fixture.manage("humanitz", "171", "disable").unwrap();
    assert!(
        server[..3]
            .iter()
            .all(|(relative, _)| !root.join(relative).exists())
    );
    fixture.manage("humanitz", "171", "enable").unwrap();
    assert!(!client.exists());
    for (relative, bytes) in &server[..3] {
        assert_eq!(fs::read(root.join(relative)).unwrap(), *bytes);
    }
    assert_eq!(fixture.inventory("humanitz").items[0].file_count, 4);
}

#[test]
fn nexus_owned_metadata_update_refuses_operator_modified_readme_without_retiring_client() {
    let fixture = Fixture::new();
    fixture
        .install(
            "humanitz",
            "171",
            &[
                ("NPCCommunityFix_Client_P.pak", b"previous client payload"),
                ("LEIA-ME_README.txt", b"previous author instructions"),
            ],
        )
        .unwrap();
    let target = fixture.target("humanitz").target_path;
    fs::write(
        target.join("nexus-humanitz-171/LEIA-ME_README.txt"),
        b"operator annotations",
    )
    .unwrap();
    let before = fingerprint_tree(&fixture.0.join("runtime")).unwrap();
    let error = fixture
        .install(
            "humanitz",
            "171",
            &[
                ("NPCCommunityFix_Server_P.pak", b"new server payload"),
                ("LEIA-ME_README.txt", b"new author instructions"),
            ],
        )
        .unwrap_err();
    assert!(error.contains("modified outside the manager"), "{error}");
    assert_eq!(
        fingerprint_tree(&fixture.0.join("runtime")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(target.join("NPCCommunityFix_Client_P.pak")).unwrap(),
        b"previous client payload"
    );
    assert!(!target.join("NPCCommunityFix_Server_P.pak").exists());
}

#[test]
fn nexus_owned_metadata_update_keeps_lua_configuration_and_nested_readme_protected() {
    // config.lua retains the existing exact-ownership rule for Lua source:
    // operator edits must stop an update. Nested text remains protected data
    // even when its name has a README suffix and its owned bytes are unchanged.
    for (file, modified) in [
        ("PWR/config.lua", true),
        ("PWR/Config/Ordinary_README.txt", false),
        ("PWR/Config/Ordinary_README.txt", true),
    ] {
        let fixture = Fixture::new();
        fixture
            .install(
                "humanitz",
                "40",
                &[
                    ("PWR/Scripts/main.lua", b"return 1"),
                    (file, b"original configuration"),
                    ("LEIA-ME_README.txt", b"old instructions"),
                ],
            )
            .unwrap();
        let live = fixture
            .0
            .join("runtime/HumanitZServer/Binaries/Win64/ue4ss/Mods")
            .join(file);
        if modified {
            fs::write(&live, b"operator configuration").unwrap();
        }
        let before = fingerprint_tree(&fixture.0.join("runtime")).unwrap();
        let error = fixture
            .install(
                "humanitz",
                "40",
                &[
                    ("PWR/Scripts/main.lua", b"return 2"),
                    (file, b"changed author configuration"),
                    ("LEIA-ME_README.txt", b"new instructions"),
                ],
            )
            .expect_err(&format!(
                "{file}, operator_modified={modified} must stop the update"
            ));
        assert!(
            error.contains(if modified {
                "modified"
            } else {
                "overwrite configuration"
            }),
            "{file}: {error}"
        );
        assert_eq!(
            fingerprint_tree(&fixture.0.join("runtime")).unwrap(),
            before
        );
        assert_eq!(
            fs::read(live).unwrap(),
            if modified {
                b"operator configuration".as_slice()
            } else {
                b"original configuration".as_slice()
            }
        );
    }
}

#[test]
fn nexus_owned_metadata_update_does_not_relax_other_provider_text_protection() {
    let fixture = Fixture::new();
    let target = ResolvedManualModTarget {
        instance_id: "other-provider-fixture".into(),
        module_id: "valheim".into(),
        source_label: "Thunderstore".into(),
        target_label: "BepInEx/plugins".into(),
        target_path: fixture.0.join("runtime/BepInEx/plugins"),
        accepts: vec!["folder".into()],
        id_strategy: None,
    };
    let source = |version: &str, readme: &[u8]| {
        let path = fixture.0.join(format!("other-provider-{version}"));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("Plugin.dll"), b"plugin payload").unwrap();
        fs::write(path.join("LEIA-ME_README.txt"), readme).unwrap();
        DownloadedManualModSource {
            path,
            label: "other provider".into(),
            identity: OnlineModIdentity {
                display_name: None,
                icon_url: None,
                thunderstore_intent: None,
                provider: "thunderstore".into(),
                project: "Author-Plugin".into(),
                historical_names: vec![],
                version: version.into(),
                dependencies: vec![],
            },
        }
    };
    stage_online_mod_sources(target.clone(), &[source("1", b"first text")]).unwrap();
    let before = fingerprint_tree(&fixture.0.join("runtime")).unwrap();
    let error = stage_online_mod_sources(target, &[source("2", b"replacement text")]).unwrap_err();
    assert!(error.contains("overwrite configuration"), "{error}");
    assert_eq!(
        fingerprint_tree(&fixture.0.join("runtime")).unwrap(),
        before
    );
}
