use super::*;
use std::fs;
use std::path::PathBuf;

const STEAM: &str = "76561198000000001";
const OTHER: &str = "76561198000000002";

struct Fixture {
    root: PathBuf,
    runtime: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-native-ban-roster-{}", uuid::Uuid::new_v4()));
        let runtime = root.join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        Self { root, runtime }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.runtime.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn read(&self, module: &str) -> Result<NativeBanRoster, StorageError> {
        read_native_ban_roster(&self.runtime, &self.root, module, "fixture-instance")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-native-ban-roster-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn utf16(content: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(content.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[test]
fn missing_verified_optional_rosters_are_empty_without_creating_native_paths() {
    let fixture = Fixture::new();
    for module in [
        "rimworld",
        "soulmask",
        "squad",
        "necesse",
        "unturned",
        "conanexiles",
        "palworld",
        "arksurvivalascended",
        "arksurvivalevolved",
    ] {
        let roster = fixture.read(module).unwrap();
        assert!(roster.entries.is_empty());
        assert_eq!(roster.module_id, module);
        assert_eq!(roster.instance_id, "fixture-instance");
        assert!(roster.observed_at_unix_ms > 0);
    }
    assert_eq!(fs::read_dir(&fixture.runtime).unwrap().count(), 0);
    assert!(!fixture.root.join("data").exists());
    assert!(fixture.read("projectzomboid").is_err());
    assert!(
        read_native_ban_roster(
            &fixture.root.join("missing-runtime"),
            &fixture.root,
            "rimworld",
            "instance"
        )
        .is_err()
    );
}

#[test]
fn conan_projects_native_id_and_name_but_palworld_guid_metadata_is_never_a_target_or_name() {
    let fixture = Fixture::new();
    let conan = b"NativeUser_123:Former player:with colon\r\n";
    let conan_path = fixture.write("ConanSandbox/Saved/blacklist.txt", conan);
    let entry = fixture.read("conanexiles").unwrap().entries.remove(0);
    assert_eq!(entry.account_id, "NativeUser_123");
    assert_eq!(
        entry.display_name.as_deref(),
        Some("Former player:with colon")
    );
    assert_eq!(entry.identity_kind, RuntimePlayerIdentityKind::ConanUserId);
    assert_eq!(fs::read(conan_path).unwrap(), conan);
    let palworld = b"steam_76561198000000001,00112233445566778899AABBCCDDEEFF\r\nEOS_00112233445566778899aabbccddeeff\n";
    let palworld_path = fixture.write("Pal/Saved/SaveGames/banlist.txt", palworld);
    let entries = fixture.read("palworld").unwrap().entries;
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.display_name.is_none()
        && entry.identity_kind == RuntimePlayerIdentityKind::PalworldUserId));
    assert!(
        entries
            .iter()
            .any(|entry| entry.account_id == "steam_76561198000000001")
    );
    assert_eq!(fs::read(palworld_path).unwrap(), palworld);
    fixture.write(
        "Pal/Saved/SaveGames/banlist.txt",
        b"00112233445566778899AABBCCDDEEFF\n",
    );
    let readonly_guid = fixture.read("palworld").unwrap().entries.remove(0);
    assert_eq!(readonly_guid.account_id, "00112233445566778899aabbccddeeff");
    assert_eq!(
        readonly_guid.identity_kind,
        RuntimePlayerIdentityKind::PalworldPlayerUid
    );
    fixture.write(
        "Pal/Saved/SaveGames/banlist.txt",
        b"foo,00112233445566778899AABBCCDDEEFF,ignored\n,00112233-4455-6677-8899-aabbccddeeff\n",
    );
    let same_guid = fixture.read("palworld").unwrap().entries;
    assert_eq!(same_guid.len(), 1);
    assert_eq!(same_guid[0].account_id, "00112233445566778899aabbccddeeff");
    assert_eq!(
        same_guid[0].identity_kind,
        RuntimePlayerIdentityKind::PalworldPlayerUid
    );
    fixture.write(
        "Pal/Saved/SaveGames/banlist.txt",
        b"00000000000000000000000000000000\n",
    );
    assert!(fixture.read("palworld").is_err());
}

#[test]
fn ark_reads_only_the_default_windows_native_file_and_rejects_unverified_row_shapes() {
    let fixture = Fixture::new();
    fixture.write(
        "ShooterGame/Saved/BanList.txt",
        format!("{OTHER}\n").as_bytes(),
    );
    for module in ["arksurvivalascended", "arksurvivalevolved"] {
        assert!(fixture.read(module).unwrap().entries.is_empty());
    }
    for module in ["arksurvivalascended", "arksurvivalevolved"] {
        let expiry = if module == "arksurvivalascended" {
            ",0"
        } else {
            ""
        };
        let content = format!(
            "{STEAM},Steam visitor{expiry}\n1234567890123456789,ARK visitor{expiry}\n00112233445566778899aabbccddeeff,Epic visitor{expiry}\n"
        );
        let path = fixture.write("ShooterGame/Binaries/Win64/BanList.txt", content.as_bytes());
        let entries = fixture.read(module).unwrap().entries;
        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .any(|entry| entry.identity_kind == RuntimePlayerIdentityKind::SteamId)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.identity_kind == RuntimePlayerIdentityKind::ArkAccountId)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.identity_kind == RuntimePlayerIdentityKind::EosId)
        );
        assert_eq!(fs::read(&path).unwrap(), content.as_bytes());
    }
    fixture.write(
        "ShooterGame/Binaries/Win64/BanList.txt",
        b"not-an-account,Name,0\n",
    );
    assert!(fixture.read("arksurvivalascended").is_err());
}

