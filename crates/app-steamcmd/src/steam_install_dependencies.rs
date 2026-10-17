//! Complete declared Steam shared depots before certifying or publishing files.
//! This gate reads installation metadata; package verification still owns hashes.
use super::*;
use crate::steam_depot::manifest::{
    Object, acf_is_complete, decimal, depot_gid, object, parse_acf, parse_acf_app_state, text,
};
use crate::steam_depot::{MAX_ACF, bounded, invalid};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

pub(super) struct SharedDependencyCompletion<'a> {
    pub steamcmd: &'a SteamCmdStatus,
    pub module: &'a ModuleDetails,
    pub app_id: u32,
    pub root: &'a Path,
    pub script: &'a Path,
    pub deadline: InstallDeadline,
    pub account_name: Option<&'a str>,
}

pub(super) async fn complete_shared_dependencies(
    request: SharedDependencyCompletion<'_>,
    on_progress: &mut impl FnMut(InstallProgressUpdate),
    original_excerpt: String,
) -> Result<String, SteamCmdError> {
    let SharedDependencyCompletion {
        steamcmd,
        module,
        app_id,
        root,
        script,
        deadline,
        account_name,
    } = request;
    let failed = |detail| SteamCmdError::InstallationVerificationFailed {
        module_id: module.summary.id.clone(),
        operation: "completing Steam shared dependencies".into(),
        detail,
    };
    deadline.check_cancelled()?;
    let inspected = inspect_dependencies(root, app_id, deadline).await;
    deadline.check_cancelled()?;
    let missing = inspected.map_err(|error| failed(error.to_string()))?;
    if missing.is_empty() {
        return Ok(original_excerpt);
    }
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        format!("Completing Steam shared dependencies for AppID {app_id}: {missing:?}"),
    ));
    deadline.check_cancelled()?;
    // Repeat the trusted main app's validation, never arbitrary app IDs taken
    // from metadata. This keeps platform selection, destination and locks intact.
    let lines =
        super::steamcmd_install_script_lines_with_login(module, app_id, root, true, account_name);
    fs::write(script, lines.join("\r\n")).map_err(|source| SteamCmdError::WriteScript {
        path: script.to_owned(),
        source,
    })?;
    let output = run_steamcmd_script_with_progress(
        &steamcmd.executable_path,
        script,
        &steamcmd.root,
        deadline,
        on_progress,
    )
    .await?;
    crate::steam_app_access::verify_command_result(app_id, account_name, &output)?;
    let inspected = inspect_dependencies(root, app_id, deadline).await;
    deadline.check_cancelled()?;
    let missing = inspected.map_err(|error| failed(error.to_string()))?;
    if !missing.is_empty() {
        return Err(failed(format!(
            "Steam shared dependencies remain incomplete after validation: {missing:?}"
        )));
    }
    Ok(output.excerpt)
}

async fn inspect_dependencies(
    root: &Path,
    app_id: u32,
    deadline: InstallDeadline,
) -> io::Result<Vec<u32>> {
    let root = root.to_owned();
    let cancellation = InstallCancellation::current();
    // Join the bounded reader before returning, including cancellation. The
    // caller retains the native install lease while this worker observes files.
    tokio::task::spawn_blocking(move || {
        unfinished_dependencies(&root, app_id, || {
            if cancellation
                .as_ref()
                .is_some_and(|token| token.is_cancelled())
                || tokio::time::Instant::now() >= deadline.expires_at()
            {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Steam dependency inspection cancelled",
                ))
            } else {
                Ok(())
            }
        })
    })
    .await
    .map_err(|error| io::Error::other(format!("Steam dependency reader failed: {error}")))?
}

fn unfinished_dependencies(
    root: &Path,
    app_id: u32,
    check: impl Fn() -> io::Result<()>,
) -> io::Result<Vec<u32>> {
    check()?;
    let main = root.join(format!("steamapps/appmanifest_{app_id}.acf"));
    let app = parse_acf(&bounded(&main, MAX_ACF)?, app_id)?;
    unfinished_shared_depots(root, &app, check)
}

pub(crate) fn unfinished_shared_depots(
    root: &Path,
    app: &Object,
    check: impl Fn() -> io::Result<()>,
) -> io::Result<Vec<u32>> {
    check()?;
    let Some(shared) = app.get("shareddepots") else {
        return Ok(Vec::new());
    };
    let shared = object(shared)?;
    if shared.len() > 128 {
        return Err(invalid("too many Steam shared depots"));
    }
    let mut owners: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    for (depot, owner) in shared {
        let owner = u32::try_from(decimal(text(owner)?)?).map_err(invalid)?;
        if owner == 0 || decimal(depot)? == 0 {
            return Err(invalid("invalid Steam shared depot or owner ID"));
        }
        owners.entry(owner).or_default().insert(depot.clone());
    }
    let mut unfinished = Vec::new();
    for (owner, depots) in owners {
        check()?;
        let path = root.join(format!("steamapps/appmanifest_{owner}.acf"));
        let bytes = match bounded(&path, MAX_ACF) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                unfinished.push(owner);
                continue;
            }
            Err(error) => return Err(error),
        };
        let app = parse_acf_app_state(&bytes, owner)
            .map_err(|error| invalid(format!("appmanifest_{owner}.acf: {error}")))?;
        let mut complete = acf_is_complete(&app)?;
        let installed = app.get("installeddepots").map(object).transpose()?;
        complete &= installed.is_some();
        for depot in depots {
            if let Some(details) = installed.and_then(|installed| installed.get(&depot)) {
                complete &= depot_gid(details)? != 0;
            } else {
                complete = false;
            }
        }
        if !complete {
            unfinished.push(owner);
        }
    }
    check()?;
    Ok(unfinished)
}

#[cfg(test)]
#[path = "steam_install_dependencies_tests.rs"]
mod tests;
