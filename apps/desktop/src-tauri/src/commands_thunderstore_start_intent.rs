//! A package version is not evidence that the user pinned a root. Preserve the
//! original root/dependency role explicitly; absent old records stay unknown.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::commands) enum ThunderstoreInstallIntent {
    Root { version: Option<String> },
    Dependency,
}

pub(in crate::commands) type ThunderstoreInstallIntents =
    BTreeMap<String, ThunderstoreInstallIntent>;

pub(in crate::commands) fn thunderstore_project(key: &str) -> Result<&str, String> {
    let project = key
        .strip_prefix("thunderstore-")
        .ok_or("Managed package has no Thunderstore identity")?;
    let (namespace, name) = project
        .split_once('-')
        .ok_or("Managed Thunderstore identity is invalid")?;
    let link = format!("https://thunderstore.io/package/{namespace}/{name}/");
    let parsed = crate::commands::commands_mods::extract_thunderstore_package_reference(&link)
        .ok_or("Managed Thunderstore identity is invalid")?;
    if format!("{}-{}", parsed.namespace, parsed.package) != project {
        return Err("Managed Thunderstore identity is invalid".into());
    }
    Ok(project)
}

#[cfg(test)]
pub(in crate::commands) fn root_thunderstore_intents(
    references: &[String],
) -> Result<ThunderstoreInstallIntents, String> {
    root_thunderstore_intents_with_policy(references, None)
}

pub(in crate::commands) fn root_thunderstore_intents_with_policy(
    references: &[String],
    policy: Option<&str>,
) -> Result<ThunderstoreInstallIntents, String> {
    if !matches!(policy, None | Some("follow_latest" | "pinned")) {
        return Err("Unsupported Thunderstore version policy".into());
    }
    if references.is_empty() || references.len() > 32 {
        return Err("Thunderstore requires 1 to 32 root requests".into());
    }
    let mut result = BTreeMap::new();
    for reference in references {
        let parsed =
            crate::commands::commands_mods::extract_thunderstore_package_reference(reference)
                .ok_or("Thunderstore source request must identify an official package")?;
        let key = format!("thunderstore-{}-{}", parsed.namespace, parsed.package);
        thunderstore_project(&key)?;
        if policy == Some("pinned") && parsed.version.is_none() {
            return Err(
                "Fixed Thunderstore versions require a version in every root reference".into(),
            );
        }
        let intent = ThunderstoreInstallIntent::Root {
            version: if policy == Some("follow_latest") {
                None
            } else {
                parsed.version
            },
        };
        if result
            .insert(key, intent.clone())
            .is_some_and(|old| old != intent)
        {
            return Err("Thunderstore root requests contain conflicting version intentions".into());
        }
    }
    Ok(result)
}

pub(super) fn retained_intent(
    previous: Option<&InstalledPackage>,
    incoming: Option<&ThunderstoreInstallIntent>,
    version: &str,
) -> Result<Option<ThunderstoreInstallIntent>, String> {
    let prior = previous.and_then(|package| package.thunderstore_intent.as_ref());
    let next = match incoming {
        Some(ThunderstoreInstallIntent::Root { .. }) => incoming.cloned(),
        Some(ThunderstoreInstallIntent::Dependency) => match prior {
            Some(ThunderstoreInstallIntent::Root { .. }) => prior.cloned(),
            None if previous.is_some() => {
                if previous.and_then(|package| package.version.as_deref()) != Some(version) {
                    return Err("Existing Thunderstore package root/version intent is unknown; confirm it directly before updating it as another package's dependency".into());
                }
                None
            }
            _ => incoming.cloned(),
        },
        None => prior.cloned(),
    };
    if let Some(ThunderstoreInstallIntent::Root { version: Some(pin) }) = &next
        && pin != version
    {
        return Err("Thunderstore update conflicts with the user's explicitly selected root version; the pin was preserved".into());
    }
    Ok(next)
}

pub(super) fn validate_thunderstore_intent(
    key: &str,
    package: &InstalledPackage,
) -> Result<(), String> {
    let Some(intent) = &package.thunderstore_intent else {
        return Ok(());
    };
    thunderstore_project(key)?;
    if let ThunderstoreInstallIntent::Root {
        version: Some(version),
    } = intent
        && (!crate::commands::commands_mods::mod_online::valid_thunderstore_version(version)
            || package.version.as_deref() != Some(version))
    {
        return Err("Thunderstore root pin does not match its installed version".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_thunderstore_start_intent_tests.rs"]
mod tests;
