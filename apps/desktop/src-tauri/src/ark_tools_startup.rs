//! Refresh only the owned ASA loader's startup policy; explicit preparation
//! remains responsible for plugin upgrades and executable-matched symbols.
use super::*;

pub(super) fn refresh(root: &Path, embedded: EmbeddedArkTools) -> Result<(), String> {
    refresh_profile(root, &ASA, embedded)
}

pub(super) fn refresh_profile(
    root: &Path,
    profile: &Profile,
    embedded: EmbeddedArkTools,
) -> Result<(), String> {
    let current = embedded_files(profile, embedded)?;
    let expected = expected(profile, embedded)?;
    let (bin, _pins) = files::runtime(root, profile.executable)?;
    if transaction::pending(&bin)? || removal::pending(&bin)? {
        return Err("Recover the interrupted ARK extension operation before starting".into());
    }
    let Some(owned) = files::ownership(&bin, profile, &expected)? else {
        return Ok(());
    };
    // A full uninstall needs no action. A framework retained for other plugins
    // still needs the local-only loader, without resurrecting LanGame tools.
    let plugin_removed = !removal::available(&bin, &owned)?;
    if plugin_removed && files::read(&bin, "version.dll")?.is_none() {
        return Ok(());
    }
    let mut payload = BTreeMap::new();
    for (path, hash) in &owned.files {
        if plugin_removed && matches!(path.as_str(), PLUGIN | INFO | NOTICES) {
            continue;
        }
        let bytes = files::read(&bin, path)?.ok_or_else(|| {
            format!("Repair the missing ARK extension file before starting: {path}")
        })?;
        if digest(&bytes) != *hash {
            return Err(format!("Preserved a modified ARK extension file: {path}"));
        }
        payload.insert(path.clone(), bytes);
    }
    if ["version.dll", "config.json"]
        .iter()
        .all(|path| payload[*path] == current[*path])
    {
        return Ok(());
    }
    for path in ["version.dll", "config.json"] {
        payload.insert(path.into(), current[path].clone());
    }
    let mut wanted = owned.files;
    for (path, bytes) in &payload {
        wanted.insert(path.clone(), digest(bytes));
    }
    // The existing journal publishes the loader last and verifies originals
    // again before replacing them. Plugin/framework bytes remain unchanged.
    publish(root, profile, payload, &wanted)
}
