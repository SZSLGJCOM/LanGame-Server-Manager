use super::*;
use std::fs;
use std::io::Write;

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("langame-ue4ss-auto-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
fn profile() -> Profile {
    Profile::get("returntomoria").unwrap()
}

#[cfg(windows)]
#[tokio::test]
async fn bundled_runtime_prepares_every_supported_game_without_downloads() {
    for module in [
        "abioticfactor",
        "returntomoria",
        "nightingale",
        "conanexiles",
        "scum",
        "windrose",
    ] {
        let prepared = prepare(module).await.unwrap();
        assert_eq!(prepared.module_id, module);
        assert_eq!(hash(&prepared.files["ue4ss/UE4SS.dll"]), CORE_HASH);
        assert!(!prepared.files["ue4ss/LICENSE"].is_empty());
    }
}

fn payload(version: u8) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("ue4ss/UE4SS.dll".into(), vec![version; 20]),
        ("ue4ss/LICENSE".into(), b"MIT fixture".to_vec()),
        ("ue4ss/UE4SS_SDK_Backends/UE4SS.json".into(), b"{}".to_vec()),
        ("dwmapi.dll".into(), vec![version; 12]),
    ])
}
fn install(fixture: &Fixture, files: &BTreeMap<String, Vec<u8>>) -> Result<InstallOutcome, String> {
    installation::publish_fixture(&fixture.0, profile(), files, publish)
}

#[test]
fn save_image_admission_preserves_game_and_provider_pairing() {
    let current_conan = "d0ea317e7e037e68454414f12828d5995cc9e519b9c34eda264be1954d352b14";
    let current_scum = "070fcad918ba04eb9797db29674cadbd6691427b936fe354bca23b008c8c9971";
    assert_eq!(
        verify_game_image(
            "conanexiles",
            Profile::get("conanexiles").unwrap().game_hash
        )
        .unwrap(),
        Some("d076d5d223360f133e365ef37901ff9789eb68186a8f6d66494038f4864684b2")
    );
    assert_eq!(
        verify_game_image("conanexiles", current_conan).unwrap(),
        Some("2eec9aa9aa443fb476db583cfdc5044285ccc246df0a60e93155fb37e14324b6")
    );
    for image in [Profile::get("scum").unwrap().game_hash, current_scum] {
        assert_eq!(verify_game_image("scum", image).unwrap(), None);
    }
    for (module_id, image) in [
        ("conanexiles", current_scum),
        ("scum", current_conan),
        ("returntomoria", current_conan),
        ("conanexiles", "unreviewed-image"),
        ("palworld", current_conan),
    ] {
        assert!(verify_game_image(module_id, image).is_err(), "{module_id}");
    }
}

#[test]
fn first_install_activates_after_core_and_configuration_and_preserves_mods() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("ue4ss/Mods/OperatorMod")).unwrap();
    fs::write(
        fixture.0.join("ue4ss/Mods/OperatorMod/enabled.txt"),
        b"operator",
    )
    .unwrap();
    let mut commits = Vec::new();
    installation::publish_fixture(
        &fixture.0,
        profile(),
        &payload(1),
        |path, previous, bytes| {
            commits.push(
                path.strip_prefix(&fixture.0)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            publish(path, previous, bytes)
        },
    )
    .unwrap();
    assert_eq!(commits[commits.len() - 2], "dwmapi.dll");
    assert!(
        commits
            .iter()
            .position(|name| name == "ue4ss/UE4SS-settings.ini")
            .unwrap()
            < commits.len() - 2
    );
    assert_eq!(
        fixture.read("ue4ss/Mods/OperatorMod/enabled.txt"),
        b"operator"
    );
    assert!(!fixture.0.join("ue4ss/Mods/ConsoleEnablerMod").exists());
    let receipt: serde_json::Value =
        serde_json::from_slice(&fixture.read(".langame-ue4ss-loader.json")).unwrap();
    assert!(receipt["pending_files"].is_null());
}

#[test]
fn repeated_preparation_reuses_runtime_without_any_publication() {
    let fixture = Fixture::new();
    install(&fixture, &payload(1)).unwrap();
    let outcome = installation::publish_fixture(&fixture.0, profile(), &payload(1), |_, _, _| {
        panic!("A complete current loader must not be rewritten")
    })
    .unwrap();
    assert_eq!(outcome, InstallOutcome::Reused);
}

