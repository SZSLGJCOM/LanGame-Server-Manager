use super::*;

#[test]
fn cached_server_account_authorization_is_bound_to_target_account_and_storage_context() {
    let (sessions, _) = fixture();
    let steam_root = std::path::Path::new("steam-a");
    let storage_root = std::path::Path::new("profile-a");
    assert!(
        sessions
            .authorize_account(&target("palworld-a"), "alice", steam_root, storage_root)
            .is_err()
    );
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steam-a".into(),
        }),
    );
    assert!(
        sessions
            .authorize_account(&target("palworld-a"), "alice", steam_root, storage_root)
            .is_ok()
    );
    for (target, account, root, profile) in [
        (target("palworld-b"), "alice", steam_root, storage_root),
        (
            SteamAccountLoginTarget::Module("palworld-a".into()),
            "alice",
            steam_root,
            storage_root,
        ),
        (target("palworld-a"), "bob", steam_root, storage_root),
        (
            target("palworld-a"),
            "alice",
            std::path::Path::new("steam-b"),
            storage_root,
        ),
        (
            target("palworld-a"),
            "alice",
            steam_root,
            std::path::Path::new("profile-b"),
        ),
    ] {
        assert!(
            sessions
                .authorize_account(&target, account, root, profile)
                .is_err()
        );
    }
}

#[test]
fn prestart_account_requires_successful_download_of_the_exact_program_and_can_be_revoked() {
    let (sessions, _) = fixture();
    let target = target("palworld-a");
    let steam = std::path::Path::new("steam-a");
    let profile = std::path::Path::new("profile-a");
    let program = std::path::Path::new("program-a");
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steam-a".into(),
        }),
    );
    let read = || {
        sessions
            .server_download_account(
                std::slice::from_ref(&target),
                "palworld",
                program,
                steam,
                profile,
            )
            .unwrap()
    };
    assert!(read().is_none());
    let authorization = sessions
        .authorize_account(&target, "alice", steam, profile)
        .unwrap();
    sessions.record_server_download(
        &target,
        "alice",
        &authorization,
        "palworld",
        program.to_owned(),
    );
    assert_eq!(read().as_deref(), Some("alice"));
    for (targets, module, root, steam, profile) in [
        (
            vec![SteamAccountLoginTarget::Instance("other".into())],
            "palworld",
            program,
            steam,
            profile,
        ),
        (vec![target.clone()], "other-game", program, steam, profile),
        (
            vec![target.clone()],
            "palworld",
            std::path::Path::new("other-program"),
            steam,
            profile,
        ),
        (
            vec![target.clone()],
            "palworld",
            program,
            std::path::Path::new("other-steam"),
            profile,
        ),
        (
            vec![target.clone()],
            "palworld",
            program,
            steam,
            std::path::Path::new("other-profile"),
        ),
    ] {
        assert!(
            sessions
                .server_download_account(&targets, module, root, steam, profile)
                .unwrap()
                .is_none()
        );
    }
    sessions.clear_server_download(&target).unwrap();
    assert!(read().is_none());
    // An install completing after anonymous was selected cannot restore permission.
    sessions.record_server_download(
        &target,
        "alice",
        &authorization,
        "palworld",
        program.to_owned(),
    );
    assert!(read().is_none());
    let authorization = sessions
        .authorize_account(&target, "alice", steam, profile)
        .unwrap();
    sessions.record_server_download(
        &target,
        "alice",
        &authorization,
        "palworld",
        program.to_owned(),
    );
    assert_eq!(read().as_deref(), Some("alice"));
}

#[test]
fn library_prestart_permission_does_not_expand_to_an_independent_instance_or_a_new_login() {
    let sessions = SteamWorkshopAccountSessions::default();
    let module = SteamAccountLoginTarget::Module("palworld".into());
    let instance = target("palworld-a");
    let steam = std::path::Path::new("steam-a");
    let profile = std::path::Path::new("profile-a");
    let program = std::path::Path::new("program-a");
    let (sender, _) = mpsc::channel(1);
    sessions
        .begin(&module, "session-a", sender, profile)
        .unwrap();
    sessions.finish(
        "session-a",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steam-a".into(),
        }),
    );
    let authorization = sessions
        .authorize_account(&module, "alice", steam, profile)
        .unwrap();
    sessions.record_server_download(
        &module,
        "alice",
        &authorization,
        "palworld",
        program.to_owned(),
    );
    assert!(
        sessions
            .server_download_account(
                std::slice::from_ref(&instance),
                "palworld",
                program,
                steam,
                profile
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        sessions
            .server_download_account(
                &[instance, module.clone()],
                "palworld",
                program,
                steam,
                profile
            )
            .unwrap()
            .as_deref(),
        Some("alice")
    );
    let (sender, _) = mpsc::channel(1);
    sessions
        .begin(&module, "session-b", sender, profile)
        .unwrap();
    sessions.finish(
        "session-b",
        Ok(SteamWorkshopAccountAuthenticationResult {
            account_name: "alice".into(),
            steamcmd_root: "steam-a".into(),
        }),
    );
    sessions.record_server_download(
        &module,
        "alice",
        &authorization,
        "palworld",
        program.to_owned(),
    );
    assert!(
        sessions
            .server_download_account(&[module], "palworld", program, steam, profile)
            .unwrap()
            .is_none()
    );
}
