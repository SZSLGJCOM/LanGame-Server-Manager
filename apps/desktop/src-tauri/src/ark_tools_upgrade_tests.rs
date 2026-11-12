use super::*;

const ASA_2_03: &str = "7d12ff5fd238c4094da863d99651f3b54d7ffde1752eaa7cc263222cb7cd09d5";
const ASA_2_1: &str = "d46af90afab50d36679f8ea4c8b7c1b2e6057ec4ad44963f00244ed8f3969a62";
const FRAMEWORK: &str = "ArkApi/AsaApi.dll";

#[test]
fn asa_upgrade_recognizes_only_the_previously_shipped_framework_pins() {
    let wanted = expected(&ASA, inspection_embedded(false)).unwrap();
    for previous in [ASA_2_03, ASA_2_1] {
        let mut owned = Ownership {
            schema: 1,
            module_id: ASA.module_id.into(),
            files: wanted.clone(),
        };
        owned.files.insert(FRAMEWORK.into(), previous.into());
        let decoded =
            files::decode_ownership(&serde_json::to_vec(&owned).unwrap(), &ASA, &wanted).unwrap();
        assert_eq!(decoded.files, owned.files);
        assert_ne!(decoded.files, wanted);

        for path in [FRAMEWORK, "msdia140.dll", "config.json"] {
            let mut invalid = Ownership {
                schema: 1,
                module_id: ASA.module_id.into(),
                files: wanted.clone(),
            };
            invalid.files.insert(
                path.into(),
                if path == FRAMEWORK {
                    digest(b"unrecognized framework")
                } else {
                    previous.into()
                },
            );
            let error =
                files::decode_ownership(&serde_json::to_vec(&invalid).unwrap(), &ASA, &wanted)
                    .err()
                    .expect("unknown dependency or configuration must be rejected");
            assert!(error.contains("unknown ownership") && error.contains(path));
        }
        owned.module_id = ASE.module_id.into();
        assert!(
            files::decode_ownership(&serde_json::to_vec(&owned).unwrap(), &ASE, &wanted).is_err()
        );
    }
}

fn record_previous_framework(fixture: &Fixture, previous: &str) -> Vec<u8> {
    let owner_path = fixture.bin().join(OWNER);
    let mut owned: Ownership = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    owned.files.insert(FRAMEWORK.into(), previous.into());
    let bytes = serde_json::to_vec(&owned).unwrap();
    fs::write(owner_path, &bytes).unwrap();
    bytes
}

#[test]
fn asa_upgrade_preserves_files_that_do_not_match_the_previous_framework_receipt() {
    for previous in [ASA_2_03, ASA_2_1] {
        let (fixture, profile) = inspection_fixture(&ASA, true);
        let original_owner = record_previous_framework(&fixture, previous);
        let original_framework = fs::read(fixture.bin().join(FRAMEWORK)).unwrap();
        let mut payload = embedded_files(&profile, inspection_embedded(false)).unwrap();
        payload.insert(FRAMEWORK.into(), b"framework".to_vec());
        let wanted = hashes(&payload);

        let error = publish(&fixture.0, &profile, payload, &wanted).unwrap_err();
        assert!(error.contains("unknown or modified") && error.contains(FRAMEWORK));
        assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), original_owner);
        assert_eq!(
            fs::read(fixture.bin().join(FRAMEWORK)).unwrap(),
            original_framework
        );
        assert!(!transaction::pending(&fixture.bin()).unwrap());
        fixture.finish();
    }
}

#[test]
fn asa_upgrade_repairs_a_missing_previous_dependency_with_rollback_and_retry() {
    for previous in [ASA_2_03, ASA_2_1] {
        let (fixture, profile) = inspection_fixture(&ASA, true);
        let original_owner = record_previous_framework(&fixture, previous);
        fs::remove_file(fixture.bin().join(FRAMEWORK)).unwrap();
        let original_plugin = fs::read(fixture.bin().join(PLUGIN)).unwrap();
        let mut payload = embedded_files(&profile, inspection_embedded(false)).unwrap();
        payload.insert(FRAMEWORK.into(), b"framework".to_vec());
        let wanted = hashes(&payload);

        let mut attempt = payload.clone();
        let error = publish_with(&fixture.0, &profile, &mut attempt, &wanted, |index| {
            if index == 2 {
                Err("injected upgrade failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error, "injected upgrade failure");
        assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), original_owner);
        assert_eq!(
            fs::read(fixture.bin().join(PLUGIN)).unwrap(),
            original_plugin
        );
        assert!(!fixture.bin().join(FRAMEWORK).exists());
        assert!(!transaction::pending(&fixture.bin()).unwrap());

        publish(&fixture.0, &profile, payload, &wanted).unwrap();
        let status = inspect_profile(&fixture.0, &profile, inspection_embedded(false)).unwrap();
        assert!(status.ready && status.matches_bundle && status.can_uninstall);
        assert_eq!(
            files::ownership(&fixture.bin(), &profile, &wanted)
                .unwrap()
                .unwrap()
                .files,
            wanted
        );
        fixture.finish();
    }
}
