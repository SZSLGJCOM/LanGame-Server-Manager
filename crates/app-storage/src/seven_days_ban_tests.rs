use super::*;
use crate::settings_value_formats::is_date_or_local_datetime;

const TARGET: &str = "Steam_76561198000000001";

fn document(users: &str, blacklist: &str) -> String {
    format!("<adminTools><blacklist>{blacklist}</blacklist><users>{users}</users></adminTools>")
}

fn row(platform: &str, userid: &str) -> String {
    format!(
        r#"<blacklisted platform="{platform}" userid="{userid}" name="Alice &amp; Bob" unbandate="2036-02-29 23:59:58" reason="Native reason &quot;quoted&quot;"/>"#
    )
}

fn schema() -> Value {
    serde_json::from_str(include_str!("../../../modules/sevendaystodie/schema.json")).unwrap()
}

#[test]
fn native_ban_uses_the_exact_platform_account_and_preserves_xml_metadata() {
    let source = document(
        "",
        &format!(
            "{}{}",
            row("EOS", "76561198000000001"),
            row("Steam", "76561198000000001")
        ),
    );
    let confirmed = xml::read_confirmed_ban(&source, TARGET).unwrap();
    assert!(!confirmed.target_is_admin);
    assert_eq!(confirmed.entry["name"], "Alice & Bob");
    assert_eq!(confirmed.entry["reason"], "Native reason \"quoted\"");
    assert_eq!(confirmed.entry["unbandate"], "2036-02-29 23:59:58");
    assert_eq!(confirmed.entry["platform"], "Steam");
    let eos =
        xml::read_confirmed_ban(&document("", &row("EOS", "abcdef12")), "EOS_abcdef12").unwrap();
    assert_eq!(eos.entry["userid"], "abcdef12");
}

#[test]
fn native_ban_requires_complete_unambiguous_safe_xml() {
    let valid = row("Steam", "76561198000000001");
    for source in [
        document("", &row("EOS", "76561198000000001")),
        document("", &format!("{valid}{valid}")),
        format!(
            "<!DOCTYPE adminTools [<!ENTITY name 'Alice'>]>{}",
            document("", &valid)
        ),
        document("", &valid.replace("Alice &amp; Bob", "&#10;Alice")),
        document("", &valid.replace("Alice &amp; Bob", "Alice\nBob")),
        document("", &valid.replace("Alice &amp; Bob", "&unknown;")),
        document(
            "",
            &valid.replace("platform=", "platform=\"Steam\" platform="),
        ),
        document("", &valid).replace("</users>", "</blacklist>"),
        format!("<adminTools><blacklist>{valid}</blacklist></adminTools>"),
        format!("<adminTools><users/><users/><blacklist>{valid}</blacklist></adminTools>"),
        document("", &valid).replace("</adminTools>", "<other/>"),
        document("", &valid.replace("/>", "><nested/></blacklisted>")),
        document("", &valid.replace("/>", ">bad text</blacklisted>")),
        document("", &valid).replace("<users>", "<?task run?><users>"),
        " ".repeat(MAX_DOCUMENT_BYTES + 1),
        "\n".repeat(8_193),
    ] {
        assert!(
            xml::read_confirmed_ban(&source, TARGET).is_err(),
            "accepted unsafe XML: {source}"
        );
    }
    for target in [
        "76561198000000001",
        "Steam_bad",
        "EOS_abcdef1",
        "EOS_gggggggg",
        "EOS_012345678901234567890123456789012",
        "PSN_Alice",
        "Steam_76561198000000001\n",
    ] {
        assert!(
            xml::account_identity(target).is_err(),
            "accepted unsafe target: {target}"
        );
    }
}

