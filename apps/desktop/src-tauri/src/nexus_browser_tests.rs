use super::*;

#[test]
fn nexus_browser_has_no_presentation_update_command() {
    use crate::command_registry::{Completion, Execution, find};
    assert!(find("update_nexus_website_host").is_none());
    assert_eq!(
        find("open_nexus_website_host").unwrap().execution,
        Execution::Desktop
    );
    assert_eq!(
        find("install_nexus_website_mod").unwrap().completion,
        Completion::Owned
    );
    for name in [
        "open_nexus_login_host",
        "update_nexus_login_host",
        "close_nexus_login_host",
        "read_nexus_website_auth_state",
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Desktop);
        assert_eq!(command.completion, Completion::Bounded);
        assert_eq!(command.lan, crate::command_registry::LanAccess::Denied);
    }
}

#[test]
fn nexus_browser_blocked_reasons_become_fixed_local_errors() {
    for (reason, code) in [
        ("login", "nexus_website_login_required"),
        ("verification", "nexus_website_verification_required"),
        ("file_selection", "nexus_file_selection_required"),
        ("file_quarantined", "nexus_website_file_quarantined"),
        ("project_identity", "nexus_browser_page_unavailable"),
        ("website_dialog", "nexus_website_action_required"),
        ("website_controls", "nexus_website_controls_unavailable"),
        ("download_not_started", "nexus_browser_timed_out"),
        (
            "https://example.invalid/?secret=private",
            "nexus_website_controls_unavailable",
        ),
    ] {
        assert_eq!(blocked_error(Some(reason)), error(code));
    }
    assert_eq!(
        blocked_error(None),
        error("nexus_website_controls_unavailable")
    );
}

fn target() -> NexusWebsiteTarget {
    NexusWebsiteTarget {
        page_url: "https://www.nexusmods.com/7daystodie/mods/2264?tab=files".into(),
        game_domain: "7daystodie".into(),
        mod_id: "2264".into(),
        accepts: vec!["zip".into()],
    }
}

#[test]
fn nexus_automatic_context_contains_only_project_and_explicit_file_identity() {
    let mut target = target();
    assert_eq!(
        automatic_context(&target).unwrap(),
        json!({
            "gameDomain": "7daystodie", "modId": "2264", "fileId": null,
        })
    );
    target
        .page_url
        .push_str("&file_id=42&key=synthetic-secret&expires=1#private");
    let context = automatic_context(&target).unwrap();
    assert_eq!(
        context,
        json!({ "gameDomain": "7daystodie", "modId": "2264", "fileId": "42" })
    );
    assert!(!context.to_string().contains("synthetic-secret"));
}

fn page() -> Page {
    Page { url: "https://www.nexusmods.com/7daystodie/mods/2264".into(),
        canonical_url: Some("https://www.nexusmods.com/games/7daystodie/mods/2264".into()),
        game_domain: Some("7daystodie".into()), mod_id: Some("2264".into()),
        title: Some("  Website\n Mod  ".into()),
        icon_url: Some("https://staticdelivery.nexusmods.com/mods/1059/images/9559/9559-1770000000-123456789.png".into()),
        state: "ready".into() }
}

#[test]
fn nexus_browser_binds_project_identity_across_official_page_forms() {
    let target = target();
    for url in [
        "https://www.nexusmods.com/7daystodie/mods/2264?tab=files&file_id=42",
        "https://nexusmods.com/games/7daystodie/mods/2264/files/42",
        "https://www.nexusmods.com/7daystodie/mods/2264/files",
    ] {
        assert!(matches_project(url, &target), "{url}");
    }
    for url in [
        "https://www.nexusmods.com/othergame/mods/2264",
        "https://www.nexusmods.com/7daystodie/mods/2265",
        "https://nexusmods.com.attacker.invalid/7daystodie/mods/2264",
        "http://www.nexusmods.com/7daystodie/mods/2264",
        "https://www.nexusmods.com/7daystodie/mods/2264/other",
    ] {
        assert!(!matches_project(url, &target), "{url}");
    }
}