#[test]
fn ark_native_csv_keeps_account_name_and_permanent_expiration_without_writing() {
    let fixture = Fixture::new();
    let account = "00112233445566778899aabbccddeeff";
    let bytes = format!("# retained native comment\n{account},Offline visitor,0\r\n");
    let path = fixture.write("ShooterGame/Binaries/Win64/BanList.txt", bytes.as_bytes());
    let entries = fixture.read("arksurvivalascended").unwrap().entries;
    assert_eq!(
        entries,
        [NativeBanEntry {
            account_id: account.into(),
            display_name: Some("Offline visitor".into()),
            identity_kind: RuntimePlayerIdentityKind::EosId,
        }]
    );
    assert_eq!(fs::read(path).unwrap(), bytes.as_bytes());
}

#[test]
fn ark_asa_native_expiration_uses_unsigned_seconds_and_the_strict_greater_boundary() {
    let path = Path::new("BanList.txt");
    let account = "00112233445566778899aabbccddeeff";
    let content = format!("{account},Temporary,100\n{STEAM},Permanent,0\n");
    let boundary =
        crate::native_bans_text::parse(&content, "arksurvivalascended", path, 100_999).unwrap();
    assert_eq!(boundary.len(), 2);
    let expired =
        crate::native_bans_text::parse(&content, "arksurvivalascended", path, 101_000).unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].account_id, STEAM);
    let far_future = format!("{account},Maximum,4294967295\n");
    assert_eq!(
        crate::native_bans_text::parse(
            &far_future,
            "arksurvivalascended",
            path,
            u64::from(u32::MAX) * 1000
        )
        .unwrap()
        .len(),
        1
    );
    assert!(
        crate::native_bans_text::parse(
            &far_future,
            "arksurvivalascended",
            path,
            (u64::from(u32::MAX) + 1) * 1000
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn ark_native_asa_and_ase_csv_shapes_are_distinct_and_bad_expiry_is_not_ignored() {
    let path = Path::new("BanList.txt");
    for row in [
        format!("{STEAM},Name,"),
        format!("{STEAM},Name,-1"),
        format!("{STEAM},Name,+1"),
        format!("{STEAM},Name,4294967296"),
        format!("{STEAM},Name,not-expiry"),
    ] {
        assert!(
            crate::native_bans_text::parse(&row, "arksurvivalascended", path, 1_000_000).is_err()
        );
    }
    for row in [
        STEAM.to_owned(),
        format!("{STEAM},Name"),
        format!("{STEAM},Name,0,native-ignored-metadata"),
    ] {
        let entries =
            crate::native_bans_text::parse(&row, "arksurvivalascended", path, u64::MAX).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].account_id, STEAM);
        assert!(
            !serde_json::to_string(&entries)
                .unwrap()
                .contains("native-ignored-metadata")
        );
    }
    let ase = format!("{STEAM},Offline visitor\n");
    let entries =
        crate::native_bans_text::parse(&ase, "arksurvivalevolved", path, u64::MAX).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name.as_deref(), Some("Offline visitor"));
    let comma_name = format!("{STEAM},Offline visitor,100\n");
    let entries =
        crate::native_bans_text::parse(&comma_name, "arksurvivalevolved", path, u64::MAX).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].display_name.as_deref(),
        Some("Offline visitor,100")
    );
    assert!(
        crate::native_bans_text::parse(STEAM, "arksurvivalevolved", path, 1_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ark_native_duplicate_accounts_use_the_last_record_before_evaluating_expiration() {
    let path = Path::new("BanList.txt");
    let expired_last = format!("{STEAM},Earlier permanent,0\n{STEAM},Later expired,100\n");
    assert!(
        crate::native_bans_text::parse(&expired_last, "arksurvivalascended", path, 101_000)
            .unwrap()
            .is_empty()
    );
    let active_last = format!("{STEAM},Earlier expired,100\n{STEAM},Later permanent,0\n");
    let entries =
        crate::native_bans_text::parse(&active_last, "arksurvivalascended", path, 101_000).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name.as_deref(), Some("Later permanent"));
    let ase = format!("{STEAM},Earlier name\n{STEAM},Last name,100\n");
    let entries =
        crate::native_bans_text::parse(&ase, "arksurvivalevolved", path, u64::MAX).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name.as_deref(), Some("Last name,100"));
}

