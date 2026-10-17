use super::*;

const ACCOUNT: u64 = 76_561_198_000_000_001;
const OTHER: u64 = 76_561_198_000_000_002;

fn record(version: u8, account: u64, banned: u32, duration: u32) -> Vec<u8> {
    let mut bytes = account.to_le_bytes().to_vec();
    if version > 2 {
        bytes.extend(0x7f000001_u32.to_le_bytes());
    }
    bytes.extend(123_u64.to_le_bytes());
    let reason = "synthetic-private-reason";
    bytes.push(reason.len() as u8);
    bytes.extend(reason.as_bytes());
    bytes.extend(duration.to_le_bytes());
    bytes.extend(banned.to_le_bytes());
    if version >= 4 {
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(3_u16.to_le_bytes());
        bytes.extend([0xaa, 0xbb, 0xcc]);
    }
    bytes
}

fn document(version: u8, rows: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![version];
    bytes.extend((rows.len() as u16).to_le_bytes());
    for row in rows {
        bytes.extend(row);
    }
    bytes
}

fn read(bytes: &[u8], seconds: u32) -> Result<Vec<NativeBanEntry>, StorageError> {
    parse(bytes, Path::new("Blacklist.dat"), u64::from(seconds) * 1000)
}

#[test]
fn verified_versions_read_accounts_and_expiry_without_disclosing_private_metadata() {
    for version in 2..=4 {
        let bytes = document(
            version,
            &[
                record(version, ACCOUNT, 100, 100),
                record(version, OTHER, 1, 2),
            ],
        );
        let entries = read(&bytes, 150).unwrap();
        assert_eq!(
            entries,
            [NativeBanEntry {
                account_id: ACCOUNT.to_string(),
                display_name: None,
                identity_kind: RuntimePlayerIdentityKind::SteamId,
            }]
        );
        let projection = serde_json::to_string(&entries).unwrap();
        assert!(!projection.contains("synthetic-private-reason"));
        assert!(!projection.contains("127.0.0.1"));
        assert!(!projection.contains("hwid"));
    }
}

#[test]
fn native_expiry_is_strict_and_duration_zero_is_not_a_permanent_ban() {
    let bytes = document(4, &[record(4, ACCOUNT, 100, 0)]);
    assert_eq!(read(&bytes, 100).unwrap().len(), 1);
    assert!(read(&bytes, 101).unwrap().is_empty());
    let wrapped = document(4, &[record(4, ACCOUNT, u32::MAX - 1, 3)]);
    assert_eq!(read(&wrapped, 1).unwrap().len(), 1);
    assert!(read(&wrapped, 2).unwrap().is_empty());
}

#[test]
fn duplicate_active_accounts_are_one_native_ban_identity() {
    let bytes = document(
        4,
        &[record(4, ACCOUNT, 100, 100), record(4, ACCOUNT, 100, 100)],
    );
    assert_eq!(read(&bytes, 150).unwrap().len(), 1);
}

#[test]
fn unknown_versions_truncation_trailing_data_and_capacity_fail_closed() {
    for version in [0, 1, 5, 255] {
        assert!(read(&[version, 0, 0], 150).is_err());
    }
    let bytes = document(4, &[record(4, ACCOUNT, 100, 100)]);
    for boundary in 0..bytes.len() {
        assert!(
            read(&bytes[..boundary], 150).is_err(),
            "boundary {boundary}"
        );
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(read(&trailing, 150).is_err());
    let mut too_many = vec![4];
    too_many.extend((MAX_ROWS as u16 + 1).to_le_bytes());
    assert!(read(&too_many, 150).is_err());
    assert!(read(&document(4, &[record(4, 0, 100, 100)]), 150).is_err());
}

#[test]
fn native_truncated_utf8_reason_is_accepted_but_excessive_hardware_count_is_rejected() {
    let mut bad_reason = record(4, ACCOUNT, 100, 100);
    bad_reason[8 + 4 + 8 + 1] = 0xff;
    assert_eq!(read(&document(4, &[bad_reason]), 150).unwrap().len(), 1);
    let mut too_many_hardware = record(4, ACCOUNT, 100, 100);
    let count_position = too_many_hardware.len() - 4 - 2 - 3;
    too_many_hardware[count_position..count_position + 4]
        .copy_from_slice(&(MAX_ROWS as i32 + 1).to_le_bytes());
    assert!(read(&document(4, &[too_many_hardware]), 150).is_err());
}