#[test]
fn nexus_browser_readiness_requires_rendered_content_at_the_current_project_or_login() {
    let target = target();
    for url in [
        target.page_url.as_str(),
        "https://users.nexusmods.com/auth/sign_in?return=private",
    ] {
        let current: reqwest::Url = url.parse().unwrap();
        let mut public_url = current.clone();
        public_url.set_query(None);
        let mut rendered = RenderedPage {
            url: public_url.into(),
            has_content: false,
        };
        assert!(!rendered_current_page(&rendered, &current, &target));
        rendered.has_content = true;
        assert!(rendered_current_page(&rendered, &current, &target));
        assert!(!rendered_current_page(
            &rendered,
            &"https://www.nexusmods.com/7daystodie/mods/999"
                .parse()
                .unwrap(),
            &target
        ));
    }
    for url in [
        "about:blank",
        "https://cf-files.nexusmods.com/file.zip",
        "http://users.nexusmods.com/auth/sign_in",
        "https://users.nexusmods.com.attacker.invalid/auth/sign_in",
    ] {
        assert!(!interactive_url(&url.parse().unwrap(), &target));
    }
}

#[test]
fn nexus_browser_only_navigates_the_project_login_and_official_downloads() {
    let target = target();
    for url in [
        &target.page_url,
        "https://users.nexusmods.com/auth/sign_in",
        "https://cf-files.nexusmods.com/payload.zip?token=fixture",
    ] {
        assert!(navigation_allowed(&url.parse().unwrap(), &target, None));
    }
    for url in [
        "https://users.nexusmods.com.attacker.invalid/auth/sign_in",
        "https://www.nexusmods.com/7daystodie/mods/999",
        "file:///C:/private.txt",
        "http://127.0.0.1:8080",
        "nxm://7daystodie/mods/2264/files/42",
    ] {
        assert!(!navigation_allowed(&url.parse().unwrap(), &target, None));
    }
}

fn file_selection_observation() -> PageObservation {
    serde_json::from_value(json!({
        "page": {
            "url": "https://www.nexusmods.com/7daystodie/mods/2264",
            "canonicalUrl": "https://www.nexusmods.com/7daystodie/mods/2264",
            "gameDomain": "7daystodie", "modId": "2264", "title": "Verified project",
            "iconUrl": null, "state": "ready"
        },
        "rendered": {"url":"https://www.nexusmods.com/7daystodie/mods/2264", "hasContent":true},
        "automation": {"state":"working", "phase":"selecting", "fileSelection":{"fileId":"42","fileUid":"1059000042"}}
    })).unwrap()
}

fn empty_challenge_observation() -> PageObservation {
    serde_json::from_value(json!({
        "page": {
            "url": "https://www.nexusmods.com/7daystodie/mods/2264",
            "canonicalUrl": null, "gameDomain": null, "modId": null,
            "title": null, "iconUrl": null, "state": "challenge"
        },
        "rendered": {"url":"https://www.nexusmods.com/7daystodie/mods/2264", "hasContent":false},
        "automation": {"state":"loading"}
    }))
    .unwrap()
}

#[test]
fn nexus_browser_empty_current_challenge_enters_verification_without_authorizing_a_file() {
    let mut target = target();
    target.page_url.push_str("&file_id=42#files");
    let current = target.page_url.parse().unwrap();
    let observation = empty_challenge_observation();
    assert!(!rendered_current_page(
        &observation.rendered,
        &current,
        &target
    ));
    assert_eq!(
        observation_phase(&observation, &current, &target).map(AutomationPhase::name),
        Some("verifying")
    );
    assert!(verified_file_selection(&observation, &current, &target, 0, 0).is_none());
    assert!(verified_page(observation.page, &target).is_none());
}

#[test]
fn nexus_browser_empty_challenge_requires_the_exact_current_official_project() {
    let target = target();
    let current = target.page_url.parse().unwrap();
    for url in [
        "https://www.nexusmods.com/7daystodie/mods/999",
        "https://www.nexusmods.com/othergame/mods/2264",
        "https://www.nexusmods.com.attacker.invalid/7daystodie/mods/2264",
        "http://www.nexusmods.com/7daystodie/mods/2264",
        "https://fixture@www.nexusmods.com/7daystodie/mods/2264",
        "https://www.nexusmods.com:444/7daystodie/mods/2264",
        "https://users.nexusmods.com/auth/sign_in",
    ] {
        let mut observation = empty_challenge_observation();
        observation.page.url = url.into();
        assert!(
            observation_phase(&observation, &current, &target).is_none(),
            "{url}"
        );
        assert!(
            observation_phase(&observation, &url.parse().unwrap(), &target).is_none(),
            "{url}"
        );
    }
    // Another URL form for the same project is still a stale observation.
    assert!(
        observation_phase(
            &empty_challenge_observation(),
            &"https://www.nexusmods.com/games/7daystodie/mods/2264/files"
                .parse()
                .unwrap(),
            &target
        )
        .is_none()
    );
}

