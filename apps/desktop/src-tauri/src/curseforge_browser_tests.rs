use super::*;

const URL: &str = "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting";

#[test]
fn automatic_checks_stay_hidden_and_only_a_persistent_challenge_requests_attention() {
    let start = Instant::now();
    let mut gate = VerificationGate::default();
    assert!(!gate.observe(false, start));
    assert!(!gate.observe(true, start + Duration::from_secs(1)));
    assert!(!gate.observe(false, start + Duration::from_secs(4)));
    assert!(!gate.observe(true, start + Duration::from_secs(10)));
    assert!(!gate.observe(true, start + Duration::from_secs(14)));
    assert!(gate.observe(true, start + Duration::from_secs(15)));
    assert!(!gate.observe(true, start + Duration::from_secs(20)));
}

fn resolved() -> ManualModReferenceResolveResult {
    ManualModReferenceResolveResult {
        instance_id: "asa-test".into(),
        module_id: "arksurvivalascended".into(),
        source_label: "CurseForge".into(),
        setting_key: "mod_ids_csv".into(),
        setting_label: "Mods".into(),
        items: vec![ManualModReferenceItem {
            reference: "1346144".into(),
            status: "resolved".into(),
            resolved_id: Some("1346144".into()),
            title: None,
            message: None,
        }],
        resolved_ids: vec!["1346144".into()],
    }
}

#[test]
fn batch_results_keep_original_links_titles_and_partial_failures() {
    let project = read_project(page("ready", Some("1346144")), URL)
        .unwrap()
        .unwrap();
    let result = merge_result(
        resolved(),
        vec![
            (URL.into(), Some(project), None),
            ("1346144".into(), None, None),
            (
                "https://www.curseforge.com/ark-survival-ascended/mods/unavailable".into(),
                None,
                Some(browser_error("timeout")),
            ),
        ],
    )
    .unwrap();
    assert_eq!(result.resolved_ids, ["1346144"]);
    assert_eq!(result.items.len(), 3);
    assert_eq!(result.items[0].reference, URL);
    assert_eq!(
        result.items[0].title.as_deref(),
        Some("DevkitLiveModTesting")
    );
    assert_eq!(result.items[2].status, "failed");
    assert!(result.items[2].resolved_id.is_none());
    assert_eq!(
        result.items[2].message.as_deref(),
        Some(browser_error("timeout").as_str())
    );
    assert!(merge_result(resolved(), vec![("999999".into(), None, None)]).is_err());
}

fn page(state: &str, id: Option<&str>) -> Page {
    Page {
        url: URL.into(),
        canonical_url: Some(URL.into()),
        project_id: id.map(str::to_owned),
        title: Some("DevkitLiveModTesting".into()),
        state: state.into(),
    }
}

#[test]
fn browser_navigation_stays_on_requested_https_project() {
    for suffix in ["", "/", "?__cf_chl_tk=token", "#details"] {
        assert!(same_page(&format!("{URL}{suffix}"), URL));
    }
    for other in [
        "http://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting",
        "https://www.curseforge.com.evil.test/ark-survival-ascended/mods/devkitlivemodtesting",
        "https://www.curseforge.com/ark-survival-ascended/mods/other",
        "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting/files/123456",
        "https://user@www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting",
        "https://www.curseforge.com:444/ark-survival-ascended/mods/devkitlivemodtesting",
    ] {
        assert!(!same_page(other, URL), "{other}");
    }
}

#[test]
fn browser_urls_discard_file_ids_queries_and_bookmark_hosts() {
    let reference = "http://legacy.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting/files/999999?token=discard";
    assert_eq!(project_url(reference).as_deref(), Some(URL));
    for direct in [
        "1346144",
        "cf:1346144",
        "https://www.curseforge.com/projects/1346144",
    ] {
        assert!(project_url(direct).is_none());
    }
    for wrong in [
        "https://evil.test/ark-survival-ascended/mods/devkitlivemodtesting",
        "https://www.curseforge.com/minecraft/mc-mods/foo",
    ] {
        assert!(project_url(wrong).is_none());
    }
}

#[test]
fn observed_page_must_match_and_provide_positive_project_id() {
    assert_eq!(
        read_project(page("ready", Some("1346144")), URL)
            .unwrap()
            .unwrap()
            .id,
        "1346144"
    );
    for state in ["pending", "challenge"] {
        assert!(read_project(page(state, None), URL).unwrap().is_none());
    }
    for id in [
        None,
        Some("0"),
        Some("4294967296"),
        Some("123abc"),
        Some("cf:123"),
    ] {
        assert!(read_project(page("ready", id), URL).is_err());
    }
    let mut redirected = page("ready", Some("1346144"));
    redirected.url = "https://www.curseforge.com/ark-survival-ascended/mods/other".into();
    assert!(read_project(redirected, URL).is_err());
    let mut wrong_canonical = page("ready", Some("1346144"));
    wrong_canonical.canonical_url = Some("https://evil.test/".into());
    assert!(read_project(wrong_canonical, URL).is_err());
    assert!(read_project(page("invalid", Some("1346144")), URL).is_err());
}
