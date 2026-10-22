use super::*;

#[test]
fn unreal_world_save_installation_manifest_accepts_only_the_two_owned_files() {
    let valid = BTreeMap::from([
        ("Scripts/main.lua".into(), "a".repeat(64)),
        ("enabled.txt".into(), "b".repeat(64)),
    ]);
    assert!(validate_hashes(Game::Abiotic, &valid));
    let mut invalid = valid.clone();
    invalid.insert("../other.lua".into(), "a".repeat(64));
    assert!(!validate_hashes(Game::Abiotic, &invalid));
    let mut invalid = valid;
    invalid.insert("enabled.txt".into(), "invalid".into());
    assert!(!validate_hashes(Game::Abiotic, &invalid));
}

#[test]
#[cfg(windows)]
fn native_dll_installations_keep_game_specific_ownership_and_preserve_external_edits() {
    for game in [Game::Nightingale, Game::Conan, Game::Scum] {
        let root = std::env::temp_dir().join(format!(
            "langame-native-save-payload-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let result = (|| -> Result<(), String> {
            publish_payload(game, &root, b"first")?;
            let (relative, expected) = game
                .native_payload()
                .ok_or("Native fixture requires Windows payloads")?;
            let native = root.join(mod_dir(game)).join(relative);
            assert_eq!(fs::read(&native).unwrap(), expected);
            let mut record: Ownership =
                serde_json::from_slice(&read(&root.join(OWNER), 8192)?).unwrap();
            assert_eq!(record.files.len(), 3);
            assert!(validate_hashes(game, &record.files));
            record
                .files
                .insert("native/operator.dll".into(), "a".repeat(64));
            assert!(!validate_hashes(game, &record.files));
            fs::write(&native, b"operator edit").unwrap();
            assert!(publish_payload(game, &root, b"second").is_err());
            assert_eq!(fs::read(&native).unwrap(), b"operator edit");
            Ok(())
        })();
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }
}

