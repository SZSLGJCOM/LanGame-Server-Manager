use super::*;
use crate::commands::commands_managed_save_control::{profile, tests::Fixture};

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
        scratch.join("VRisingSaveCompletion.cs").display()
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
    assert!(!sons.contains("VRisingSaveCompletion.cs"));
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
