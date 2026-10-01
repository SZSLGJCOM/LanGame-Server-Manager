use super::*;

#[cfg(test)]
pub(super) fn combine_soulmask_profile_templates(config_dir: &Path) -> Result<(), StorageError> {
    let mut files = ManagedConfigMutation::new("soulmask");
    match combine_soulmask_profile_templates_pending(config_dir, &mut files) {
        Ok(()) => {
            files.commit();
            Ok(())
        }
        Err(error) => Err(files.rollback_after(error)),
    }
}

pub(super) fn combine_soulmask_profile_templates_pending(
    config_dir: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let mut profiles = Map::new();
    let mut profile_paths = Vec::new();
    for profile_id in ["0", "1", "2"] {
        let path = config_dir.join(format!("GameXishu.profile-{profile_id}.json"));
        let text = fs::read_to_string(&path).map_err(|source| StorageError::ReadConfig {
            path: path.clone(),
            source,
        })?;
        let mut document = serde_json::from_str::<Value>(&text)?;
        let profile = document
            .as_object_mut()
            .and_then(|object| object.remove(profile_id))
            .filter(Value::is_object)
            .ok_or_else(|| StorageError::ModuleSupportMaterialization {
                module_id: String::from("soulmask"),
                path: path.clone(),
                message: format!("rendered profile must contain object key {profile_id:?}"),
            })?;
        profiles.insert(String::from(profile_id), profile);
        profile_paths.push(path);
    }

    let mut bytes = serde_json::to_vec_pretty(&Value::Object(profiles))?;
    bytes.push(b'\n');
    let output_path = config_dir.join(SOULMASK_GAME_XISHU_FILE);
    files.write(&output_path, &bytes)?;
    for path in profile_paths {
        files.remove(&path)?;
    }
    Ok(())
}
