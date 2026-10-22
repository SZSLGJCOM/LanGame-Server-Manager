use super::*;
use crate::commands::commands_managed_save_control::{profile, tests::Fixture};

fn previous_vrising_observer(root: &Path) {
    let game = profile("vrising").unwrap();
    publish_payload(root, game, b"previous plugin".to_vec()).unwrap();
    let mut owner = inspect(root, game).unwrap().unwrap();
    for path in retired_paths(game) {
        std::fs::write(root.join(path), path.as_bytes()).unwrap();
        owner
            .files
            .insert((*path).into(), io::digest(path.as_bytes()));
    }
    owner.source_sha256 = "previous observer source".into();
    std::fs::write(root.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
}

#[test]
fn vrising_publication_retires_owned_observer_after_replacement_and_recovers_interruption() {
    for interrupted in [false, true] {
        let (fixture, game) = fixture("vrising");
        previous_vrising_observer(&fixture.0);
        if interrupted {
            assert!(
                publish_payload_with(
                    &fixture.0,
                    game,
                    b"new plugin".to_vec(),
                    |path, old, new| {
                        io::replace_owned(path, old, new)?;
                        Err("publication interrupted".into())
                    }
                )
                .is_err()
            );
            assert_eq!(
                std::fs::read(fixture.0.join(game.plugin)).unwrap(),
                b"new plugin"
            );
            assert!(
                retired_paths(game)
                    .iter()
                    .all(|path| fixture.0.join(path).is_file())
            );
        }
        publish_payload(&fixture.0, game, b"new plugin".to_vec()).unwrap();
        verify_prepared(&fixture.0, game).unwrap();
        assert!(
            retired_paths(game)
                .iter()
                .all(|path| !fixture.0.join(path).exists())
        );
        assert_eq!(inspect(&fixture.0, game).unwrap().unwrap().files.len(), 1);
    }
}

#[test]
fn vrising_observer_retirement_preserves_foreign_or_modified_sidecars_before_publication() {
    for owned in [false, true] {
        let (fixture, game) = fixture("vrising");
        if owned {
            previous_vrising_observer(&fixture.0);
        } else {
            publish_payload(&fixture.0, game, b"previous plugin".to_vec()).unwrap();
        }
        std::fs::write(fixture.0.join(retired_paths(game)[1]), b"operator file").unwrap();
        let before = owned_paths(game)
            .into_iter()
            .chain([OWNER])
            .map(|path| (path, std::fs::read(fixture.0.join(path)).ok()))
            .collect::<Vec<_>>();
        assert!(publish_payload(&fixture.0, game, b"new plugin".to_vec()).is_err());
        for (path, bytes) in before {
            assert_eq!(std::fs::read(fixture.0.join(path)).ok(), bytes);
        }
    }
}

#[tokio::test]
async fn unsupported_vrising_start_also_retires_the_owned_native_observer() {
    let fixture = Fixture::new();
    let (storage, details) = fixture.unsupported_instance("vrising");
    let runtime = fixture.0.join("runtime");
    previous_vrising_observer(&runtime);
    let receipt = std::fs::read(runtime.join(OWNER)).unwrap();
    let state = crate::state::DesktopState::default();
    let operation = state
        .begin_storage_context_operation("optional save test")
        .unwrap();
    assert!(
        super::super::ensure_for_start(&storage, &details, &operation)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        owned_paths(profile("vrising").unwrap())
            .iter()
            .all(|path| !runtime.join(path).exists())
    );
    assert_eq!(std::fs::read(runtime.join(OWNER)).unwrap(), receipt);
}

#[tokio::test]
async fn unsupported_managed_start_without_an_extension_needs_no_session() {
    for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
        let fixture = Fixture::new();
        let (storage, details) = fixture.unsupported_instance(module);
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("optional save test")
            .unwrap();
        let environment = super::super::ensure_for_start(&storage, &details, &operation)
            .await
            .unwrap_or_else(|error| {
                panic!("{module}: optional extension blocked startup: {error}")
            });
        assert_eq!(
            serde_json::to_value(environment).unwrap(),
            serde_json::Value::Null
        );
        let runtime = fixture.0.join("runtime");
        assert!(!runtime.join(OWNER).exists());
        assert!(!runtime.join(DIRECTORY).exists());
    }
}