#[test]
fn confirmed_ban_merges_only_that_account_and_its_proven_admin_revocation() {
    let original = json!({
        "server_name":"Pending next start", "max_players":19,
        "admin_users":[
            {"platform":"Steam", "userid":"76561198000000001", "permission_level":0},
            {"platform":"EOS", "userid":"76561198000000001", "permission_level":5}
        ],
        "whitelist_users":[{"platform":"Steam", "userid":"76561198000000001"}],
        "blacklist_entries":[
            {"platform":"Steam", "userid":"76561198000000001", "unbandate":"2030-01-01", "custom":"keep"},
            {"platform":"Steam", "userid":"76561198000000002", "reason":"other", "unbandate":"9999-12-31"}
        ]
    });
    let mut settings = original.as_object().unwrap().clone();
    let native = document(
        "",
        &format!(
            "{}{}",
            row("Steam", "76561198000000001"),
            row("Steam", "76561198000000009")
        ),
    );
    merge_confirmed(
        &mut settings,
        &schema(),
        xml::read_confirmed_ban(&native, TARGET).unwrap(),
        TARGET,
    )
    .unwrap();
    assert_eq!(settings[FIELD].as_array().unwrap().len(), 2);
    assert_eq!(settings[FIELD][0]["custom"], "keep");
    assert_eq!(settings[FIELD][0]["unbandate"], "2036-02-29 23:59:58");
    assert_eq!(settings[FIELD][1], original[FIELD][1]);
    assert_eq!(settings["admin_users"], json!([original["admin_users"][1]]));
    for key in ["server_name", "max_players", "whitelist_users"] {
        assert_eq!(settings[key], original[key]);
    }
    let users = r#"<user platform="Steam" userid="76561198000000001" permission_level="0"/>"#;
    let mut retained = original.as_object().unwrap().clone();
    merge_confirmed(
        &mut retained,
        &schema(),
        xml::read_confirmed_ban(&document(users, &row("Steam", "76561198000000001")), TARGET)
            .unwrap(),
        TARGET,
    )
    .unwrap();
    assert_eq!(retained["admin_users"], original["admin_users"]);
}

#[test]
fn native_expiry_is_calendar_checked_and_preserved_to_the_second() {
    for value in [
        "2024-02-29",
        "9999-12-31",
        "2036-02-29 00:00:00",
        "2036-02-29 23:59:59",
    ] {
        assert!(is_date_or_local_datetime(value));
        let source = document(
            "",
            &row("Steam", "76561198000000001").replace("2036-02-29 23:59:58", value),
        );
        let mut settings = Map::new();
        merge_confirmed(
            &mut settings,
            &schema(),
            xml::read_confirmed_ban(&source, TARGET).unwrap(),
            TARGET,
        )
        .unwrap();
        assert_eq!(settings[FIELD][0]["unbandate"], value);
    }
    for value in [
        "0000-01-01",
        "2035-02-29",
        "2036-04-31",
        "2036-02-29 24:00:00",
        "2036-02-29 23:60:00",
        "2036-02-29 23:59:60",
        "2036-02-29 1:00:00",
        "2036-02-29T01:00:00Z",
        "2036-02-29 12:00:00 ",
    ] {
        assert!(!is_date_or_local_datetime(value), "accepted {value}");
    }
    let source = document(
        "",
        &row("Steam", "76561198000000001").replace("2036-02-29 23:59:58", "2035-02-29 00:00:00"),
    );
    assert!(
        merge_confirmed(
            &mut Map::new(),
            &schema(),
            xml::read_confirmed_ban(&source, TARGET).unwrap(),
            TARGET
        )
        .is_err()
    );
}

#[test]
fn receipts_bind_one_root_and_at_most_one_distinct_steam_owner() {
    let receipt = SevenDaysBanReceipt {
        canonical_target: TARGET.to_string(),
        unban_date: "2036-02-29 23:59:58".to_string(),
        reason: "LanGame".to_string(),
    };
    assert!(validate_receipts(TARGET, std::slice::from_ref(&receipt)).is_ok());
    assert!(validate_receipts(TARGET, &[]).is_err());
    assert!(validate_receipts("EOS_abcdef12", std::slice::from_ref(&receipt)).is_err());
    assert!(validate_receipts(TARGET, &[receipt.clone(), receipt.clone()]).is_err());
    let mut owner = receipt.clone();
    owner.canonical_target = "Steam_76561198000000002".to_string();
    assert!(validate_receipts(TARGET, &[receipt.clone(), owner.clone()]).is_ok());
    assert!(validate_receipts(TARGET, &[receipt.clone(), owner.clone(), owner.clone()]).is_err());
    for invalid in [
        SevenDaysBanReceipt {
            canonical_target: "EOS_abcdef12".to_string(),
            ..owner.clone()
        },
        SevenDaysBanReceipt {
            unban_date: "2036-03-01 00:00:00".to_string(),
            ..owner.clone()
        },
        SevenDaysBanReceipt {
            reason: "different".to_string(),
            ..owner.clone()
        },
        SevenDaysBanReceipt {
            unban_date: "2036-02-29".to_string(),
            ..owner.clone()
        },
        SevenDaysBanReceipt {
            unban_date: "2036-02-30 00:00:00".to_string(),
            ..owner.clone()
        },
    ] {
        assert!(validate_receipts(TARGET, &[receipt.clone(), invalid]).is_err());
    }
}