#[test]
fn soulmask_native_file_is_independent_of_online_state_and_read_only_in_supported_encodings() {
    let fixture = Fixture::new();
    let content = format!("{STEAM}\r\n{OTHER}\r\n{STEAM}\r\n");
    for bytes in [
        content.as_bytes().to_vec(),
        [&[0xef, 0xbb, 0xbf][..], content.as_bytes()].concat(),
        utf16(&content),
    ] {
        let path = fixture.write("WS/Saved/BlackAccountList.txt", &bytes);
        let roster = fixture.read("soulmask").unwrap();
        assert_eq!(roster.entries.len(), 2);
        assert_eq!(roster.entries[0].account_id, STEAM);
        assert_eq!(
            roster.entries[0].identity_kind,
            RuntimePlayerIdentityKind::SteamId
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn soulmask_epic_compound_accounts_preserve_both_halves_and_remain_distinct_from_steam() {
    let fixture = Fixture::new();
    let epic = "ABCDEF0123456789abcdef0123456789|00112233445566778899AABBCCDDEEFF";
    let content = format!("{STEAM}\r\n{epic}\r\n{epic}\r\n");
    let path = fixture.write("WS/Saved/BlackAccountList.txt", content.as_bytes());
    let roster = fixture.read("soulmask").unwrap();
    assert_eq!(roster.entries.len(), 2);
    assert!(roster.entries.iter().any(|entry| entry.account_id == STEAM
        && entry.identity_kind == RuntimePlayerIdentityKind::SteamId));
    let entry = roster
        .entries
        .iter()
        .find(|entry| entry.account_id == epic)
        .unwrap();
    assert_eq!(
        entry.identity_kind,
        RuntimePlayerIdentityKind::SoulmaskAccountId
    );
    assert_eq!(entry.display_name, None);
    assert_eq!(
        serde_json::to_value(entry).unwrap()["identity_kind"],
        "soulmask_account_id"
    );
    assert_eq!(fs::read(path).unwrap(), content.as_bytes());
}

#[test]
fn soulmask_rejects_incomplete_malformed_or_split_epic_account_identities() {
    let left = "00112233445566778899aabbccddeeff";
    let right = "ffeeddccbbaa99887766554433221100";
    for account in [
        format!("|{right}"),
        format!("{left}|"),
        format!("{left}|{right}|{right}"),
        format!("{}|{right}", &left[..31]),
        format!("{left}|{}", &right[..31]),
        format!("{left}|z{}", &right[1..]),
        format!("\"{left}|{right}\""),
    ] {
        let fixture = Fixture::new();
        fixture.write("WS/Saved/BlackAccountList.txt", account.as_bytes());
        assert!(fixture.read("soulmask").is_err());
    }
}

#[test]
fn necesse_reads_instance_data_root_and_uses_its_own_native_subject_namespace() {
    let fixture = Fixture::new();
    let path = fixture.root.join("data/cfg/banned.cfg");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = b"ExampleName\n123456789\nexampleName\n";
    fs::write(&path, bytes).unwrap();
    fixture.write("data/cfg/banned.cfg", b"wrong-runtime-copy\n");
    let entries = fixture.read("necesse").unwrap().entries;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.account_id.as_str())
            .collect::<Vec<_>>(),
        ["123456789", "examplename"]
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.identity_kind == RuntimePlayerIdentityKind::NecesseBanId)
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
    fs::write(
        fixture.root.join("data/cfg/banned.cfg"),
        b"\xef\xbb\xbfexample\n",
    )
    .unwrap();
    assert!(fixture.read("necesse").is_err());
}

#[test]
fn rimworld_projects_username_and_boolean_ban_without_credentials_or_ip() {
    let fixture = Fixture::new();
    let banned = br#"{"Username":"FormerVisitor","IsBanned":true,"Password":"synthetic-private-password","LatestIP":"synthetic-private-ip","Hash":"synthetic-private-hash"}"#;
    let path = fixture.write("Assets/Users/FormerVisitor.json", banned);
    fixture.write(
        "Assets/Users/Allowed.json",
        br#"{"Username":"Allowed","IsBanned":false}"#,
    );
    let roster = fixture.read("rimworld").unwrap();
    assert_eq!(
        roster.entries,
        [NativeBanEntry {
            account_id: "FormerVisitor".into(),
            display_name: Some("FormerVisitor".into()),
            identity_kind: RuntimePlayerIdentityKind::PlayerName,
        }]
    );
    assert!(
        !serde_json::to_string(&roster)
            .unwrap()
            .contains("synthetic-private")
    );
    assert_eq!(fs::read(path).unwrap(), banned);
    assert_eq!(fixture.read("rimworld").unwrap().entries, roster.entries);
}

#[test]
fn malformed_player_files_duplicate_accounts_and_native_text_fail_closed() {
    for content in [
        br#"{"Username":"Name"}"#.as_slice(),
        br#"{"Username":"Name","IsBanned":"true"}"#.as_slice(),
        br#"{"Username":"Name","IsBanned":false,"IsBanned":true}"#.as_slice(),
        br#"{"Username":"","IsBanned":true}"#.as_slice(),
        b"not-json".as_slice(),
    ] {
        let fixture = Fixture::new();
        fixture.write("Assets/Users/User.json", content);
        assert!(fixture.read("rimworld").is_err());
    }
    let fixture = Fixture::new();
    for path in ["Assets/Users/one.json", "Assets/Users/two.json"] {
        fixture.write(path, br#"{"Username":"Duplicate","IsBanned":true}"#);
    }
    assert!(fixture.read("rimworld").is_err());
    for bytes in [b"not-a-steam-id".as_slice(), &[0xff], &[0xff, 0xfe, 0x01]] {
        let fixture = Fixture::new();
        fixture.write("WS/Saved/BlackAccountList.txt", bytes);
        assert!(fixture.read("soulmask").is_err());
    }
}

#[test]
fn native_file_size_record_capacity_and_path_traversal_are_rejected() {
    let fixture = Fixture::new();
    let path = fixture.write("WS/Saved/BlackAccountList.txt", b"");
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(MAX_BYTES as u64 + 1)
        .unwrap();
    assert!(fixture.read("soulmask").is_err());
    fixture.write(
        "WS/Saved/BlackAccountList.txt",
        format!("{STEAM}\n").repeat(MAX_ROWS + 1).as_bytes(),
    );
    assert!(fixture.read("soulmask").is_err());
    for id in ["../outside", "..\\outside", "C:outside", ".", ".."] {
        assert!(read_native_ban_roster(&fixture.runtime, &fixture.root, "unturned", id).is_err());
    }
}

#[test]
fn squad_known_native_forms_deduplicate_active_accounts_and_remove_only_exact_subjects() {
    let path = Path::new("Bans.cfg");
    let content = format!(
        "// keep comment\r\n{STEAM}:0 // reason is not a name\r\nAdmin [SteamID {OTHER}] Banned:{STEAM}:200 // repeated\n{OTHER}:100 // expired\n"
    );
    let entries = crate::native_bans_squad::parse(&content, path, 150_000).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].account_id, STEAM);
    assert_eq!(entries[0].display_name, None);
    let expected = format!("// keep comment\r\n{OTHER}:100 // expired\n");
    for (original, expected) in [
        (content.as_bytes().to_vec(), expected.as_bytes().to_vec()),
        (utf16(&content), utf16(&expected)),
        (
            [&[0xef, 0xbb, 0xbf][..], content.as_bytes()].concat(),
            [&[0xef, 0xbb, 0xbf][..], expected.as_bytes()].concat(),
        ),
    ] {
        assert_eq!(
            remove_squad_native_ban_lines(&original, STEAM, path).unwrap(),
            expected
        );
    }
    assert!(remove_squad_native_ban_lines(content.as_bytes(), "76561198000000003", path).is_err());
    let malformed = format!("{STEAM}:0\nunsupported-native-record\n");
    assert!(remove_squad_native_ban_lines(malformed.as_bytes(), STEAM, path).is_err());
}

