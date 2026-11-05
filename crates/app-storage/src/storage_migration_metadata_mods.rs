use super::*;

pub(super) fn prepare_owner(
    directory: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let path = directory.join("owner.json");
    let before = read_bytes(&path, 32_768)?;
    let mut value = serde_json::from_slice(&before)?;
    rewrite_owner(&mut value, moves, &path)?;
    add_json_patch(&path, &before, &value, moves, copied, plan)
}

pub(super) fn is_owner(relative: &str) -> bool {
    let path = Path::new(relative);
    path.file_name().is_some_and(|name| name == "owner.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(crate::mod_management_state::is_state_directory_name)
}

pub(super) fn rewrite_owner(
    value: &mut Value,
    moves: &[RootMove],
    file: &Path,
) -> Result<(), StorageError> {
    let owner = value
        .as_str()
        .filter(|owner| !owner.is_empty())
        .ok_or_else(|| invalid(file, "Invalid Mod management storage identity."))?;
    let target = remap_path(Path::new(owner), moves)?;
    *value = Value::String(crate::mod_management_state::target_identity(&target));
    Ok(())
}
