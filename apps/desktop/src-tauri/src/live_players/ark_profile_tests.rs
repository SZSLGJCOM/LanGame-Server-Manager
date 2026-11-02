use super::*;

const EOS: &str = "00112233445566778899aabbccddeeff";
const STEAM: &str = "76561198000000001";

fn integer(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend(value.to_le_bytes());
}

fn string(value: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    if value.is_ascii() {
        integer(&mut bytes, value.len() as i32 + 1);
        bytes.extend(value.bytes());
        bytes.push(0);
    } else {
        let words: Vec<_> = value.encode_utf16().collect();
        integer(&mut bytes, -(words.len() as i32 + 1));
        for word in words {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend([0, 0]);
    }
    bytes
}

fn asa_property(name: &str, kind: &str, types: &[&str], value: &[u8]) -> Vec<u8> {
    let mut bytes = string(name);
    bytes.extend(string(kind));
    for subtype in types {
        integer(&mut bytes, 1);
        bytes.extend(string(subtype));
    }
    integer(&mut bytes, 0);
    integer(&mut bytes, value.len() as i32);
    // The real ASA v7 account ID uses HasBinaryOrNativeSerialize, with no
    // array-index/GUID header. The account name and MyData use tagged values.
    bytes.push(if types.first() == Some(&"UniqueNetIdRepl") {
        0x08
    } else {
        0
    });
    bytes.extend(value);
    bytes
}

fn ase_property(name: &str, kind: &str, subtype: Option<&str>, value: &[u8]) -> Vec<u8> {
    let mut bytes = string(name);
    bytes.extend(string(kind));
    integer(&mut bytes, value.len() as i32);
    integer(&mut bytes, 0);
    if let Some(subtype) = subtype {
        bytes.extend(string(subtype));
    }
    bytes.extend(value);
    bytes
}

fn properties(parts: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes: Vec<u8> = parts.into_iter().flatten().collect();
    bytes.extend(string("None"));
    bytes
}

fn asa_identity(account: &str) -> Vec<u8> {
    assert_eq!(account.len(), 32);
    let mut bytes = vec![0xf9];
    bytes.extend(string("RedpointEOS"));
    bytes.push(16);
    for pair in account.as_bytes().as_chunks::<2>().0 {
        let text = std::str::from_utf8(pair).unwrap();
        bytes.push(u8::from_str_radix(text, 16).unwrap());
    }
    bytes
}

fn asa_fields(account: &str, name_payload: Option<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut fields = vec![
        asa_property("PlayerDataID", "UInt64Property", &[], &42_u64.to_le_bytes()),
        asa_property(
            "UniqueID",
            "StructProperty",
            &["UniqueNetIdRepl"],
            &asa_identity(account),
        ),
    ];
    if let Some(payload) = name_payload {
        fields.push(asa_property("PlayerName", "StrProperty", &[], &payload));
    }
    let character = properties(vec![asa_property(
        "PlayerCharacterName",
        "StrProperty",
        &[],
        &string("Different survivor 角色"),
    )]);
    fields.push(asa_property(
        "MyPlayerCharacterConfig",
        "StructProperty",
        &["PrimalPlayerCharacterConfigStruct", "/Script/ShooterGame"],
        &character,
    ));
    fields
}

fn container(edition: Edition, objects: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = Vec::new();
    integer(&mut bytes, if edition == Edition::Asa { 7 } else { 1 });
    if edition == Edition::Asa {
        integer(&mut bytes, 522);
        integer(&mut bytes, 1013);
    }
    integer(&mut bytes, objects.len() as i32);
    let mut offsets = Vec::new();
    for (class, _) in &objects {
        bytes.extend([0; 16]);
        bytes.extend(string(class));
        integer(&mut bytes, 0); // isItem
        integer(&mut bytes, 1);
        bytes.extend(string("PrimalPlayerData_1"));
        integer(&mut bytes, 0); // fromDataFile
        integer(&mut bytes, 0); // dataFileIndex
        integer(&mut bytes, 0); // hasLocation
        offsets.push(bytes.len());
        integer(&mut bytes, 0); // Absolute properties offset, filled below.
        integer(&mut bytes, 0);
    }
    for ((_, block), offset) in objects.into_iter().zip(offsets) {
        let start = bytes.len() as i32;
        bytes[offset..offset + 4].copy_from_slice(&start.to_le_bytes());
        if edition == Edition::Asa {
            bytes.push(0);
        }
        bytes.extend(block);
    }
    bytes
}

fn asa_with_fields(fields: Vec<Vec<u8>>) -> Vec<u8> {
    let block = properties(vec![asa_property(
        "MyData",
        "StructProperty",
        &["PrimalPlayerDataStruct", "/Script/ShooterGame"],
        &properties(fields),
    )]);
    container(Edition::Asa, vec![(ASA_PLAYER_CLASS, block)])
}

/// Synthetic source-format fixture for file-layer and roster integration tests.
/// Contains deliberately different account/character names and an internal EOS ID.
pub(crate) fn asa_profile(account_id: &str, account_name: &str) -> Vec<u8> {
    asa_with_fields(asa_fields(account_id, Some(string(account_name))))
}

#[test]
fn asa_v7_reads_unicode_account_including_surrogate_pairs() {
    let bytes = asa_profile(EOS, "LanGame中文 🚀");
    assert_eq!(
        read_account_name(&bytes, EOS, Edition::Asa)
            .unwrap()
            .as_deref(),
        Some("LanGame中文 🚀")
    );
    assert_eq!(
        read_account_name(&bytes, &EOS.to_uppercase(), Edition::Asa)
            .unwrap()
            .as_deref(),
        Some("LanGame中文 🚀")
    );
}

#[test]
fn native_struct_flags_do_not_turn_mydata_into_a_tagged_property_list() {
    let payload = properties(asa_fields(EOS, Some(string("中文账户"))));
    let mut data = asa_property(
        "MyData",
        "StructProperty",
        &["PrimalPlayerDataStruct", "/Script/ShooterGame"],
        &payload,
    );
    let flag_offset = data.len() - payload.len() - 1;
    data[flag_offset] = 0x08;
    let bytes = container(
        Edition::Asa,
        vec![(ASA_PLAYER_CLASS, properties(vec![data]))],
    );
    assert!(read_account_name(&bytes, EOS, Edition::Asa).is_err());
}

#[test]
fn ase_v1_uses_internal_network_id_and_account_name() {
    let mut identity = 8_i32.to_le_bytes().to_vec();
    identity.extend(string(STEAM));
    let data = properties(vec![
        ase_property(
            "UniqueID",
            "StructProperty",
            Some("UniqueNetIdRepl"),
            &identity,
        ),
        ase_property("PlayerName", "StrProperty", None, &string("Steam中文")),
        ase_property(
            "MyPlayerCharacterConfig",
            "StructProperty",
            Some("PrimalPlayerCharacterConfigStruct"),
            &properties(vec![ase_property(
                "PlayerCharacterName",
                "StrProperty",
                None,
                &string("Different character"),
            )]),
        ),
    ]);
    let block = properties(vec![ase_property(
        "MyData",
        "StructProperty",
        Some("PrimalPlayerDataStruct"),
        &data,
    )]);
    let bytes = container(Edition::Ase, vec![("PrimalPlayerData", block)]);
    assert_eq!(
        read_account_name(&bytes, STEAM, Edition::Ase)
            .unwrap()
            .as_deref(),
        Some("Steam中文")
    );
    assert_eq!(
        read_account_name(&bytes, "76561198000000002", Edition::Ase),
        Err(ProfileError::IdentityMismatch)
    );
}

#[test]
fn rejects_foreign_or_missing_internal_identity() {
    let bytes = asa_profile(EOS, "中文");
    assert_eq!(
        read_account_name(&bytes, "10112233445566778899aabbccddeeff", Edition::Asa),
        Err(ProfileError::IdentityMismatch)
    );
    let bytes = asa_with_fields(vec![asa_property(
        "PlayerName",
        "StrProperty",
        &[],
        &string("中文"),
    )]);
    assert_eq!(
        read_account_name(&bytes, EOS, Edition::Asa),
        Err(ProfileError::Invalid("missing account identity"))
    );
}

#[test]
fn missing_account_name_never_uses_character_or_nested_decoy() {
    let mut fields = asa_fields(EOS, None);
    let decoy = properties(vec![asa_property(
        "PlayerName",
        "StrProperty",
        &[],
        &string("Wrong nested name"),
    )]);
    fields.push(asa_property(
        "UnrelatedStruct",
        "StructProperty",
        &["OtherStruct"],
        &decoy,
    ));
    assert_eq!(
        read_account_name(&asa_with_fields(fields), EOS, Edition::Asa).unwrap(),
        None
    );
}

#[test]
fn rejects_duplicate_account_names_and_identities() {
    for field in [
        asa_property("PlayerName", "StrProperty", &[], &string("Other name")),
        asa_property(
            "UniqueID",
            "StructProperty",
            &["UniqueNetIdRepl"],
            &asa_identity(EOS),
        ),
    ] {
        let mut fields = asa_fields(EOS, Some(string("中文")));
        fields.push(field);
        assert!(read_account_name(&asa_with_fields(fields), EOS, Edition::Asa).is_err());
    }
}

#[test]
fn rejects_duplicate_player_objects_and_mydata() {
    let mydata = asa_property(
        "MyData",
        "StructProperty",
        &["PrimalPlayerDataStruct", "/Script/ShooterGame"],
        &properties(asa_fields(EOS, Some(string("中文")))),
    );
    let block = properties(vec![mydata.clone()]);
    let bytes = container(
        Edition::Asa,
        vec![(ASA_PLAYER_CLASS, block.clone()), (ASA_PLAYER_CLASS, block)],
    );
    assert_eq!(
        read_account_name(&bytes, EOS, Edition::Asa),
        Err(ProfileError::Invalid("duplicate player data object"))
    );
    let bytes = container(
        Edition::Asa,
        vec![(ASA_PLAYER_CLASS, properties(vec![mydata.clone(), mydata]))],
    );
    assert_eq!(
        read_account_name(&bytes, EOS, Edition::Asa),
        Err(ProfileError::Invalid("duplicate MyData"))
    );
}

#[test]
fn rejects_truncation_at_every_byte_boundary() {
    let bytes = asa_profile(EOS, "中文 🚀");
    for end in 0..bytes.len() {
        assert!(
            read_account_name(&bytes[..end], EOS, Edition::Asa).is_err(),
            "accepted truncated prefix {end}"
        );
    }
}

#[test]
fn rejects_invalid_utf16_terminators_and_positive_non_ascii() {
    for payload in [
        vec![0xfe, 0xff, 0xff, 0xff, 0x00, 0xd8, 0, 0], // Unpaired high surrogate.
        vec![0xfe, 0xff, 0xff, 0xff, 0x41, 0, 1, 0],    // Missing wide NUL.
        vec![0xfe, 0xff, 0xff, 0xff, 0, 0, 0, 0],       // Embedded wide NUL.
        vec![2, 0, 0, 0, 0xff, 0], // Narrow strings are ANSI/ASCII, not guessed encodings.
        i32::MIN.to_le_bytes().to_vec(),
    ] {
        let bytes = asa_with_fields(asa_fields(EOS, Some(payload)));
        assert!(read_account_name(&bytes, EOS, Edition::Asa).is_err());
    }
}

#[test]
fn enforces_file_name_object_and_payload_bounds() {
    assert!(matches!(
        read_account_name(&vec![0; MAX_PROFILE_BYTES + 1], EOS, Edition::Asa),
        Err(ProfileError::LimitExceeded(_))
    ));
    assert!(matches!(
        read_account_name(&asa_profile(EOS, &"名".repeat(257)), EOS, Edition::Asa),
        Err(ProfileError::LimitExceeded(_))
    ));
    let mut bytes = asa_profile(EOS, "中文");
    bytes[12..16].copy_from_slice(&65_i32.to_le_bytes());
    assert!(matches!(
        read_account_name(&bytes, EOS, Edition::Asa),
        Err(ProfileError::LimitExceeded(_))
    ));
    let mut fields = asa_fields(EOS, None);
    let mut value = string("Name");
    value.push(0);
    fields.push(asa_property("PlayerName", "StrProperty", &[], &value));
    assert_eq!(
        read_account_name(&asa_with_fields(fields), EOS, Edition::Asa),
        Err(ProfileError::Invalid("property payload length"))
    );
}

#[test]
fn rejects_unsupported_versions_instead_of_guessing_dialects() {
    for version in [0_i32, 1, 5, 6, 8, 14] {
        let mut bytes = asa_profile(EOS, "中文");
        bytes[..4].copy_from_slice(&version.to_le_bytes());
        assert_eq!(
            read_account_name(&bytes, EOS, Edition::Asa),
            Err(ProfileError::UnsupportedVersion(version))
        );
    }
}

#[test]
fn empty_or_replacement_name_is_not_a_recovered_name() {
    for name in ["", "  ", "\u{fffd}"] {
        assert_eq!(
            read_account_name(&asa_profile(EOS, name), EOS, Edition::Asa).unwrap(),
            None
        );
    }
    assert!(read_account_name(&asa_profile(EOS, "name\ncommand"), EOS, Edition::Asa).is_err());
}

#[test]
fn exact_class_and_field_types_are_required() {
    let fields = asa_fields(EOS, Some(string("中文")));
    let block = properties(vec![asa_property(
        "MyData",
        "StructProperty",
        &["PrimalPlayerDataStruct", "/Script/ShooterGame"],
        &properties(fields),
    )]);
    let bytes = container(
        Edition::Asa,
        vec![("UnrelatedPrimalPlayerDataClass", block)],
    );
    assert!(read_account_name(&bytes, EOS, Edition::Asa).is_err());
    let mut fields = asa_fields(EOS, None);
    fields.push(asa_property(
        "PlayerName",
        "NameProperty",
        &[],
        &string("Wrong field type"),
    ));
    assert!(read_account_name(&asa_with_fields(fields), EOS, Edition::Asa).is_err());
    let mut fields = asa_fields(EOS, None);
    fields.push(asa_property(
        "PlayerName",
        "StrProperty",
        &["InvalidArgument"],
        &string("Wrong type arguments"),
    ));
    assert_eq!(
        read_account_name(&asa_with_fields(fields), EOS, Edition::Asa),
        Err(ProfileError::Invalid("property type arguments"))
    );
}

#[test]
fn bounded_type_trees_allow_skipping_unrelated_multitype_properties() {
    let mut ignored = string("IgnoredMap");
    ignored.extend(string("MapProperty"));
    integer(&mut ignored, 2);
    ignored.extend(string("StrProperty"));
    integer(&mut ignored, 0);
    ignored.extend(string("IntProperty"));
    integer(&mut ignored, 0);
    integer(&mut ignored, 0); // No payload is decoded for unrelated properties.
    ignored.push(0);
    let mut fields = vec![ignored];
    fields.extend(asa_fields(EOS, Some(string("中文"))));
    assert_eq!(
        read_account_name(&asa_with_fields(fields), EOS, Edition::Asa)
            .unwrap()
            .as_deref(),
        Some("中文")
    );
    let oversized = asa_property("Ignored", "ArrayProperty", &["a", "b", "c", "d", "e"], &[]);
    let mut fields = vec![oversized];
    fields.extend(asa_fields(EOS, Some(string("中文"))));
    assert!(matches!(
        read_account_name(&asa_with_fields(fields), EOS, Edition::Asa),
        Err(ProfileError::LimitExceeded(_))
    ));
}
