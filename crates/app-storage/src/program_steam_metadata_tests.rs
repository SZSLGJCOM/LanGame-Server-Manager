use super::installed_app_state;

fn app_state(fields: &str) -> String {
    format!("\"AppState\"\n{{\n{fields}\n}}")
}

#[test]
fn installation_identity_is_read_only_from_unambiguous_top_level_fields() {
    let valid = app_state(
        r#"
        "appid" "896660"
        "StateFlags" "4"
        "buildid" "25730807"
        "TargetBuildID" "25730807"
        "BytesToDownload" "123"
        "BytesDownloaded" "123"
        "BytesToStage" "456"
        "BytesStaged" "456"
        "InstalledDepots" { "896662" { "manifest" "6497701753597013477" } }
        "UserConfig" { "appid" "42" "StateFlags" "1026" "buildid" "0" }
    "#,
    );
    assert!(installed_app_state(&valid, 896660));
    assert!(!installed_app_state(&valid, 42));
    for (from, to) in [
        ("\"896660\"", "\"42\""),
        ("\"StateFlags\" \"4\"", "\"StateFlags\" \"1026\""),
        ("\"buildid\" \"25730807\"", "\"buildid\" \"0\""),
        (
            "\"TargetBuildID\" \"25730807\"",
            "\"TargetBuildID\" \"25730808\"",
        ),
        ("\"BytesDownloaded\" \"123\"", "\"BytesDownloaded\" \"122\""),
        ("\"BytesStaged\" \"456\"", "\"BytesStaged\" \"455\""),
        (
            "\"BytesToDownload\" \"123\"",
            "\"BytesToDownload\" \"invalid\"",
        ),
    ] {
        assert!(
            !installed_app_state(&valid.replacen(from, to, 1), 896660),
            "accepted: {to}"
        );
    }
}

#[test]
fn malformed_or_shadowed_app_identity_cannot_authorize_metadata_refresh() {
    for fields in [
        r#""UserConfig" { "appid" "896660" "StateFlags" "4" "buildid" "1" }"#,
        r#""appid" "896660" "appid" "42" "StateFlags" "4" "buildid" "1""#,
        r#""appid" "896660" "appid" { "value" "42" } "StateFlags" "4" "buildid" "1""#,
        r#""appid" "896660" "StateFlags" "4" "STATEFLAGS" "1026" "buildid" "1""#,
        r#""appid" "896660" "StateFlags" "4" "buildid" "18446744073709551616""#,
        r#""appid" "896660" "StateFlags" "4" "buildid" "1" "unfinished" {"#,
    ] {
        assert!(
            !installed_app_state(&app_state(fields), 896660),
            "accepted: {fields}"
        );
    }
    let valid = app_state(r#""appid" "896660" "StateFlags" "4" "buildid" "1""#);
    assert!(!installed_app_state(&(valid.clone() + &valid), 896660));
}