#[test]
fn nexus_browser_empty_page_needs_an_explicit_challenge_marker() {
    let target = target();
    let current = target.page_url.parse().unwrap();
    for state in ["pending", "invalid", "ready"] {
        let mut observation = empty_challenge_observation();
        observation.page.state = state.into();
        observation.automation.phase = Some(AutomationPhase::Verifying);
        assert!(
            observation_phase(&observation, &current, &target).is_none(),
            "{state}"
        );
    }
}

#[test]
fn nexus_browser_rendered_automation_phases_still_require_current_content() {
    let target = target();
    let current = target.page_url.parse().unwrap();
    for phase in [
        AutomationPhase::Selecting,
        AutomationPhase::Waiting,
        AutomationPhase::Verifying,
    ] {
        let mut observation = file_selection_observation();
        observation.automation.phase = Some(phase);
        assert_eq!(
            observation_phase(&observation, &current, &target).map(AutomationPhase::name),
            Some(phase.name())
        );
        observation.rendered.has_content = false;
        assert!(observation_phase(&observation, &current, &target).is_none());
        observation.rendered.has_content = true;
        observation.rendered.url = "https://www.nexusmods.com/7daystodie/mods/999".into();
        assert!(observation_phase(&observation, &current, &target).is_none());
    }
}

#[test]
fn nexus_browser_empty_challenge_uses_verification_and_total_deadlines() {
    assert_eq!(
        selection_timeout(PAGE_LOAD_TIMEOUT, false, None),
        Some("nexus_browser_page_unavailable")
    );
    assert_eq!(
        selection_timeout(PAGE_LOAD_TIMEOUT, false, Some(Duration::from_secs(1))),
        None
    );
    assert_eq!(
        selection_timeout(PAGE_LOAD_TIMEOUT, false, Some(VERIFICATION_TIMEOUT)),
        Some("nexus_website_verification_required")
    );
    assert_eq!(
        selection_timeout(AUTOMATIC_TIMEOUT, false, Some(Duration::from_secs(1))),
        Some("nexus_browser_timed_out")
    );
    // Rendered content and successful verification do not reset the total budget.
    assert_eq!(selection_timeout(PAGE_LOAD_TIMEOUT, true, None), None);
    assert_eq!(
        selection_timeout(AUTOMATIC_TIMEOUT, true, None),
        Some("nexus_browser_timed_out")
    );
}

#[test]
fn nexus_file_selection_requires_verified_current_rendered_project_and_generation() {
    let target = target();
    let current = target.page_url.parse().unwrap();
    let valid =
        verified_file_selection(&file_selection_observation(), &current, &target, 3, 3).unwrap();
    assert_eq!(json!(valid), json!({"fileId":"42","fileUid":"1059000042"}));
    assert!(
        verified_file_selection(&file_selection_observation(), &current, &target, 3, 4).is_none()
    );
    assert!(
        verified_file_selection(
            &file_selection_observation(),
            &"https://www.nexusmods.com/7daystodie/mods/999"
                .parse()
                .unwrap(),
            &target,
            3,
            3,
        )
        .is_none()
    );
    for change in 0..7 {
        let mut observation = file_selection_observation();
        match change {
            0 => observation.page.state = "pending".into(),
            1 => observation.page.title = None,
            2 => {
                observation.page.canonical_url =
                    Some("https://www.nexusmods.com/7daystodie/mods/999".into())
            }
            3 => observation.rendered.has_content = false,
            4 => observation.rendered.url = "https://www.nexusmods.com/7daystodie/mods/999".into(),
            5 => observation.automation.state = AutomationState::Blocked,
            6 => observation.automation.phase = Some(AutomationPhase::Waiting),
            _ => unreachable!(),
        }
        assert!(
            verified_file_selection(&observation, &current, &target, 3, 3).is_none(),
            "case {change}"
        );
    }
}

#[test]
fn nexus_file_selection_ids_are_canonical_positive_u64_and_honor_explicit_file() {
    let mut target = target();
    let current = target.page_url.parse().unwrap();
    for value in [
        "",
        "0",
        "01",
        "+1",
        "-1",
        "1 ",
        "1.0",
        "18446744073709551616",
        "42/download",
    ] {
        for uid in [false, true] {
            let mut observation = file_selection_observation();
            let selection = observation.automation.file_selection.as_mut().unwrap();
            if uid {
                selection.file_uid = value.into();
            } else {
                selection.file_id = value.into();
            }
            assert!(verified_file_selection(&observation, &current, &target, 0, 0).is_none());
        }
    }
    target.page_url.push_str("&file_id=43");
    assert!(
        verified_file_selection(&file_selection_observation(), &current, &target, 0, 0).is_none()
    );
    target.page_url = target.page_url.replace("file_id=43", "file_id=42");
    assert!(
        verified_file_selection(&file_selection_observation(), &current, &target, 0, 0).is_some()
    );
}

