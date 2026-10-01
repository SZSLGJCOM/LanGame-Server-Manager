use super::managed_config_merge::ManagedConfigMergePlan;
use super::*;
use std::io::Read;

#[path = "unturned_dat.rs"]
mod dat;

const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MANAGED_SNAPSHOT: &str = "unturned-managed-config.txt";

fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: "unturned".into(),
        field: "Config.txt".into(),
        message: message.into(),
    }
}

fn read_bounded(path: &Path, optional: bool) -> Result<Option<Vec<u8>>, StorageError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadConfig {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(invalid("Native configuration exceeds 4 MiB."));
    }
    Ok(Some(bytes))
}

fn utf8(bytes: Option<&[u8]>) -> Result<&str, StorageError> {
    std::str::from_utf8(bytes.unwrap_or_default()).map_err(|error| {
        invalid(format!(
            "Refusing to replace non-UTF-8 native configuration: {error}"
        ))
    })
}

pub(super) fn merge_gameplay_config(
    source: &Path,
    destination: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let rendered = read_bounded(source, false)?;
    let original = read_bounded(destination, true)?;
    let snapshot_path = source.with_file_name(MANAGED_SNAPSHOT);
    let snapshot = read_bounded(&snapshot_path, true)?;
    let merged = dat::merge_managed(
        utf8(original.as_deref())?,
        utf8(rendered.as_deref())?,
        utf8(snapshot.as_deref())?,
        &super::super::templates_render_unturned::managed_native_paths()?,
    )
    .map_err(|message| {
        invalid(format!(
            "Refusing to replace invalid native configuration: {message}"
        ))
    })?;
    // Both the native bytes and ownership snapshot belong to the existing rollback transaction.
    // Keeping the read snapshots in these plans also detects concurrent external edits.
    files.apply(vec![
        ManagedConfigMergePlan {
            destination_path: destination.to_path_buf(),
            replacement: merged.into_bytes(),
            original,
        },
        ManagedConfigMergePlan {
            destination_path: snapshot_path,
            replacement: rendered.unwrap_or_default(),
            original: snapshot,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_reading_and_clearing_keeps_unknown_native_data_and_external_edits() {
        let root =
            std::env::temp_dir().join(format!("lsgm-unturned-merge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("config/Config.txt");
        let destination = root.join("server/Config.txt");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let existing = "// user note\nVersion 1\nItems\n{\nUnknown_Value 42\nSpawn_Chance 0.75 // custom note\n}\n";
        fs::write(&destination, existing).unwrap();
        fs::write(
            &source,
            "Version 1\nItems\n{\nSpawn_Chance 0.25\nHas_Durability false\n}\n",
        )
        .unwrap();
        let mut files = ManagedConfigMutation::new("unturned");
        merge_gameplay_config(&source, &destination, &mut files).unwrap();
        files.commit();
        let first = fs::read_to_string(&destination).unwrap();
        assert!(first.contains("Spawn_Chance 0.25 // custom note"));
        assert!(first.contains("Unknown_Value 42"));
        // An external owner changes this value after the first persisted write.
        fs::write(
            &destination,
            first.replace("Has_Durability false", "Has_Durability true"),
        )
        .unwrap();
        fs::write(&source, "Version 1\n").unwrap();
        let mut files = ManagedConfigMutation::new("unturned");
        merge_gameplay_config(&source, &destination, &mut files).unwrap();
        files.commit();
        let cleared = fs::read_to_string(&destination).unwrap();
        assert!(!cleared.contains("Spawn_Chance"));
        assert!(cleared.contains("Has_Durability true"));
        assert!(cleared.contains("Unknown_Value 42"));
        assert!(cleared.contains("// custom note"));
        assert_eq!(
            fs::read_to_string(source.with_file_name(MANAGED_SNAPSHOT)).unwrap(),
            "Version 1\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_and_oversized_native_files_remain_unchanged() {
        let root =
            std::env::temp_dir().join(format!("lsgm-unturned-invalid-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("Config.txt");
        let destination = root.join("native.txt");
        fs::write(&source, "Version 1\n").unwrap();
        for bytes in [
            b"Items\n{\nKey 1".to_vec(),
            vec![b'x'; MAX_CONFIG_BYTES as usize + 1],
            vec![0xff],
        ] {
            fs::write(&destination, &bytes).unwrap();
            let mut files = ManagedConfigMutation::new("unturned");
            assert!(merge_gameplay_config(&source, &destination, &mut files).is_err());
            assert_eq!(fs::read(&destination).unwrap(), bytes);
            assert!(!source.with_file_name(MANAGED_SNAPSHOT).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