#[test]
fn missing_owned_runtime_member_is_repaired_without_replacing_present_files() {
    let fixture = Fixture::new();
    install(&fixture, &payload(1)).unwrap();
    fs::remove_file(fixture.0.join("ue4ss/UE4SS_SDK_Backends/UE4SS.json")).unwrap();
    let mut destinations = Vec::new();
    installation::publish_fixture(&fixture.0, profile(), &payload(1), |path, prior, bytes| {
        destinations.push(
            path.strip_prefix(&fixture.0)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        );
        publish(path, prior, bytes)
    })
    .unwrap();
    assert_eq!(fixture.read("ue4ss/UE4SS_SDK_Backends/UE4SS.json"), b"{}");
    assert_eq!(
        destinations,
        [
            ".langame-ue4ss-loader.json",
            "ue4ss/UE4SS_SDK_Backends/UE4SS.json",
            ".langame-ue4ss-loader.json"
        ]
    );
    assert_eq!(
        install(&fixture, &payload(1)).unwrap(),
        InstallOutcome::Reused
    );
}

#[test]
fn every_interrupted_publication_boundary_recovers_with_owned_cas() {
    for boundary in 1..=7 {
        let fixture = Fixture::new();
        let mut written = 0;
        let first = installation::publish_fixture(
            &fixture.0,
            profile(),
            &payload(1),
            |path, previous, bytes| {
                publish(path, previous, bytes)?;
                written += 1;
                if written == boundary {
                    return Err("simulated interruption after durable publication".into());
                }
                Ok(())
            },
        );
        assert!(first.is_err(), "boundary {boundary}");
        install(&fixture, &payload(1)).unwrap();
        assert_eq!(
            fixture.read("ue4ss/UE4SS.dll"),
            payload(1)["ue4ss/UE4SS.dll"]
        );
        assert_eq!(
            install(&fixture, &payload(1)).unwrap(),
            InstallOutcome::Reused
        );
    }
}

#[test]
fn interrupted_upgrade_preserves_actual_previous_intent_before_new_version() {
    let fixture = Fixture::new();
    install(&fixture, &payload(1)).unwrap();
    let mut wrote = 0;
    assert!(
        installation::publish_fixture(&fixture.0, profile(), &payload(2), |path, prior, bytes| {
            publish(path, prior, bytes)?;
            wrote += 1;
            if wrote == 2 {
                return Err("version two interrupted".into());
            }
            Ok(())
        })
        .is_err()
    );
    assert!(
        installation::publish_fixture(&fixture.0, profile(), &payload(3), |path, prior, bytes| {
            publish(path, prior, bytes)?;
            Err("version three interrupted after replacing intent".into())
        })
        .is_err()
    );
    install(&fixture, &payload(4)).unwrap();
    assert_eq!(
        fixture.read("ue4ss/UE4SS.dll"),
        payload(4)["ue4ss/UE4SS.dll"]
    );
}

#[test]
fn changed_owned_core_and_unknown_proxy_are_preserved_before_any_write() {
    let fixture = Fixture::new();
    install(&fixture, &payload(1)).unwrap();
    let receipt = fixture.read(".langame-ue4ss-loader.json");
    fs::write(fixture.0.join("ue4ss/UE4SS.dll"), b"operator changed").unwrap();
    let error = install(&fixture, &payload(2)).unwrap_err();
    assert!(error.contains("ue4ss/UE4SS.dll"));
    assert_eq!(fixture.read("ue4ss/UE4SS.dll"), b"operator changed");
    assert_eq!(fixture.read(".langame-ue4ss-loader.json"), receipt);
    let separate = Fixture::new();
    fs::write(separate.0.join("version.dll"), b"another loader").unwrap();
    assert!(
        install(&separate, &payload(1))
            .unwrap_err()
            .contains("version.dll")
    );
    assert!(!separate.0.join(".langame-ue4ss-loader.json").exists());
}

#[test]
fn concurrent_external_core_change_fails_cas_before_proxy_activation() {
    let fixture = Fixture::new();
    install(&fixture, &payload(1)).unwrap();
    let mut first = true;
    let result =
        installation::publish_fixture(&fixture.0, profile(), &payload(2), |path, prior, bytes| {
            publish(path, prior, bytes)?;
            if first {
                first = false;
                fs::write(
                    fixture.0.join("ue4ss/UE4SS.dll"),
                    b"concurrent external edit",
                )
                .unwrap();
            }
            Ok(())
        });
    assert!(result.is_err());
    assert_eq!(fixture.read("ue4ss/UE4SS.dll"), b"concurrent external edit");
    assert_eq!(fixture.read("dwmapi.dll"), payload(1)["dwmapi.dll"]);
}