#[test]
fn moria_owned_native_bridge_retirement_recovers_and_preserves_external_edits() {
    let root = std::env::temp_dir().join(format!(
        "langame-moria-save-retirement-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let game = Game::Moria;
    let directory = root.join(mod_dir(game));
    fs::create_dir_all(directory.join("Scripts")).unwrap();
    fs::create_dir_all(directory.join("native")).unwrap();
    let old = [
        ("Scripts/main.lua", &b"previous policy"[..]),
        ("enabled.txt", &b""[..]),
        (RETIRED_MORIA_BRIDGE, &b"previous owned bridge"[..]),
    ];
    for (name, bytes) in old {
        fs::write(directory.join(name), bytes).unwrap();
    }
    let owner = Ownership {
        protocol: 1,
        module_id: game.id().into(),
        files: old
            .into_iter()
            .map(|(name, bytes)| (name.into(), hash(bytes)))
            .collect(),
        pending_files: None,
    };
    fs::write(root.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(owned(game, &root).unwrap());
    verify_owned_mods(game, &root).unwrap();
    let bridge = directory.join(RETIRED_MORIA_BRIDGE);
    fs::write(&bridge, b"operator edit").unwrap();
    assert!(verify_owned_mods(game, &root).is_err());
    let original_owner = fs::read(root.join(OWNER)).unwrap();
    assert!(publish_payload(game, &root, b"current policy").is_err());
    assert_eq!(fs::read(&bridge).unwrap(), b"operator edit");
    assert_eq!(fs::read(root.join(OWNER)).unwrap(), original_owner);
    fs::write(&bridge, b"previous owned bridge").unwrap();
    assert!(
        publish_payload_with(game, &root, b"current policy", |path, before, after| {
            if path == root.join(OWNER) {
                return Err("interrupted before intent publication".into());
            }
            publish(path, before, after)
        })
        .is_err()
    );
    assert!(!bridge.exists());
    assert!(owned(game, &root).unwrap());
    verify_owned_mods(game, &root).unwrap();
    publish_payload(game, &root, b"current policy").unwrap();
    assert_eq!(
        fs::read(directory.join("Scripts/main.lua")).unwrap(),
        b"current policy"
    );
    let record: Ownership = serde_json::from_slice(&fs::read(root.join(OWNER)).unwrap()).unwrap();
    assert_eq!(record.files.len(), 2);
    assert!(record.pending_files.is_none() && game.native_payload().is_none());
    fs::write(&bridge, b"unowned replacement").unwrap();
    assert!(publish_payload(game, &root, b"next policy").is_err());
    assert_eq!(fs::read(&bridge).unwrap(), b"unowned replacement");
    assert_eq!(
        fs::read(directory.join("Scripts/main.lua")).unwrap(),
        b"current policy"
    );
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn loader_deactivation_requires_only_unchanged_owned_save_mods() {
    let root = std::env::temp_dir().join(format!(
        "langame-save-mod-admission-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let game = Game::Abiotic;
    verify_owned_mods(game, &root).unwrap();
    publish_payload(game, &root, b"owned policy").unwrap();
    verify_owned_mods(game, &root).unwrap();
    let owner_path = root.join(OWNER);
    let original = fs::read(&owner_path).unwrap();
    let mut owner = decode(game, &original).unwrap();
    owner.pending_files = Some(BTreeMap::from([
        ("Scripts/main.lua".into(), hash(b"pending policy")),
        ("enabled.txt".into(), hash(b"")),
    ]));
    let pending = serde_json::to_vec(&owner).unwrap();
    fs::write(&owner_path, &pending).unwrap();
    let directory = root.join(mod_dir(game));
    let script = directory.join("Scripts/main.lua");
    fs::write(&script, b"pending policy").unwrap();
    verify_owned_mods(game, &root).unwrap();
    fs::remove_file(directory.join("enabled.txt")).unwrap();
    verify_owned_mods(game, &root).unwrap();
    fs::write(&script, b"operator edit").unwrap();
    assert!(verify_owned_mods(game, &root).is_err());
    assert_eq!(fs::read(&script).unwrap(), b"operator edit");
    assert_eq!(fs::read(&owner_path).unwrap(), pending);
    fs::write(&script, b"pending policy").unwrap();
    for foreign in [
        directory.join("user.lua"),
        directory.parent().unwrap().join("mods.txt"),
    ] {
        fs::write(&foreign, b"operator content").unwrap();
        assert!(verify_owned_mods(game, &root).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"operator content");
        fs::remove_file(foreign).unwrap();
    }
    let query = directory.parent().unwrap().join("LgsmPlayerQuery");
    fs::create_dir(&query).unwrap();
    assert!(verify_owned_mods(game, &root).is_err());
    fs::remove_dir(&query).unwrap();
    fs::remove_file(&owner_path).unwrap();
    assert!(verify_owned_mods(game, &root).is_err());
    owner.module_id = Game::Conan.id().into();
    fs::write(&owner_path, serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(verify_owned_mods(game, &root).is_err());
    assert_eq!(fs::read(&script).unwrap(), b"pending policy");
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreal_world_save_installation_cas_never_replaces_an_external_edit() {
    let root = std::env::temp_dir().join(format!(
        "langame-unreal-save-install-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("owned.lua");
    publish(&path, None, b"original").unwrap();
    assert!(publish(&path, Some(b"different"), b"replacement").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert!(publish(&path, None, b"replacement").is_err());
    publish(&path, Some(b"original"), b"replacement").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"replacement");
    fs::remove_file(path).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
#[cfg(windows)]
fn scum_bootstrap_retirement_recovers_and_preserves_external_code() {
    for pending_bootstrap in [false, true] {
        retire_scum_bootstrap(pending_bootstrap);
    }
}

#[cfg(windows)]
fn retire_scum_bootstrap(pending_bootstrap: bool) {
    let game = Game::Scum;
    let root =
        std::env::temp_dir().join(format!("langame-scum-early-save-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let directory = root.join(mod_dir(game));
    fs::create_dir_all(directory.join("Scripts")).unwrap();
    fs::create_dir_all(directory.join("native")).unwrap();
    fs::create_dir_all(directory.join("dlls")).unwrap();
    let old = [
        ("Scripts/main.lua", &b"previous policy"[..]),
        ("enabled.txt", &b""[..]),
        (
            game.native_payload().unwrap().0,
            &b"previous native payload"[..],
        ),
        (RETIRED_SCUM_BOOTSTRAP, &b"previous owned bootstrap"[..]),
    ];
    for (name, bytes) in old {
        fs::write(directory.join(name), bytes).unwrap();
    }
    let mut files: BTreeMap<String, String> = old
        .into_iter()
        .map(|(name, bytes)| (name.into(), hash(bytes)))
        .collect();
    let pending_files = pending_bootstrap.then(|| files.clone());
    if pending_bootstrap {
        files.remove(RETIRED_SCUM_BOOTSTRAP);
    }
    let owner = Ownership {
        protocol: 1,
        module_id: game.id().into(),
        files,
        pending_files,
    };
    fs::write(root.join(OWNER), serde_json::to_vec(&owner).unwrap()).unwrap();
    assert!(owned(game, &root).unwrap());
    let bootstrap = directory.join(RETIRED_SCUM_BOOTSTRAP);
    fs::write(&bootstrap, b"operator bootstrap").unwrap();
    assert!(publish_payload(game, &root, b"current policy").is_err());
    assert_eq!(fs::read(&bootstrap).unwrap(), b"operator bootstrap");
    fs::write(&bootstrap, b"previous owned bootstrap").unwrap();
    let mut interrupted = false;
    assert!(
        publish_payload_with(game, &root, b"current policy", |path, before, after| {
            if path == root.join(OWNER) && !interrupted {
                interrupted = true;
                return Err("injected interruption after bootstrap retirement".into());
            }
            publish(path, before, after)
        })
        .is_err()
    );
    assert!(interrupted);
    assert!(!bootstrap.exists());
    assert!(owned(game, &root).unwrap());
    publish_payload(game, &root, b"current policy").unwrap();
    assert!(!bootstrap.exists());
    let record: Ownership = serde_json::from_slice(&fs::read(root.join(OWNER)).unwrap()).unwrap();
    assert_eq!(record.files.len(), 3);
    assert!(record.pending_files.is_none());
    fs::write(&bootstrap, b"external change").unwrap();
    assert!(publish_payload(game, &root, b"next policy").is_err());
    assert_eq!(fs::read(&bootstrap).unwrap(), b"external change");
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreal_world_save_installation_refresh_recovers_and_upgrades_only_owned_payloads() {
    let root = std::env::temp_dir().join(format!(
        "langame-unreal-save-refresh-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let game = Game::Moria;
    assert!(!owned(game, &root).unwrap());
    publish_payload(game, &root, b"old-payload").unwrap();
    assert!(owned(game, &root).unwrap());
    publish_payload(game, &root, b"current-payload").unwrap();
    let script = root.join(mod_dir(game)).join("Scripts/main.lua");
    assert_eq!(fs::read(&script).unwrap(), b"current-payload");
    fs::write(&script, b"operator edit").unwrap();
    let owner = fs::read(root.join(OWNER)).unwrap();
    assert!(publish_payload(game, &root, b"upgrade").is_err());
    assert_eq!(fs::read(&script).unwrap(), b"operator edit");
    assert_eq!(fs::read(root.join(OWNER)).unwrap(), owner);
    // Only this fresh UUID directory is owned by the fixture.
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unreal_world_save_installation_retains_actual_pending_bytes_across_two_interrupted_upgrades() {
    struct Cleanup<'a>(&'a Path);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
            let _ = fs::remove_dir_all(self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "langame-unreal-save-intent-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir(&root).unwrap();
    let _cleanup = Cleanup(&root);
    let game = Game::Moria;
    publish_payload(game, &root, b"release-a").unwrap();
    let script = root.join(mod_dir(game)).join("Scripts/main.lua");
    let mut commits = 0;
    let result = publish_payload_with(game, &root, b"release-b", |path, original, replacement| {
        commits += 1;
        if commits == 3 {
            return Err("interrupted after the first payload write".into());
        }
        publish(path, original, replacement)
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&script).unwrap(), b"release-b");
    let mut commits = 0;
    let result = publish_payload_with(game, &root, b"release-c", |path, original, replacement| {
        commits += 1;
        if commits == 2 {
            return Err("interrupted after the new intent write".into());
        }
        publish(path, original, replacement)
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&script).unwrap(), b"release-b");
    let owner: Ownership = serde_json::from_slice(&fs::read(root.join(OWNER)).unwrap()).unwrap();
    assert_eq!(
        owner.files.get("Scripts/main.lua"),
        Some(&hash(b"release-b"))
    );
    publish_payload(game, &root, b"release-d").unwrap();
    assert_eq!(fs::read(&script).unwrap(), b"release-d");
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
}
