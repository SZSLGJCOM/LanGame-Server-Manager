use super::*;
use crate::instance_archive_external::{ExternalFile, ExternalProgramPlan};

#[test]
fn archived_external_mod_owner_rebinds_without_reindexing_payload_slots() {
    let root =
        std::env::temp_dir().join(format!("lgsm-archived-mod-state-{}", uuid::Uuid::new_v4()));
    let source = root.join("program");
    let target = root.join("moved-program");
    let physical = root.join("archive");
    let payload = physical.join(".langame-archived-program");
    for path in [source.join("Mods"), target.join("Mods"), payload.clone()] {
        fs::create_dir_all(path).unwrap();
    }
    let owner = crate::mod_management_state::target_identity(
        &fs::canonicalize(source.join("Mods")).unwrap(),
    );
    let leaf = crate::mod_management_state::initial_directory_name(&owner);
    let owner_key = format!("{leaf}/owner.json");
    let entries = BTreeMap::from([
        (
            format!("{leaf}/disabled/Example.dll"),
            b"archived plugin".to_vec(),
        ),
        (owner_key.clone(), serde_json::to_vec(&owner).unwrap()),
        (
            format!("{leaf}/pending.json"),
            b"{\"action\":\"disable\"}".to_vec(),
        ),
    ]);
    let mut files = BTreeMap::new();
    for (index, (name, bytes)) in entries.iter().enumerate() {
        fs::write(payload.join(format!("{index:06}")), bytes).unwrap();
        files.insert(
            name.clone(),
            ExternalFile {
                sha256: digest(bytes),
                bytes: bytes.len() as u64,
                stored: true,
                owned: false,
            },
        );
    }
    let directories = std::collections::BTreeSet::from([leaf.clone(), format!("{leaf}/disabled")]);
    let external = ExternalProgramPlan {
        root: source.clone(),
        identity: identity(&source, true).unwrap(),
        parent_identity: identity(&root, true).unwrap(),
        fingerprint: digest(&serde_json::to_vec(&(&files, &directories)).unwrap()),
        files,
        directories,
        payload_identity: Some(identity(&payload, true).unwrap()),
        exclusive: true,
        complete_inventory: true,
        restore_token: uuid::Uuid::new_v4().to_string(),
        restore_identity: None,
        binding_sha256: digest(b"binding"),
        binding_bytes: 7,
    };
    let mut snapshot: Snapshot = serde_json::from_value(serde_json::json!({
        "version":3,"tables":{},"config_sha256":digest(b"config"),"external_program":external,
    }))
    .unwrap();
    let movement = RootMove {
        source,
        target: target.clone(),
    };
    let mut plan = MetadataPlan::default();
    external_program(
        &mut snapshot,
        &physical,
        "fixture",
        &[movement],
        true,
        &mut plan,
    )
    .unwrap();
    assert_eq!(plan.files.len(), 1);
    let owner_slot = entries.keys().position(|key| key == &owner_key).unwrap();
    assert!(
        same_path(
            &plan.files[0].path,
            &payload.join(format!("{owner_slot:06}"))
        )
        .unwrap()
    );
    let changed: String = serde_json::from_slice(&plan.files[0].after).unwrap();
    assert_eq!(
        changed,
        crate::mod_management_state::target_identity(
            &fs::canonicalize(target.join("Mods")).unwrap(),
        )
    );
    let rebound = snapshot.external_program.unwrap();
    rebound.validate().unwrap();
    assert_eq!(
        rebound.files.keys().collect::<Vec<_>>(),
        entries.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        rebound.files[&owner_key].sha256,
        digest(&plan.files[0].after)
    );
    for (index, (key, bytes)) in entries.iter().enumerate() {
        assert_eq!(
            fs::read(payload.join(format!("{index:06}"))).unwrap(),
            *bytes
        );
        if key != &owner_key {
            assert_eq!(rebound.files[key].sha256, digest(bytes));
        }
    }
    fs::remove_dir_all(root).unwrap();
}