#[test]
fn concurrent_settings_change_is_preserved_and_recovery_merges_new_custom_fields() {
    let fixture = Fixture::new();
    let mut first = true;
    let concurrent = b"[Operator]\nNewValue=keep after interrupted install\n";
    let result =
        installation::publish_fixture(&fixture.0, profile(), &payload(1), |path, prior, bytes| {
            publish(path, prior, bytes)?;
            if first {
                first = false;
                fs::create_dir_all(fixture.0.join("ue4ss")).unwrap();
                fs::write(fixture.0.join("ue4ss/UE4SS-settings.ini"), concurrent).unwrap();
            }
            Ok(())
        });
    assert!(result.is_err());
    assert_eq!(fixture.read("ue4ss/UE4SS-settings.ini"), concurrent);
    assert!(!fixture.0.join("dwmapi.dll").exists());
    install(&fixture, &payload(1)).unwrap();
    assert!(
        String::from_utf8(fixture.read("ue4ss/UE4SS-settings.ini"))
            .unwrap()
            .contains("NewValue=keep after interrupted install")
    );
}

#[test]
fn compatible_manual_runtime_is_reused_without_overwriting_native_bytes() {
    let fixture = Fixture::new();
    for (path, bytes) in payload(1) {
        fs::create_dir_all(fixture.0.join(&path).parent().unwrap()).unwrap();
        fs::write(fixture.0.join(path), bytes).unwrap();
    }
    fs::write(
        fixture.0.join("ue4ss/UE4SS-settings.ini"),
        profile().settings(),
    )
    .unwrap();
    assert_eq!(
        install(&fixture, &payload(1)).unwrap(),
        InstallOutcome::Reused
    );
    assert!(!fixture.0.join(".langame-ue4ss-loader.json").exists());
}

#[test]
fn settings_merge_retains_custom_values_comments_mods_and_required_dispatch() {
    let current = b"; operator comment\r\n[General]\r\nEnableHotReloadSystem = 1\r\nCustomOperatorValue = keep\r\n[Operator]\r\nCustom = untouched\r\n";
    let merged = profile::merge_settings(current, profile().settings()).unwrap();
    let text = std::str::from_utf8(&merged).unwrap();
    assert!(text.contains("; operator comment\r\n"));
    assert!(text.contains("CustomOperatorValue = keep\r\n"));
    assert!(text.contains("Custom = untouched\r\n"));
    assert!(text.contains("EnableHotReloadSystem = 0\r\n"));
    assert_eq!(
        profile::merge_settings(&merged, profile().settings()).unwrap(),
        merged
    );
}

#[test]
fn settings_ambiguity_never_publishes_a_loader_intent() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("ue4ss")).unwrap();
    let duplicate = b"[General]\nUseCache=0\nUseCache=1\n";
    fs::write(fixture.0.join("ue4ss/UE4SS-settings.ini"), duplicate).unwrap();
    assert!(
        install(&fixture, &payload(1))
            .unwrap_err()
            .contains("ambiguous")
    );
    assert_eq!(fixture.read("ue4ss/UE4SS-settings.ini"), duplicate);
    assert!(!fixture.0.join(".langame-ue4ss-loader.json").exists());
}

#[test]
fn unexpected_engine_override_is_preserved_and_rejected_before_publication() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("ue4ss")).unwrap();
    let override_ini = b"[EngineVersionOverride]\nMajorVersion=5\nMinorVersion=6\n";
    fs::write(fixture.0.join("ue4ss/UE4SS-settings.ini"), override_ini).unwrap();
    assert!(
        install(&fixture, &payload(1))
            .unwrap_err()
            .contains("engine-version override")
    );
    assert_eq!(fixture.read("ue4ss/UE4SS-settings.ini"), override_ini);
    assert!(!fixture.0.join(".langame-ue4ss-loader.json").exists());
    assert!(!compatible_settings(
        &format!(
            "{}\n{}",
            profile().settings(),
            std::str::from_utf8(override_ini).unwrap()
        ),
        profile().settings()
    ));
    let scum = Profile::get("scum").unwrap();
    assert!(compatible_settings(scum.settings(), scum.settings()));
    assert!(compatible_settings(
        profile().settings(),
        profile().settings()
    ));
    for suffix in [
        "MajorVersion=\nMinorVersion=\n",
        "MajorVersion=-1\nMinorVersion=-1\n",
    ] {
        assert!(compatible_settings(
            &format!(
                "{}\n[EngineVersionOverride]\n{suffix}",
                profile().settings()
            ),
            profile().settings()
        ));
    }
    for suffix in [
        "MajorVersion=0\nMinorVersion=0\n",
        "DebugBuild=true\n",
        "Stats=false\n",
    ] {
        assert!(!compatible_settings(
            &format!(
                "{}\n[EngineVersionOverride]\n{suffix}",
                profile().settings()
            ),
            profile().settings()
        ));
    }
}

