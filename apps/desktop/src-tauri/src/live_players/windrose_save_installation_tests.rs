use super::*;
use std::fs;
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("langame-windrose-install-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn interrupted_pending_version_remains_owned_after_a_new_intent_and_second_crash() {
    let root = Directory::new();
    verify_owned_mods(&root.0).unwrap();
    publish_installation(&root.0, b"script-a", b"dll-a", publish).unwrap();
    verify_owned_mods(&root.0).unwrap();
    let owner_path = root.0.join(OWNER);
    let bytes = read(&owner_path, 8192).unwrap();
    let mut owner = decode(&bytes).unwrap();
    owner.pending_files = Some(
        payload(b"script-b", b"dll-b")
            .iter()
            .map(|(name, bytes)| (name.clone(), hash(bytes)))
            .collect(),
    );
    publish(
        &owner_path,
        Some(&bytes),
        &serde_json::to_vec(&owner).unwrap(),
    )
    .unwrap();
    let script = root.0.join(MOD).join("Scripts/main.lua");
    publish(&script, Some(b"script-a"), b"script-b").unwrap();
    verify_owned_mods(&root.0).unwrap();
    // New release C recognizes actual script B, publishes its intent, then
    // crashes before touching any payload. A later D must still recognize B.
    let mut publications = 0;
    assert!(
        publish_installation(
            &root.0,
            b"script-c",
            b"dll-c",
            |path, original, replacement| {
                publications += 1;
                if publications == 2 {
                    return Err("injected process loss after intent".into());
                }
                publish(path, original, replacement)
            }
        )
        .is_err()
    );
    let interrupted = decode(&read(&owner_path, 8192).unwrap()).unwrap();
    assert_eq!(
        interrupted.files.get("Scripts/main.lua"),
        Some(&hash(b"script-b"))
    );
    assert_eq!(fs::read(&script).unwrap(), b"script-b");
    publish_installation(&root.0, b"script-d", b"dll-d", publish).unwrap();
    assert!(complete(&root.0, b"script-d", b"dll-d").unwrap());
}
#[test]
fn unowned_and_externally_modified_native_files_are_preserved() {
    let root = Directory::new();
    fs::create_dir_all(root.0.join(MOD)).unwrap();
    assert!(verify_owned_mods(&root.0).is_err());
    assert!(publish_installation(&root.0, b"script", b"dll", publish).is_err());
    fs::remove_dir_all(root.0.join("R5")).unwrap();
    publish_installation(&root.0, b"script", b"dll", publish).unwrap();
    let dll = root.0.join(MOD).join(NATIVE);
    fs::write(&dll, b"operator-edit").unwrap();
    assert!(verify_owned_mods(&root.0).is_err());
    assert!(publish_installation(&root.0, b"next", b"next-dll", publish).is_err());
    assert_eq!(fs::read(dll).unwrap(), b"operator-edit");
}
#[test]
fn ownership_cannot_authorize_paths_outside_the_three_file_contract() {
    let mut files = payload(b"script", b"dll")
        .iter()
        .map(|(name, bytes)| (name.clone(), hash(bytes)))
        .collect::<BTreeMap<_, _>>();
    assert!(hashes(&files));
    files.insert("../external.dll".into(), "a".repeat(64));
    assert!(!hashes(&files));
}
