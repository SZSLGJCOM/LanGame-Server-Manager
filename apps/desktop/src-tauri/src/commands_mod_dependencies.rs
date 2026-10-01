use super::*;

const MAX_DEPENDENCIES: usize = 128;
const MAX_DEPENDENCY_DEPTH: usize = 16;

pub(super) fn verify_dependencies(
    target: &Path,
    manifest: &InstalledPackages,
    identities: &[OnlineModIdentity],
) -> Result<(), String> {
    let incoming = identities
        .iter()
        .filter(|identity| identity.provider == "thunderstore")
        .map(|identity| (identity.project.as_str(), identity))
        .collect::<BTreeMap<_, _>>();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut pending = Vec::new();
    for identity in incoming.values() {
        queue_dependencies(&mut pending, &identity.dependencies, 0)?;
    }
    queue_affected_consumers(manifest, &incoming, &mut pending, deadline)?;
    let mut visited = HashSet::new();
    let mut missing = std::collections::BTreeSet::new();
    while let Some((dependency, depth)) = pending.pop() {
        check_deadline(deadline)?;
        if !visited.insert(dependency.clone()) {
            continue;
        }
        if visited.len() > MAX_DEPENDENCIES || depth > MAX_DEPENDENCY_DEPTH {
            return Err(graph_limit());
        }
        let (name, version) = parse_thunderstore_dependency(&dependency)?;
        let dependencies = if let Some(identity) = incoming.get(name) {
            // A replacement masks the old record even when its version does not
            // match: using the old bytes here would validate a broken final state.
            (identity.version == version).then_some(identity.dependencies.as_slice())
        } else if let Some(package) = manifest.packages.get(&format!("thunderstore-{name}")) {
            if package.version.as_deref() != Some(version) || package.dependencies.is_none() {
                None
            } else {
                let current = fingerprint_tree(&target.join(&package.target_name))?;
                if !package.files.is_empty()
                    && package
                        .files
                        .iter()
                        .all(|(path, hash)| current.get(path) == Some(hash))
                {
                    package.dependencies.as_deref()
                } else {
                    None
                }
            }
        } else {
            None
        };
        match dependencies {
            Some(dependencies) => queue_dependencies(&mut pending, dependencies, depth + 1)?,
            None => {
                missing.insert(dependency);
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let dependencies = missing.into_iter().collect::<Vec<_>>();
    Err(serde_json::json!({
        "code": "mod_dependencies_unverified", "dependencies": dependencies,
        "message": format!(
            "Required Thunderstore dependencies are missing, changed, or have an unverified version: {}. Include the exact dependency versions in this installation, or install the complete package manually. Loader packages must be installed separately in the server root.",
            dependencies.join(", ")
        ),
    }).to_string())
}

/// Validate existing consumers in the replacement's reverse dependency closure.
/// Unrelated old records without version/dependency evidence do not gate installs.
fn queue_affected_consumers(
    manifest: &InstalledPackages,
    incoming: &BTreeMap<&str, &OnlineModIdentity>,
    pending: &mut Vec<(String, usize)>,
    deadline: Instant,
) -> Result<(), String> {
    if incoming.is_empty() {
        return Ok(());
    }
    let mut reverse = BTreeMap::<&str, Vec<(&str, &InstalledPackage)>>::new();
    for (key, package) in &manifest.packages {
        check_deadline(deadline)?;
        let Some(name) = key.strip_prefix("thunderstore-") else {
            continue;
        };
        if incoming.contains_key(name) {
            continue;
        }
        for dependency in package.dependencies.iter().flatten() {
            if let Ok((dependency_name, _)) = parse_thunderstore_dependency(dependency) {
                reverse
                    .entry(dependency_name)
                    .or_default()
                    .push((name, package));
            }
        }
    }
    let mut affected = incoming
        .keys()
        .copied()
        .map(|name| (name, 0usize))
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    while let Some((name, depth)) = affected.pop() {
        check_deadline(deadline)?;
        if !visited.insert(name) {
            continue;
        }
        if visited.len() > MAX_DEPENDENCIES || depth > MAX_DEPENDENCY_DEPTH {
            return Err(graph_limit());
        }
        for (consumer, package) in reverse.get(name).into_iter().flatten() {
            if visited.contains(consumer) {
                continue;
            }
            // Known reverse edges imply a present dependency list. Queuing the
            // complete list also validates transitive requirements of this consumer.
            if let Some(dependencies) = &package.dependencies {
                queue_dependencies(pending, dependencies, 0)?;
            }
            affected.push((consumer, depth + 1));
            if affected.len() > MAX_DEPENDENCIES {
                return Err(graph_limit());
            }
        }
    }
    Ok(())
}

fn queue_dependencies(
    pending: &mut Vec<(String, usize)>,
    dependencies: &[String],
    depth: usize,
) -> Result<(), String> {
    if dependencies.len() > MAX_DEPENDENCIES
        || pending.len() + dependencies.len() > MAX_DEPENDENCIES
    {
        return Err(graph_limit());
    }
    pending.extend(
        dependencies
            .iter()
            .map(|dependency| (dependency.clone(), depth)),
    );
    Ok(())
}

fn graph_limit() -> String {
    "Thunderstore dependency graph exceeds its 128-package or 16-level limit".into()
}

fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        return Err("Thunderstore dependency verification exceeded its 15-second limit".into());
    }
    Ok(())
}

fn parse_thunderstore_dependency(value: &str) -> Result<(&str, &str), String> {
    let valid = value.len() <= 300
        && value.rsplit_once('-').is_some_and(|(name, version)| {
            let parts = name.split('-').collect::<Vec<_>>();
            parts.len() == 2
                && parts.iter().all(|part| {
                    !part.is_empty()
                        && part.len() <= 128
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                })
                && version.split('.').count() == 3
                && version.split('.').all(|part| {
                    !part.is_empty()
                        && part.bytes().all(|byte| byte.is_ascii_digit())
                        && part.parse::<u32>().is_ok()
                })
        });
    if !valid {
        return Err("Thunderstore metadata contains an invalid dependency identity".into());
    }
    value
        .rsplit_once('-')
        .ok_or_else(|| "invalid Thunderstore dependency identity".into())
}