#[test]
fn squad_eos_administrative_prefix_never_becomes_the_banned_identity() {
    let path = Path::new("Bans.cfg");
    let banned = "00112233445566778899aabbccddeeff";
    let admin = "ffeeddccbbaa99887766554433221100";
    let content =
        format!("Admin name [EOSID {admin}] Banned:{banned}:0 // do not expose this reason\r\n");
    let entries = crate::native_bans_squad::parse(&content, path, 150_000).unwrap();
    assert_eq!(
        entries,
        [NativeBanEntry {
            account_id: banned.into(),
            display_name: None,
            identity_kind: RuntimePlayerIdentityKind::EosId,
        }]
    );
    assert!(remove_squad_native_ban_lines(content.as_bytes(), admin, path).is_err());
    assert!(
        remove_squad_native_ban_lines(content.as_bytes(), banned, path)
            .unwrap()
            .is_empty()
    );
    let rcon = format!("RCON Banned:{banned}:0 // reason\n");
    assert_eq!(
        crate::native_bans_squad::parse(&rcon, path, 150_000).unwrap()[0].account_id,
        banned
    );
    assert!(
        remove_squad_native_ban_lines(rcon.as_bytes(), banned, path)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "Explicit read-only probe of installed game persistence; requires external case manifest."]
fn native_installed_bans_readonly_probe() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Case {
        runtime_root: PathBuf,
        instance_root: PathBuf,
        module_id: String,
        instance_id: String,
        expected_accounts: Option<Vec<String>>,
    }
    let path = std::env::var_os("LANGAME_NATIVE_BAN_PROBE_CASES")
        .expect("Provide the external native ban probe case manifest.");
    let cases: Vec<Case> = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(!cases.is_empty() && cases.len() <= 32);
    for case in cases {
        let roster = read_native_ban_roster(
            &case.runtime_root,
            &case.instance_root,
            &case.module_id,
            &case.instance_id,
        )
        // A storage error can include a native player filename. Limit this
        // real-install probe's diagnostics to the module identity as well.
        .unwrap_or_else(|_| panic!("{} native ban read failed", case.module_id));
        println!(
            "native_ban_probe module={} count={}",
            case.module_id,
            roster.entries.len()
        );
        if let Some(mut expected) = case.expected_accounts {
            let mut actual = roster
                .entries
                .into_iter()
                .map(|entry| entry.account_id)
                .collect::<Vec<_>>();
            expected.sort();
            actual.sort();
            assert_eq!(
                actual, expected,
                "{} native account projection differs",
                case.module_id
            );
        }
    }
}
