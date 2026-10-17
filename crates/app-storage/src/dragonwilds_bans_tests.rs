use super::*;
use std::path::PathBuf;

const FIRST: &str = "11111111111111111111111111111111";
const SECOND: &str = "22222222222222222222222222222222";

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new(content: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-dragonwilds-ban-roster-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join(FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        Self { root, path }
    }

    fn read(&self) -> Result<DragonwildsBanRoster, StorageError> {
        read_dragonwilds_ban_roster(&self.root, "fixture-instance")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.root.is_absolute());
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-dragonwilds-ban-roster-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn row(account: &str, name: &str, banned: bool) -> String {
    format!(
        "KnownPlayerList=(UserId={account},UserName={name},Privileges=(PrivilegeMask=14),bIsBanned={})",
        if banned { "True" } else { "False" }
    )
}

fn document(rows: &[String]) -> String {
    format!("{SECTION}\r\n{}\r\n", rows.join("\r\n"))
}

#[test]
fn existing_persisted_bans_survive_reader_recreation_without_online_state_or_writes() {
    let content = document(&[
        row(SECOND, "Former visitor", false),
        row(FIRST, "Existing ban", true),
    ]);
    let fixture = Fixture::new(content.as_bytes());
    let first = fixture.read().unwrap();
    let after_restart = read_dragonwilds_ban_roster(&fixture.root, "fixture-instance").unwrap();
    assert_eq!(first.entries, after_restart.entries);
    assert_eq!(
        first.entries,
        [DragonwildsBanEntry {
            account_id: FIRST.into(),
            display_name: Some("Existing ban".into())
        }]
    );
    assert_eq!(first.instance_id, "fixture-instance");
    assert!(first.observed_at_unix_ms > 0);
    assert_eq!(fs::read(&fixture.path).unwrap(), content.as_bytes());
    let serialized = serde_json::to_value(&first).unwrap();
    assert!(serialized.get("observed_at_unix_ms").is_some());
    assert!(serialized.get("instanceId").is_none());
}

#[test]
fn unicode_commas_and_duplicate_names_preserve_distinct_account_identities() {
    let fixture = Fixture::new(
        document(&[
            row(SECOND, "符文,旅者", true),
            row(FIRST, "符文,旅者", true),
        ])
        .as_bytes(),
    );
    let roster = fixture.read().unwrap();
    assert_eq!(roster.entries.len(), 2);
    assert_eq!(roster.entries[0].account_id, FIRST);
    assert_eq!(roster.entries[1].account_id, SECOND);
    assert!(
        roster
            .entries
            .iter()
            .all(|entry| entry.display_name.as_deref() == Some("符文,旅者"))
    );
}

#[test]
fn supported_native_encodings_have_identical_lossless_projection() {
    let content = document(&[row(FIRST, "Rune 🐉,玩家", true)]);
    let utf8 = [vec![0xef, 0xbb, 0xbf], content.as_bytes().to_vec()].concat();
    let mut utf16 = vec![0xff, 0xfe];
    for unit in content.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let plain = Fixture::new(content.as_bytes()).read().unwrap().entries;
    for bytes in [utf8, utf16] {
        assert_eq!(Fixture::new(&bytes).read().unwrap().entries, plain);
    }
}

#[test]
fn invalid_encoding_is_an_error_instead_of_an_empty_roster() {
    for bytes in [
        vec![0xff],
        vec![0xff, 0xfe, 0x01],
        vec![0xff, 0xfe, 0x00, 0xd8],
        vec![0xfe, 0xff, 0, 0],
    ] {
        assert!(Fixture::new(&bytes).read().is_err());
    }
}

#[test]
fn only_one_complete_target_section_can_produce_an_empty_roster() {
    assert!(
        Fixture::new(document(&[]).as_bytes())
            .read()
            .unwrap()
            .entries
            .is_empty()
    );
    for content in ["", "[OtherSection]\n", &format!("{SECTION}\n{SECTION}\n")] {
        assert!(Fixture::new(content.as_bytes()).read().is_err());
    }
    let fixture = Fixture::new(document(&[]).as_bytes());
    fs::remove_file(&fixture.path).unwrap();
    assert!(fixture.read().is_err());
    fs::create_dir(&fixture.path).unwrap();
    assert!(fixture.read().is_err());
}

#[test]
fn malformed_or_ambiguous_account_and_ban_flag_records_fail_closed() {
    let valid = row(FIRST, "Player", true);
    for malformed in [
        valid.replace(FIRST, "A1111111111111111111111111111111"),
        valid.replace(FIRST, "short"),
        valid.replace(FIRST, "g1111111111111111111111111111111"),
        valid.replace("UserId=", "Account="),
        valid.replace("bIsBanned=True", "bIsBanned=true"),
        valid.replace(",bIsBanned=True", ""),
        format!("{valid} trailing"),
        format!("+{valid}"),
        "KnownPlayerList".into(),
    ] {
        assert!(
            Fixture::new(document(&[malformed]).as_bytes())
                .read()
                .is_err()
        );
    }
}

#[test]
fn duplicate_accounts_are_rejected_even_when_one_record_is_not_banned() {
    for banned in [false, true] {
        assert!(
            Fixture::new(
                document(&[row(FIRST, "One", true), row(FIRST, "Two", banned)]).as_bytes()
            )
            .read()
            .is_err()
        );
    }
}

#[test]
fn all_known_rows_count_toward_the_native_record_limit() {
    let mut rows = (0..MAX_ROWS)
        .map(|index| row(&format!("{index:032x}"), "Visitor", false))
        .collect::<Vec<_>>();
    assert!(
        Fixture::new(document(&rows).as_bytes())
            .read()
            .unwrap()
            .entries
            .is_empty()
    );
    rows.push(row(&format!("{MAX_ROWS:032x}"), "Visitor", false));
    assert!(Fixture::new(document(&rows).as_bytes()).read().is_err());
}

#[test]
fn oversized_regular_file_is_rejected_before_reading_its_content() {
    let fixture = Fixture::new(document(&[]).as_bytes());
    OpenOptions::new()
        .write(true)
        .open(&fixture.path)
        .unwrap()
        .set_len(MAX_BYTES as u64 + 1)
        .unwrap();
    assert!(fixture.read().is_err());
}

#[test]
fn future_fields_credentials_and_suspicious_names_never_become_display_labels() {
    for middle in [
        "UserName=Player,LastAdminPassword=synthetic-test-only,Privileges=(PrivilegeMask=14)",
        "UserName=Player,Privileges=(PrivilegeMask=14),FuturePassword=synthetic-test-only",
        "UserName=Player,FutureField=synthetic-test-only,Privileges=(PrivilegeMask=14)",
        "UserName=synthetic-test-only=secret,Privileges=(PrivilegeMask=14)",
        "UserName=synthetic-test-only\\secret,Privileges=(PrivilegeMask=14)",
    ] {
        let content = format!(
            "{SECTION}\nAdminPassword=synthetic-test-only\nKnownPlayerList=(UserId={FIRST},{middle},bIsBanned=True)\n"
        );
        let roster = Fixture::new(content.as_bytes()).read().unwrap();
        assert_eq!(roster.entries[0].display_name, None);
        let json = serde_json::to_string(&roster).unwrap();
        assert!(!json.contains("synthetic-test-only") && !json.contains("Password"));
    }
    let fixture =
        Fixture::new(format!("{SECTION}\nKnownPlayerList=synthetic-test-only\n").as_bytes());
    assert!(
        !fixture
            .read()
            .unwrap_err()
            .to_string()
            .contains("synthetic-test-only")
    );
}

#[test]
fn records_in_other_sections_are_not_returned_as_native_bans() {
    let content = format!(
        "[OtherSection]\n{}\n{SECTION}\n{}\n",
        row(FIRST, "Other", true),
        row(SECOND, "Native", true)
    );
    let roster = Fixture::new(content.as_bytes()).read().unwrap();
    assert_eq!(roster.entries.len(), 1);
    assert_eq!(roster.entries[0].account_id, SECOND);
}

#[cfg(unix)]
#[test]
fn symbolic_configuration_files_and_ancestor_directories_are_rejected() {
    use std::os::unix::fs::symlink;
    let target = Fixture::new(document(&[row(FIRST, "Player", true)]).as_bytes());
    let fixture = Fixture::new(document(&[]).as_bytes());
    fs::remove_file(&fixture.path).unwrap();
    symlink(&target.path, &fixture.path).unwrap();
    assert!(fixture.read().is_err());
    fs::remove_file(&fixture.path).unwrap();
    let link = fixture.root.join("linked-runtime");
    symlink(&target.root, &link).unwrap();
    assert!(read_dragonwilds_ban_roster(&link, "fixture-instance").is_err());
    fs::remove_file(link).unwrap();
    assert!(target.read().is_ok());
}

#[cfg(windows)]
#[test]
fn junction_runtime_roots_are_rejected_without_reading_the_target() {
    let target = Fixture::new(document(&[row(FIRST, "Player", true)]).as_bytes());
    let fixture = Fixture::new(document(&[]).as_bytes());
    let link = fixture.root.join("linked-runtime");
    let result = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target.root)
        .output()
        .unwrap();
    assert!(result.status.success());
    let read = read_dragonwilds_ban_roster(&link, "fixture-instance");
    // Remove the owned junction itself before recursive fixture cleanup.
    fs::remove_dir(&link).unwrap();
    assert!(read.is_err());
    assert!(target.read().is_ok());
}
