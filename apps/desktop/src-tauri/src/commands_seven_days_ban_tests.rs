use super::*;

const TARGET: &str = "EOS_00000000000000000000000000000001";
const OWNER: &str = "Steam_76561198000000002";
const EXPIRY: &str = "2036-09-28 00:12:34";

fn primary(target: &str) -> String {
    format!("{target} banned until {EXPIRY}, reason: LanGame.\r\n")
}

fn owner(target: &str) -> String {
    format!(
        "Steam Family Sharing license owner {target} banned until {EXPIRY}, reason: LanGame.\r\n"
    )
}

#[test]
fn native_ban_receipts_keep_exact_account_expiry_and_reason_for_xml_confirmation() {
    let text = format!(
        "Please enter password:\r\nLogon successful.\r\n*** Connected with 7DTD server.\r\n{}",
        primary(TARGET)
    );
    let receipts = confirmed_ban_receipts(Some(&text), TARGET).unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].canonical_target, TARGET);
    assert_eq!(receipts[0].unban_date, EXPIRY);
    assert_eq!(receipts[0].reason, "LanGame");
    let short = confirmed_ban_receipts(Some(&primary("EOS_deadbeef")), "EOS_deadbeef").unwrap();
    assert_eq!(short[0].canonical_target, "EOS_deadbeef");
    let family = confirmed_ban_receipts(
        Some(&format!("{}{}", primary(TARGET), owner(OWNER))),
        TARGET,
    )
    .unwrap();
    assert_eq!(family.len(), 2);
    assert_eq!(family[1].canonical_target, OWNER);
    assert_eq!(family[1].unban_date, family[0].unban_date);
    let logged = confirmed_ban_receipts(Some(&format!(
        "{}2026-09-28T00:12:34 1.000 INF Player disconnected\r\n{}2026-09-28T00:12:35 2.000 INF Connection closed\r\n",
        primary(TARGET), owner(OWNER)
    )), TARGET).unwrap();
    assert_eq!(logged, family);
}

#[test]
fn native_ban_receipts_reject_wrong_targets_partial_duplicate_and_malformed_output() {
    for response in [
        String::new(),
        String::from("Logon successful.\n"),
        primary(OWNER),
        primary(TARGET).trim_end().to_string(),
        primary(TARGET).replace("LanGame.", "another reason."),
        primary(TARGET).replace(EXPIRY, "2036-09-28"),
        primary(TARGET).replace(EXPIRY, "2036-09-28 00:12:34 trailing"),
        format!("{}{}", primary(TARGET), primary(TARGET)),
        format!("{}{}", owner(OWNER), primary(TARGET)),
        format!("{}{}", primary(TARGET), owner("EOS_deadbeef")),
        format!("{}{}", primary(TARGET), owner(TARGET)),
        format!(
            "{}{}{}",
            primary(TARGET),
            owner(OWNER),
            owner("Steam_76561198000000003")
        ),
        format!(
            "{}{}",
            primary(TARGET),
            owner(OWNER).replace(EXPIRY, "2036-09-28 00:12:35")
        ),
        format!(
            "{}*** ERROR: Executing command 'ban' failed\r\n",
            primary(TARGET)
        ),
        format!("*** ERROR: unknown command 'ban'\r\n{}", primary(TARGET)),
        format!(
            "{}Steam Family Sharing license owner malformed\r\n",
            primary(TARGET)
        ),
        primary("EOS_invalidvalue"),
    ] {
        assert!(
            confirmed_ban_receipts(Some(&response), TARGET).is_err(),
            "must reject {response:?}"
        );
    }
    assert!(confirmed_ban_receipts(None, TARGET).is_err());
}

#[test]
fn native_ban_receipts_do_not_extract_confirmation_from_names_or_interleaved_logs() {
    for response in [
        format!("Player '{}", primary(TARGET)),
        format!("2026-09-28T00:12:34 1.000 INF Player '{}", primary(TARGET)),
        format!(
            "{}2026-09-28T00:12:34 1.000 INFO Name:\r\n{}",
            primary(TARGET),
            owner(OWNER)
        ),
        format!("{}{}", primary(TARGET), primary(TARGET)),
        format!("Name\r\n{}*** ERROR: ban failed\r\n", primary(TARGET)),
    ] {
        assert!(
            confirmed_ban_receipts(Some(&response), TARGET).is_err(),
            "must reject {response:?}"
        );
    }
}