#[test]
fn real_mod_search_path_overrides_are_preserved_before_any_loader_publication() {
    for setting in [
        "ModsFolderPath=../Other",
        "+ModsFolderPaths=../Other",
        "-ModsFolderPaths=Mods",
    ] {
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("ue4ss")).unwrap();
        let document = format!("[Overrides]\n{setting}\n");
        fs::write(fixture.0.join("ue4ss/UE4SS-settings.ini"), &document).unwrap();
        assert!(
            install(&fixture, &payload(1))
                .unwrap_err()
                .contains("mod-directory override")
        );
        assert_eq!(
            fixture.read("ue4ss/UE4SS-settings.ini"),
            document.as_bytes()
        );
        assert!(!fixture.0.join(".langame-ue4ss-loader.json").exists());
        assert!(!compatible_settings(
            &format!("{}\n{document}", profile().settings()),
            profile().settings()
        ));
    }
    // The fixed source still starts enabled.txt mods independently of its
    // controlling mods.txt list, so that operator setting does not redirect us.
    assert!(compatible_settings(
        &format!(
            "{}\n[Overrides]\nModsFolderPath=\nControllingModsTxt=operator-mods.txt\n",
            profile().settings()
        ),
        profile().settings()
    ));
}

#[test]
fn game_specific_native_config_directory_cannot_hide_the_owned_profile() {
    let fixture = Fixture::new();
    let win64 = profile().win64(&fixture.0.join("runtime"));
    let redirect = win64.join("ue4ss/runtime");
    fs::create_dir_all(&redirect).unwrap();
    fs::write(redirect.join("UE4SS-settings.ini"), b"operator profile").unwrap();
    assert!(
        installation::publish_fixture(&win64, profile(), &payload(1), publish)
            .unwrap_err()
            .contains("game-specific")
    );
    assert_eq!(
        fs::read(redirect.join("UE4SS-settings.ini")).unwrap(),
        b"operator profile"
    );
    assert!(!win64.join(".langame-ue4ss-loader.json").exists());
}

#[test]
fn rejects_relative_or_reparse_runtime_without_writing_through_it() {
    assert!(
        installation::publish_fixture(
            Path::new("relative-runtime"),
            profile(),
            &payload(1),
            publish
        )
        .is_err()
    );
    let fixture = Fixture::new();
    // An ordinary-file ancestor is also an unsafe path and exercises admission
    // on platforms where creating a directory symlink requires elevation.
    fs::write(fixture.0.join("not-a-directory"), b"untouched").unwrap();
    assert!(
        installation::publish_fixture(
            &fixture.0.join("not-a-directory/child"),
            profile(),
            &payload(1),
            publish
        )
        .is_err()
    );
    assert_eq!(fixture.read("not-a-directory"), b"untouched");
}

#[cfg(windows)]
#[test]
fn junction_to_shared_program_is_rejected_without_touching_its_files() {
    use std::os::windows::process::CommandExt;
    let private = Fixture::new();
    let shared = Fixture::new();
    fs::write(shared.0.join("shared-game.bin"), b"original shared bytes").unwrap();
    let link = private.0.join("shared-program-junction");
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&shared.0)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let rejected = installation::publish_fixture(&link, profile(), &payload(1), publish);
    fs::remove_dir(&link).unwrap();
    assert!(rejected.is_err());
    assert_eq!(shared.read("shared-game.bin"), b"original shared bytes");
    assert!(!shared.0.join(".langame-ue4ss-loader.json").exists());
}

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, value) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(value).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[test]
fn archive_checks_traversal_aliases_duplicates_and_digest_before_installation() {
    for path in [
        "../outside.dll",
        "/absolute.dll",
        "ue4ss/../outside.dll",
        "ue4ss\\UE4SS.dll",
        "C:/outside.dll",
        "ue4ss./UE4SS.dll",
    ] {
        assert!(
            archive::extract_fixture(&zip(&[(path, b"x")])).is_err(),
            "{path}"
        );
    }
    assert!(
        archive::extract_fixture(&zip(&[
            ("ue4ss/UE4SS.dll", b"one"),
            ("ue4ss/ue4ss.dll", b"two")
        ]))
        .is_err()
    );
    assert!(archive::prepare(profile(), &zip(&[("ue4ss/UE4SS.dll", b"untrusted")])).is_err());
}

