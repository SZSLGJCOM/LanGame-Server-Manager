use super::*;

const BUILDING: &str = "Difficulty.Progression.BuildingMaterialCostScale";
const FUTURE: &str = "Difficulty.Future.UnrecognizedRule";

fn str_bytes(value: &str) -> Vec<u8> {
    if value.is_ascii() {
        let mut bytes = ((value.len() + 1) as i32).to_le_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
        bytes
    } else {
        let units: Vec<u16> = value.encode_utf16().collect();
        let mut bytes = (-(units.len() as i32 + 1)).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }
}

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn offsets(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = (fields.len() as u32).to_le_bytes().to_vec();
    let mut offset = 0u32;
    for field in fields {
        bytes.extend_from_slice(&offset.to_le_bytes());
        offset += field.len() as u32;
    }
    bytes.extend_from_slice(&offset.to_le_bytes());
    for field in fields {
        bytes.extend_from_slice(field);
    }
    bytes
}

fn names(values: &[String]) -> Vec<u8> {
    let mut bytes = (values.len() as u32).to_le_bytes().to_vec();
    for name in values {
        bytes.extend_from_slice(&str_bytes(name));
    }
    bytes
}

// Hand-written current native serialization, independent of the production map
// encoder. The ignored real-server probe below independently checks this fixture.
fn opaque_map(values: &BTreeMap<String, f32>) -> Vec<u8> {
    let mut bytes = 0u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for (tag, value) in values {
        bytes.extend_from_slice(&str_bytes("TagName"));
        bytes.extend_from_slice(&str_bytes("NameProperty"));
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let payload = str_bytes(tag);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&str_bytes("None"));
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn values(entries: &[(&str, f32)]) -> BTreeMap<String, f32> {
    entries
        .iter()
        .map(|(tag, value)| ((*tag).to_owned(), *value))
        .collect()
}

fn fixture(
    mode: u16,
    cinf_values: &BTreeMap<String, f32>,
    map_values: &BTreeMap<String, f32>,
    reversed: bool,
) -> Vec<u8> {
    let guid: Vec<u8> = [11u32, 22, 33, 44]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut cinf = vec![
        ("VERSION".to_owned(), 9u32.to_le_bytes().to_vec()),
        ("GUID_A".to_owned(), 11u32.to_le_bytes().to_vec()),
        ("GUID_B".to_owned(), 22u32.to_le_bytes().to_vec()),
        ("GUID_C".to_owned(), 33u32.to_le_bytes().to_vec()),
        ("GUID_D".to_owned(), 44u32.to_le_bytes().to_vec()),
        ("WorldName".to_owned(), str_bytes("脱敏世界")),
        (
            "SurvivalDifficulty".to_owned(),
            u32::from(mode).to_le_bytes().to_vec(),
        ),
        ("HardcoreState".to_owned(), 1u32.to_le_bytes().to_vec()),
        ("UnknownInfo".to_owned(), vec![231, 2, 3, 4, 5]),
    ];
    for (tag, value) in cinf_values {
        cinf.push((tag.clone(), value.to_le_bytes().to_vec()));
    }
    let mut custom = names(&cinf.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>());
    custom.extend_from_slice(&offsets(
        &cinf.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
    ));
    let versions: Vec<u8> = [522u32, 1017]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut info = 8u16.to_le_bytes().to_vec();
    info.extend_from_slice(&versions);
    info.extend_from_slice(&[0, 0, 0, 0, 255]);
    info.extend_from_slice(&0u32.to_le_bytes()); // Empty FText has no culture-invariant source.
    info.extend_from_slice(&str_bytes("2026-01-01T00:00:00.000Z"));
    info.extend_from_slice(&chunk(b"SHOT", &[1, 2, 3, 4]));
    info.extend_from_slice(&chunk(b"CINF", &custom));

    let mut props = vec![
        ("WorldName", 30u16, str_bytes("脱敏世界")),
        ("CustomDifficultySettings", 64, opaque_map(map_values)),
        ("UnknownObjectData", 777, vec![77, 78, 79]),
        ("HardcoreState", 1, 1u16.to_le_bytes().to_vec()),
        ("WorldSaveGuid", 23, guid),
        ("SurvivalDifficulty", 1, mode.to_le_bytes().to_vec()),
    ];
    if reversed {
        props.reverse();
    }
    let mut property_names = vec!["WorldSaveSettings".to_owned()];
    property_names.extend(props.iter().map(|(name, _, _)| (*name).to_owned()));
    let class_name = "/Script/Dominion.PersistenceSubsystem";
    let mut class = str_bytes(class_name);
    class.extend_from_slice(&(props.len() as u16).to_le_bytes());
    for (index, (_, kind, _)) in props.iter().enumerate() {
        class.extend_from_slice(&(index as u32 + 1).to_le_bytes());
        class.extend_from_slice(&0u32.to_le_bytes());
        class.extend_from_slice(&kind.to_le_bytes());
    }
    let mut wrapped = vec![0];
    wrapped.extend_from_slice(&chunk(b"CDEF", &class));
    let mut metadata = chunk(b"VERS", &5u32.to_le_bytes());
    metadata.extend_from_slice(&chunk(b"CNIX", &names(&[class_name.to_owned()])));
    metadata.extend_from_slice(&chunk(b"CLST", &chunk(b"CDVE", &wrapped)));
    metadata.extend_from_slice(&chunk(b"PNIX", &names(&property_names)));
    let mut object = 0u32.to_le_bytes().to_vec();
    object.extend_from_slice(&str_bytes("FixturePersistence"));
    object.extend_from_slice(&0u32.to_le_bytes());
    object.extend_from_slice(&versions);
    object.extend_from_slice(&chunk(
        b"PROP",
        &offsets(
            &props
                .into_iter()
                .map(|(_, _, value)| value)
                .collect::<Vec<_>>(),
        ),
    ));
    object.extend_from_slice(&chunk(b"CUST", &[123, 34, 56, 78]));
    let mut global = str_bytes("L_World");
    global.extend_from_slice(&chunk(b"META", &metadata));
    global.extend_from_slice(&chunk(b"GOBS", &chunk(b"NOBJ", &object)));
    global.extend_from_slice(&chunk(b"GLAI", &[22, 44, 55, 66]));
    let mut body = chunk(b"INFO", &info);
    body.extend_from_slice(&chunk(b"GLOB", &global));
    body.extend_from_slice(&chunk(b"LVLS", &[0, 255, 100, 3, 4, 5]));
    body.extend_from_slice(&chunk(b"FUTR", &[20, 21, 22]));
    chunk(b"SAVE", &body)
}

#[test]
fn standard_world_adds_custom_rule_and_preserves_world_content() {
    let original = fixture(0, &BTreeMap::new(), &BTreeMap::new(), true);
    let expected = values(&[(BUILDING, 1.5)]);
    let patched = patch_world_settings(&original, 3, &expected).unwrap();
    assert_eq!(
        decode_world_settings(&patched).unwrap(),
        NativeWorldSettings {
            world_name: "脱敏世界".into(),
            world_mode: 3,
            hardcore_state: 1,
            overrides: expected,
        }
    );
    let before = parse(&original).unwrap();
    let after = parse(&patched).unwrap();
    for tag in [b"LVLS", b"FUTR"] {
        assert_eq!(
            before.children[unique(&before.children, tag).unwrap()].raw,
            after.children[unique(&after.children, tag).unwrap()].raw
        );
    }
    assert_eq!(
        before.info.custom.field("UnknownInfo").unwrap(),
        after.info.custom.field("UnknownInfo").unwrap()
    );
    assert_eq!(
        before.global.children[unique(&before.global.children, b"META").unwrap()].raw,
        after.global.children[unique(&after.global.children, b"META").unwrap()].raw
    );
    assert_eq!(
        before.global.world.children[unique(&before.global.world.children, b"CUST").unwrap()].raw,
        after.global.world.children[unique(&after.global.world.children, b"CUST").unwrap()].raw
    );
}

#[test]
fn existing_rule_edit_remove_and_unknown_entry_retention() {
    let initial = values(&[(BUILDING, 1.5), (FUTURE, 2.25)]);
    let original = fixture(3, &initial, &initial, false);
    let expected = values(&[(BUILDING, 2.0), (FUTURE, 2.25)]);
    let edited = patch_world_settings(&original, 3, &expected).unwrap();
    assert_eq!(decode_world_settings(&edited).unwrap().overrides, expected);
    let before = parse(&original).unwrap();
    let after = parse(&edited).unwrap();
    assert_eq!(
        before.entries.iter().find(|v| v.tag == FUTURE).unwrap().raw,
        after.entries.iter().find(|v| v.tag == FUTURE).unwrap().raw
    );
    let only_future = values(&[(FUTURE, 2.25)]);
    let removed = patch_world_settings(&edited, 3, &only_future).unwrap();
    assert_eq!(
        decode_world_settings(&removed).unwrap().overrides,
        only_future
    );
    assert!(
        !parse(&removed)
            .unwrap()
            .info
            .custom
            .names
            .iter()
            .any(|v| v == BUILDING)
    );
}

#[test]
fn unchanged_settings_return_byte_identical_save() {
    let initial = values(&[(BUILDING, 1.5), (FUTURE, 2.25)]);
    let original = fixture(3, &initial, &initial, true);
    assert_eq!(
        patch_world_settings(&original, 3, &initial).unwrap(),
        original
    );
}

#[test]
fn unknown_existing_tag_bytes_survive_without_authorizing_new_tag_names() {
    let unknown = "FutureGameplay.UnknownKey";
    let initial = values(&[(unknown, 2.25)]);
    let original = fixture(3, &initial, &initial, false);
    let mut expected = initial.clone();
    expected.insert(BUILDING.to_owned(), 1.5);
    let patched = patch_world_settings(&original, 3, &expected).unwrap();
    let before = parse(&original).unwrap();
    let after = parse(&patched).unwrap();
    assert_eq!(before.entries[0].raw, after.entries[0].raw);
    assert_eq!(after.settings.overrides, expected);
    let empty = fixture(0, &BTreeMap::new(), &BTreeMap::new(), false);
    assert!(patch_world_settings(&empty, 3, &initial).is_err());
}

#[test]
fn rejects_mode_identity_and_hardcore_disagreement() {
    let original = fixture(0, &BTreeMap::new(), &BTreeMap::new(), false);
    for name in ["SurvivalDifficulty", "HardcoreState", "GUID_A"] {
        let offset = {
            let parsed = parse(&original).unwrap();
            let field = parsed.info.custom.field(name).unwrap();
            field.as_ptr() as usize - original.as_ptr() as usize
        };
        let mut invalid = original.clone();
        invalid[offset..offset + 4].copy_from_slice(&99u32.to_le_bytes());
        assert!(
            decode_world_settings(&invalid).is_err(),
            "accepted divergent {name}"
        );
    }
}

#[test]
fn rejects_truncation_malformed_counts_and_wrong_metadata() {
    let original = fixture(0, &BTreeMap::new(), &BTreeMap::new(), false);
    for size in 0..original.len() {
        assert!(
            decode_world_settings(&original[..size]).is_err(),
            "accepted truncation {size}"
        );
    }
    let mut invalid = original.clone();
    invalid[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_world_settings(&invalid).is_err());
    let mut invalid = original.clone();
    let start = invalid.windows(4).position(|v| v == b"PROP").unwrap() + 8;
    invalid[start..start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_world_settings(&invalid).is_err());
    let mut invalid = original;
    let position = invalid
        .windows("CustomDifficultySettings".len())
        .position(|v| v == b"CustomDifficultySettings")
        .unwrap();
    invalid[position] = b'X';
    assert!(decode_world_settings(&invalid).is_err());
}

#[test]
fn rejects_inconsistent_duplicates_and_unsupported_map_grammar() {
    let cinf = values(&[(BUILDING, 1.5)]);
    let map = values(&[(BUILDING, 2.0)]);
    assert!(decode_world_settings(&fixture(3, &cinf, &map, false)).is_err());
    assert!(decode_world_settings(&fixture(3, &cinf, &BTreeMap::new(), false)).is_err());
    let original = fixture(3, &cinf, &cinf, false);
    let parsed = parse(&original).unwrap();
    let raw = parsed.global.world.properties.fields[parsed.global.world.map_index];
    let mut duplicate = raw.to_vec();
    duplicate[4..8].copy_from_slice(&2u32.to_le_bytes());
    duplicate.extend_from_slice(&raw[8..]);
    assert!(map::decode(&duplicate).is_err());
    let mut invalid = raw.to_vec();
    invalid[..4].copy_from_slice(&1u32.to_le_bytes());
    assert!(map::decode(&invalid).is_err());
    let mut invalid = raw.to_vec();
    invalid[8] = 0xff;
    assert!(map::decode(&invalid).is_err());
}

#[test]
fn refuses_invalid_new_tags_modes_and_nonfinite_values() {
    let original = fixture(0, &BTreeMap::new(), &BTreeMap::new(), false);
    assert!(patch_world_settings(&original, 4, &BTreeMap::new()).is_err());
    for tag in [
        "WorldName",
        "Difficulty.",
        "Difficulty..Invalid",
        "Difficulty.Valid\nName",
    ] {
        assert!(patch_world_settings(&original, 3, &values(&[(tag, 1.0)])).is_err());
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(patch_world_settings(&original, 3, &values(&[(BUILDING, value)])).is_err());
    }
}

#[test]
#[ignore = "requires an explicitly supplied isolated, stopped native Dragonwilds probe save"]
fn native_probe_save_matches_actual_custom_setting_and_round_trips() {
    let path = std::env::var_os("LGSM_DRAGONWILDS_SAVE_FIXTURE")
        .expect("set the isolated native probe save path");
    let bytes = std::fs::read(path).expect("read isolated native probe save");
    let settings = decode_world_settings(&bytes).unwrap();
    assert_eq!(settings.world_mode, 3);
    assert_eq!(settings.overrides.get(BUILDING), Some(&1.5));
    assert_eq!(
        patch_world_settings(&bytes, settings.world_mode, &settings.overrides).unwrap(),
        bytes
    );
}