#[test]
fn nexus_website_route_is_exact_and_requires_the_authorized_uid() {
    let target = target();
    let current = target.page_url.parse().unwrap();
    let selected =
        verified_file_selection(&file_selection_observation(), &current, &target, 0, 0).unwrap();
    let route = "https://www.nexusmods.com/api/files/1059000042/download"
        .parse()
        .unwrap();
    assert!(!navigation_allowed(&route, &target, None));
    assert!(navigation_allowed(&route, &target, Some(&selected)));
    assert!(!interactive_url(&route, &target));
    assert!(!matches_project(route.as_str(), &target));
    let mut response_page = page();
    response_page.url = route.to_string();
    assert!(verified_page(response_page, &target).is_none());
    for value in [
        "https://www.nexusmods.com/api/files/1059000043/download",
        "https://www.nexusmods.com/api/files/1059000042/download/",
        "https://www.nexusmods.com/api/files/1059000042/download?key=private",
        "https://www.nexusmods.com/api/files/1059000042/download#private",
        "https://www.nexusmods.com:444/api/files/1059000042/download",
        "https://private@www.nexusmods.com/api/files/1059000042/download",
        "https://nexusmods.com/api/files/1059000042/download",
        "https://www.nexusmods.com.attacker.invalid/api/files/1059000042/download",
        "http://www.nexusmods.com/api/files/1059000042/download",
    ] {
        let url = value.parse().unwrap();
        assert!(
            !navigation_allowed(&url, &target, Some(&selected)),
            "{value}"
        );
    }
}

#[test]
fn nexus_file_authorization_is_cleared_when_navigation_leaves_the_selected_project() {
    let target = target();
    let selected = file_selection_observation()
        .automation
        .file_selection
        .unwrap();
    for value in [
        "https://www.nexusmods.com/7daystodie/mods/999",
        "https://users.nexusmods.com/auth/sign_in",
        "https://www.nexusmods.com/api/files/1059000043/download",
        "https://outside.invalid/",
    ] {
        let mut selection = Some(selected.clone());
        retain_selection_for_navigation(&mut selection, &value.parse().unwrap(), &target);
        assert!(selection.is_none());
    }
    for value in [
        target.page_url.as_str(),
        "https://www.nexusmods.com/api/files/1059000042/download",
        "https://cf-files.nexusmods.com/file.zip?token=fixture",
    ] {
        let mut selection = Some(selected.clone());
        retain_selection_for_navigation(&mut selection, &value.parse().unwrap(), &target);
        assert_eq!(selection, Some(selected.clone()));
    }
}

#[test]
fn nexus_browser_verifies_public_metadata_but_does_not_require_an_avatar() {
    let target = target();
    let valid = verified_page(page(), &target).unwrap();
    assert_eq!(valid.title.as_deref(), Some("Website Mod"));
    assert!(valid.icon_url.is_some());
    let mut no_image = page();
    no_image.icon_url = Some("https://untrusted.invalid/avatar.png".into());
    assert!(verified_page(no_image, &target).unwrap().icon_url.is_none());
    for changed in [
        Page {
            state: "challenge".into(),
            ..page()
        },
        Page {
            canonical_url: None,
            ..page()
        },
        Page {
            mod_id: Some("999".into()),
            ..page()
        },
        Page {
            game_domain: Some("othergame".into()),
            ..page()
        },
        Page {
            canonical_url: Some("https://www.nexusmods.com/7daystodie/mods/999".into()),
            ..page()
        },
    ] {
        assert!(verified_page(changed, &target).is_none());
    }
}

#[test]
fn nexus_download_capture_waits_for_page_metadata_without_touching_the_destination() {
    let url = "https://cf-files.nexusmods.com/server.zip?token=synthetic-test-only"
        .parse()
        .unwrap();
    let requested =
        requested_download(&url, std::path::Path::new("C:/Unwritten/Server Mod.zip")).unwrap();
    assert_eq!(requested.file_name, "Server Mod.zip");
    assert_eq!(requested.url, url.as_str());
    // Capture does not depend on the separate DOM callback having completed first.
    assert!(verified_page(page(), &target()).is_some());
    let rejected = requested_download(
        &"https://untrusted.invalid/file.zip?token=synthetic-test-only"
            .parse()
            .unwrap(),
        std::path::Path::new("file.zip"),
    )
    .err()
    .unwrap();
    assert_eq!(rejected, error("nexus_file_unavailable"));
    assert!(!rejected.contains("synthetic-test-only"));
}