#[test]
fn game_profiles_use_the_native_verified_import_proxies() {
    assert_eq!(Profile::get("abioticfactor").unwrap().proxy, "version.dll");
    assert_eq!(Profile::get("conanexiles").unwrap().proxy, "dsound.dll");
    for id in ["returntomoria", "nightingale", "scum", "windrose"] {
        assert_eq!(Profile::get(id).unwrap().proxy, "dwmapi.dll");
    }
    assert!(!supported("squad"));
    assert!(supported("abioticfactor"));
}

#[test]
fn abiotic_modding_update_admits_the_reviewed_image_and_rejects_image_changes() {
    let evidence: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../modules/abioticfactor/reference-fixtures/2026-10-27-save_factory_build_26092453.json"
    ))
    .unwrap();
    let current = evidence["executable_sha256"].as_str().unwrap();
    let profile = Profile::get("abioticfactor").unwrap();
    assert_eq!(profile.verify_game_image(current).unwrap(), None);
    let mut changed = current.as_bytes().to_vec();
    changed[0] = if changed[0] == b'0' { b'1' } else { b'0' };
    assert!(
        profile
            .verify_game_image(std::str::from_utf8(&changed).unwrap())
            .is_err()
    );
    assert!(
        profile
            .verify_game_image(&current[..current.len() - 1])
            .is_err()
    );
    assert_eq!(
        profile
            .verify_game_image("2563f8d798c8277400acd28eefde604c0ac7d77601627296211af48b30dba25f")
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn native_save_games_reject_unneeded_ue4ss_preparation() {
    let fixture = Fixture::new();
    for id in ["palworld", "astroneer", "humanitz"] {
        assert!(!supported(id), "{id}");
        assert!(Profile::get(id).is_err(), "{id}");
        assert!(installed(&fixture.0, id).is_err(), "{id}");
        assert!(prepare(id).await.is_err(), "{id}");
    }
    assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());
}

#[test]
fn unrelated_winmm_forwarding_proxy_is_preserved_before_loader_publication() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("winmm.dll"), b"external forwarding proxy").unwrap();
    assert!(
        installation::publish_fixture(&fixture.0, profile(), &payload(1), publish)
            .unwrap_err()
            .contains("Another loader")
    );
    assert_eq!(fixture.read("winmm.dll"), b"external forwarding proxy");
    assert!(!fixture.0.join(".langame-ue4ss-loader.json").exists());
}

#[test]
fn conan_layout_matches_the_native_fname_property_slot() {
    let conan = Profile::get("conanexiles").unwrap();
    let settings = crate::live_players::file_ipc::unreal_world_save::installation::profile_entries(
        conan.settings(),
    )
    .unwrap();
    // The fixed loader resolves layout inheritance before automatic version
    // detection. Without both overrides it treats FProperty as UField-derived.
    for (key, value) in [("MajorVersion", "5"), ("MinorVersion", "8")] {
        assert_eq!(
            settings.get(&("EngineVersionOverride".into(), key.into())),
            Some(&value.to_owned())
        );
    }
    let layout = include_bytes!("../../../../../modules/conanexiles/control/VTableLayout.ini");
    assert_eq!(
        hash(layout),
        "20bd7529e536c9267b60f8b13990785401bf3cb4c091d2f59dd94105ccba2c10"
    );
    assert_eq!(
        archive::expected(conan).get("ue4ss/VTableLayout.ini"),
        Some(&hash(layout))
    );
    let mut section = "";
    let mut fields = Vec::new();
    let mut properties = Vec::new();
    for line in std::str::from_utf8(layout).unwrap().lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
        } else if !line.is_empty() && !line.starts_with(';') {
            match section {
                "[FField]" => fields.push(line),
                "[FProperty]" => properties.push(line),
                _ => {}
            }
        }
    }
    // Both admitted Conan images put the native return-4 implementation at
    // slot 46. Slot 45 returns the vtable pointer, not an alignment.
    let slot = fields.len() - 1
        + properties
            .iter()
            .position(|name| *name == "GetMinAlignment")
            .unwrap();
    assert_eq!(slot, 46);
    for id in [
        "abioticfactor",
        "returntomoria",
        "nightingale",
        "scum",
        "windrose",
    ] {
        let other = Profile::get(id).unwrap();
        assert!(!archive::expected(other).contains_key("ue4ss/VTableLayout.ini"));
    }
}

