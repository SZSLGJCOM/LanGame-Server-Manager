use super::*;

#[test]
fn public_project_ids_and_native_numeric_references_remain_available_without_credentials() {
    for reference in [
        "1346144",
        " cf:1346144 ",
        "CurseForge:1346144",
        "https://www.curseforge.com/projects/1346144",
        "http://www.curseforge.com/projects/1346144",
        "https://api.curseforge.com/v1/mods/1346144",
        "https://www.curseforge.com/ark-survival-ascended/mods/example?projectId=1346144",
    ] {
        assert_eq!(
            project_id_from_reference(reference).as_deref(),
            Some("1346144")
        );
    }
}

#[test]
fn file_references_preserve_file_identity_separately_from_project_identity() {
    let slug = parse_reference(
        "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting/files/6614383",
    )
    .unwrap();
    assert_eq!(slug.project_id, None);
    assert_eq!(slug.game_slug.as_deref(), Some("ark-survival-ascended"));
    assert_eq!(slug.mod_slug.as_deref(), Some("devkitlivemodtesting"));
    assert_eq!(slug.file_id.as_deref(), Some("6614383"));
    let numeric =
        parse_reference("https://api.curseforge.com/v1/mods/1346144/files/6614383").unwrap();
    assert_eq!(numeric.project_id.as_deref(), Some("1346144"));
    assert_eq!(numeric.file_id.as_deref(), Some("6614383"));
}

#[test]
fn existing_minecraft_and_windrose_mod_routes_preserve_the_game_scope() {
    for (reference, game, slug) in [
        (
            "https://www.curseforge.com/minecraft/mc-mods/example-mod",
            "minecraft",
            "example-mod",
        ),
        (
            "https://www.curseforge.com/windrose/mods/example-mod/files",
            "windrose",
            "example-mod",
        ),
    ] {
        let parsed = parse_reference(reference).unwrap();
        assert_eq!(parsed.game_slug.as_deref(), Some(game));
        assert_eq!(mod_slug_from_reference(reference).as_deref(), Some(slug));
    }
    assert!(parse_reference("https://www.curseforge.com/windrose/mc-mods/example").is_none());
    assert!(parse_reference("https://www.curseforge.com/minecraft/modpacks/example").is_none());
}

#[test]
fn unrelated_and_lookalike_hosts_cannot_supply_curseforge_ids_or_slugs() {
    for reference in [
        "https://example.com/projects/1346144",
        "https://example.com/?projectId=1346144",
        "https://evilcurseforge.com/windrose/mods/example",
        "https://curseforge.com.example.com/projects/1346144",
        "https://api.curse.tools/v1/cf/mods/1346144",
        "https://other.curseforge.com/projects/1346144",
        "https://curseforge.com@evil.example/projects/1346144",
    ] {
        assert!(parse_reference(reference).is_none(), "accepted {reference}");
    }
}

#[test]
fn credentials_and_nonstandard_transport_are_not_public_mod_references() {
    for reference in [
        "ftp://www.curseforge.com/projects/1346144",
        "https://user@www.curseforge.com/projects/1346144",
        "https://user:fixture@www.curseforge.com/projects/1346144",
        "https://www.curseforge.com:444/projects/1346144",
        "file:///projects/1346144",
    ] {
        assert!(parse_reference(reference).is_none(), "accepted {reference}");
    }
}

#[test]
fn invalid_or_conflicting_project_identifiers_are_not_silently_selected() {
    for reference in [
        "0",
        "cf:0",
        "cf:+1",
        "4294967296",
        "cf:１２３",
        "cf:😀",
        "https://www.curseforge.com/projects/1346144?modId=1346145",
        "https://www.curseforge.com/windrose/mods/example?projectId=1346144&projectId=1346145",
        "https://www.curseforge.com/projects/1346144?modId=broken",
        "https://www.curseforge.com/projects/",
        "https://www.curseforge.com/windrose/mods/example/files/0",
        "https://www.curseforge.com/windrose/mods/example/files/1346144/another-mod",
    ] {
        assert!(parse_reference(reference).is_none(), "accepted {reference}");
    }
    assert_eq!(
        project_id_from_reference("0001346144").as_deref(),
        Some("1346144")
    );
    assert_eq!(
        project_id_from_reference("https://www.curseforge.com/projects/1346144?modId=1346144")
            .as_deref(),
        Some("1346144")
    );
}

#[test]
fn input_and_slug_bounds_are_enforced_before_metadata_requests() {
    assert!(
        parse_reference(&format!(
            "https://www.curseforge.com/windrose/mods/{}",
            "a".repeat(129)
        ))
        .is_none()
    );
    assert!(
        parse_reference(&format!(
            "https://www.curseforge.com/projects/1346144?ignored={}",
            "a".repeat(2048)
        ))
        .is_none()
    );
    assert!(parse_reference("https://www.curseforge.com/windrose/mods/😀").is_none());
    assert!(parse_reference("https://www.curseforge.com/windrose//mods/example").is_none());
}