#[tokio::test]
async fn unsupported_managed_start_retires_owned_scan_files_and_can_prepare_again() {
    for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
        let fixture = Fixture::new();
        let (storage, details) = fixture.unsupported_instance(module);
        let game = profile(module).unwrap();
        let runtime = fixture.0.join("runtime");
        publish_payload(&runtime, game, b"previous plugin".to_vec()).unwrap();
        let mut owner = decode(
            &io::read_regular(&runtime.join(OWNER), 65536).unwrap(),
            game,
        )
        .unwrap();
        owner
            .pending_files
            .insert(game.plugin.into(), io::digest(b"pending plugin"));
        std::fs::write(runtime.join(game.plugin), b"pending plugin").unwrap();
        let receipt = serde_json::to_vec(&owner).unwrap();
        std::fs::write(runtime.join(OWNER), &receipt).unwrap();
        let session = runtime.join(DIRECTORY).join("session.txt");
        std::fs::write(&session, b"existing session").unwrap();
        let foreign = runtime.join("operator-mod.dll");
        std::fs::write(&foreign, b"operator mod").unwrap();
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("optional save test")
            .unwrap();
        for _ in 0..2 {
            let environment = super::super::ensure_for_start(&storage, &details, &operation)
                .await
                .unwrap_or_else(|error| panic!("{module}: {error}"));
            assert_eq!(
                serde_json::to_value(environment).unwrap(),
                serde_json::Value::Null
            );
            for relative in paths(game) {
                assert!(!runtime.join(relative).exists(), "{module}: {relative}");
            }
            assert_eq!(std::fs::read(runtime.join(OWNER)).unwrap(), receipt);
            assert_eq!(std::fs::read(&session).unwrap(), b"existing session");
            assert_eq!(std::fs::read(&foreign).unwrap(), b"operator mod");
        }
        publish_payload(&runtime, game, b"new reviewed plugin".to_vec()).unwrap();
        verify_prepared(&runtime, game).unwrap();
        assert_eq!(
            std::fs::read(runtime.join(game.plugin)).unwrap(),
            b"new reviewed plugin"
        );
    }
}