#[test]
fn conan_layout_upgrade_is_owned_recoverable_and_preserves_custom_files() {
    let conan = Profile::get("conanexiles").unwrap();
    let layout = include_bytes!("../../../../../modules/conanexiles/control/VTableLayout.ini");
    let mut prior_files = payload(1);
    prior_files.remove("dwmapi.dll");
    prior_files.insert("dsound.dll".into(), vec![1; 12]);
    let mut updated = prior_files.clone();
    updated.insert("ue4ss/VTableLayout.ini".into(), layout.to_vec());
    for scenario in [
        "upgrade",
        "interrupted",
        "custom",
        "incomplete_receipt",
        "foreign_minor",
        "foreign_major",
        "foreign_debug",
        "foreign_stats",
    ] {
        let fixture = Fixture::new();
        installation::publish_fixture(&fixture.0, conan, &prior_files, publish).unwrap();
        let overrides = match scenario {
            "foreign_minor" => "\n[EngineVersionOverride]\nMajorVersion=5\nMinorVersion=6\n",
            "foreign_major" => "\n[EngineVersionOverride]\nMajorVersion=4\nMinorVersion=27\n",
            "foreign_debug" => "\n[EngineVersionOverride]\nDebugBuild=true\n",
            "foreign_stats" => "\n[EngineVersionOverride]\nStats=true\n",
            _ => "",
        };
        let prior_settings = format!("{}{overrides}", profile().settings());
        fs::write(fixture.0.join("ue4ss/UE4SS-settings.ini"), &prior_settings).unwrap();
        if scenario == "custom" {
            fs::write(fixture.0.join("ue4ss/VTableLayout.ini"), b"operator layout").unwrap();
        } else if scenario == "incomplete_receipt" {
            let mut receipt: serde_json::Value =
                serde_json::from_slice(&fixture.read(".langame-ue4ss-loader.json")).unwrap();
            receipt["files"]
                .as_object_mut()
                .unwrap()
                .remove("ue4ss/LICENSE");
            fs::write(
                fixture.0.join(".langame-ue4ss-loader.json"),
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
        }
        let prior_receipt = fixture.read(".langame-ue4ss-loader.json");
        let mut layout_written = false;
        let result =
            installation::publish_fixture(&fixture.0, conan, &updated, |path, previous, bytes| {
                if scenario == "interrupted" && layout_written {
                    return Err("publication interrupted after layout".into());
                }
                publish(path, previous, bytes)?;
                layout_written |= path == fixture.0.join("ue4ss/VTableLayout.ini");
                Ok(())
            });
        if matches!(scenario, "custom" | "incomplete_receipt") || scenario.starts_with("foreign_") {
            assert!(result.is_err(), "{scenario}");
            assert_eq!(fixture.read(".langame-ue4ss-loader.json"), prior_receipt);
            assert_eq!(
                fixture.read("ue4ss/UE4SS-settings.ini"),
                prior_settings.as_bytes()
            );
            if scenario == "custom" {
                assert_eq!(fixture.read("ue4ss/VTableLayout.ini"), b"operator layout");
            } else {
                assert!(!fixture.0.join("ue4ss/VTableLayout.ini").exists());
            }
            continue;
        }
        if scenario == "interrupted" {
            assert!(result.is_err());
            assert!(layout_written);
            assert_eq!(
                installation::publish_fixture(&fixture.0, conan, &updated, publish).unwrap(),
                InstallOutcome::Installed
            );
        } else {
            assert_eq!(result.unwrap(), InstallOutcome::Installed);
        }
        assert_eq!(fixture.read("ue4ss/VTableLayout.ini"), layout);
        assert!(compatible_settings(
            std::str::from_utf8(&fixture.read("ue4ss/UE4SS-settings.ini")).unwrap(),
            conan.settings()
        ));
        for (name, bytes) in &prior_files {
            assert_eq!(&fixture.read(name), bytes);
        }
        assert_eq!(
            installation::publish_fixture(&fixture.0, conan, &updated, |_, _, _| {
                panic!("A complete Conan layout must not be republished")
            })
            .unwrap(),
            InstallOutcome::Reused
        );
    }
}