#[tokio::test]
async fn unsupported_managed_start_preserves_conflicts_before_retiring_any_file() {
    for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
        for problem in ["foreign", "modified", "invalid_owner", "missing_native"] {
            let fixture = Fixture::new();
            let (storage, details) = fixture.unsupported_instance(module);
            let game = profile(module).unwrap();
            let runtime = fixture.0.join("runtime");
            if problem == "foreign" {
                let plugin = runtime.join(game.plugin);
                std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
                std::fs::write(plugin, b"operator mod").unwrap();
            } else {
                publish_payload(&runtime, game, b"owned plugin".to_vec()).unwrap();
                match problem {
                    "modified" => std::fs::write(
                        runtime.join(paths(game).last().unwrap()),
                        b"operator change",
                    )
                    .unwrap(),
                    "invalid_owner" => {
                        std::fs::write(runtime.join(OWNER), b"invalid receipt").unwrap()
                    }
                    _ => std::fs::remove_file(
                        runtime.join(native_files(game).unwrap().last().unwrap().0),
                    )
                    .unwrap(),
                }
            }
            let tracked = paths(game).into_iter().chain([OWNER]).collect::<Vec<_>>();
            let before = tracked
                .iter()
                .map(|path| std::fs::read(runtime.join(path)).ok())
                .collect::<Vec<_>>();
            let state = crate::state::DesktopState::default();
            let operation = state
                .begin_storage_context_operation("optional save test")
                .unwrap();
            assert!(
                super::super::ensure_for_start(&storage, &details, &operation)
                    .await
                    .is_err(),
                "{module}: {problem}"
            );
            assert_eq!(
                tracked
                    .iter()
                    .map(|path| std::fs::read(runtime.join(path)).ok())
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
}

#[test]
fn compiler_response_preserves_long_reference_lists_and_quoted_unicode_paths() {
    let game = profile("vrising").unwrap();
    let root = PathBuf::from(format!(
        "C:/operator games/存档/{}/runtime",
        "x".repeat(140)
    ));
    let references = (0..200)
        .map(|index| {
            (
                root.join(format!("dotnet/System.Reference{index}.dll")),
                "fixed hash".into(),
            )
        })
        .collect::<Vec<_>>();
    let scratch = PathBuf::from("C:/compiler output/保存");
    let bytes = compiler_response(
        Path::new("C:/Windows/Framework/csc.exe"),
        &scratch,
        game,
        &references,
    )
    .unwrap();
    assert!(bytes.len() > 32767);
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.starts_with('\u{feff}'));
    assert!(
        !text.contains("/noconfig"),
        "Framework csc requires this option on its command line"
    );
    for (reference, _) in references {
        assert!(text.contains(&format!("/reference:\"{}\"\r\n", reference.display())));
    }
    assert!(text.contains(&format!(
        "/out:\"{}\"\r\n",
        scratch.join("save.dll").display()
    )));
    assert!(text.contains(&format!(
        "\"{}\"\r\n",
        scratch.join("NativeSave.cs").display()
    )));
    assert!(text.ends_with(&format!(
        "\"{}\"\r\n",
        scratch.join("NativeSave.cs").display()
    )));
    let sons = compiler_response(
        Path::new("C:/Windows/Framework/csc.exe"),
        &scratch,
        profile("sonsoftheforest").unwrap(),
        &[],
    )
    .unwrap();
    let sons = std::str::from_utf8(&sons).unwrap();
    assert!(sons.ends_with(&format!(
        "\"{}\"\r\n",
        scratch.join("SonsSaveCompletion.cs").display()
    )));
}

#[test]
fn compiler_response_rejects_path_characters_that_could_inject_options() {
    for path in [
        "C:/path\n/unsafe.cs",
        "C:/path\r/unsafe.cs",
        "C:/path\"unsafe.cs",
        "C:/path\0unsafe.cs",
    ] {
        assert!(compiler_path(Path::new(path)).is_err(), "{path:?}");
    }
    assert_eq!(
        compiler_path(Path::new("C:/operator # notes/存档.dll")).unwrap(),
        "\"C:/operator # notes/存档.dll\""
    );
}

fn fixture(module: &str) -> (Fixture, Profile) {
    let root = Fixture::new();
    let game = profile(module).unwrap();
    let image = root.0.join(game.image);
    std::fs::create_dir_all(image.parent().unwrap()).unwrap();
    std::fs::write(image, b"test process admission image").unwrap();
    std::fs::create_dir(root.0.join("user-world")).unwrap();
    std::fs::write(root.0.join("user-world/persistent.sav"), b"existing world").unwrap();
    (root, game)
}

#[test]
fn automatic_first_publication_and_repeated_preparation_cover_every_managed_profile() {
    for module in ["corekeeper", "vrising", "sonsoftheforest", "barotrauma"] {
        let (root, game) = fixture(module);
        assert!(inspect(&root.0, game).unwrap().is_none());
        publish_payload(&root.0, game, b"compiled reviewed plugin".to_vec()).unwrap();
        let first = io::read_regular(&root.0.join(OWNER), 65536).unwrap();
        publish_payload(&root.0, game, b"compiled reviewed plugin".to_vec()).unwrap();
        assert_eq!(
            io::read_regular(&root.0.join(OWNER), 65536).unwrap(),
            first,
            "{module}"
        );
        assert!(verify_prepared(&root.0, game).is_ok(), "{module}");
        assert_eq!(
            std::fs::read(root.0.join("user-world/persistent.sav")).unwrap(),
            b"existing world"
        );
    }
}

#[test]
fn automatic_retry_recovers_interrupted_first_install_and_later_pending_payloads() {
    for module in ["corekeeper", "barotrauma"] {
        let (root, game) = fixture(module);
        let mut wrote = false;
        assert!(
            publish_payload_with(
                &root.0,
                game,
                b"interrupted B".to_vec(),
                |path, expected, bytes| {
                    io::replace_owned(path, expected, bytes)?;
                    wrote = true;
                    Err("publication interrupted after the file write".into())
                }
            )
            .is_err()
        );
        assert!(wrote);
        assert!(inspect(&root.0, game).is_ok());
        let actual = io::read_regular(&root.0.join(game.plugin), 1024 * 1024).unwrap();
        assert!(
            publish_payload_with(&root.0, game, b"interrupted C".to_vec(), |_, _, _| {
                Err("publication interrupted before the file write".into())
            })
            .is_err()
        );
        let owner = decode(&io::read_regular(&root.0.join(OWNER), 65536).unwrap(), game).unwrap();
        assert_eq!(owner.files.get(game.plugin), Some(&io::digest(&actual)));
        assert!(inspect(&root.0, game).is_ok());
        publish_payload(&root.0, game, b"final D".to_vec()).unwrap();
        assert!(verify_prepared(&root.0, game).is_ok());
        assert_eq!(
            io::read_regular(&root.0.join(game.plugin), 1024 * 1024).unwrap(),
            b"final D"
        );
    }
}

#[test]
fn automatic_publication_preserves_unowned_and_modified_plugin_bytes() {
    for prepared in [false, true] {
        let (root, game) = fixture("barotrauma");
        if prepared {
            publish_payload(&root.0, game, b"owned".to_vec()).unwrap();
        } else {
            std::fs::create_dir(root.0.join(DIRECTORY)).unwrap();
        }
        std::fs::write(root.0.join(game.plugin), b"operator plugin").unwrap();
        let owner = io::read_optional(&root.0.join(OWNER), 65536).unwrap();
        assert!(publish_payload(&root.0, game, b"replacement".to_vec()).is_err());
        assert_eq!(
            io::read_regular(&root.0.join(game.plugin), 1024 * 1024).unwrap(),
            b"operator plugin"
        );
        assert_eq!(
            io::read_optional(&root.0.join(OWNER), 65536).unwrap(),
            owner
        );
    }
}
