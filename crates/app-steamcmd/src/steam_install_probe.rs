//! Ordinary status probes distinguish an installed build from a future update.
//! Acquisition certification continues to use the strict ACF completion reader.
use crate::steam_depot::manifest::{Object, decimal, depot_gid, object, text};

pub(super) fn has_prefetched_update(app: &Object) -> bool {
    let number = |key| {
        app.get(key)
            .and_then(|value| text(value).ok())
            .and_then(|value| decimal(value).ok())
    };
    // SteamCMD can prefetch main-app update metadata during a Workshop-only
    // session. Exactly FullyInstalled | UpdateRequired, with no payload work,
    // describes the existing build plus an available update, not a partial install.
    if number("stateflags") != Some(6)
        || !number("buildid").is_some_and(|build| build > 0)
        || !number("targetbuildid")
            .is_some_and(|target| target > 0 && Some(target) != number("buildid"))
        || !number("bytestodownload").is_some_and(|bytes| bytes > 0)
        || number("bytesdownloaded") != Some(0)
        || number("bytestostage") != Some(0)
        || number("bytesstaged") != Some(0)
        || app.contains_key("updateresult") && number("updateresult") != Some(0)
    {
        return false;
    }
    if let Some(staged) = app.get("stageddepots")
        && !object(staged).is_ok_and(|depots| depots.is_empty())
    {
        return false;
    }
    app.get("installeddepots")
        .and_then(|value| object(value).ok())
        .is_some_and(|depots| {
            !depots.is_empty()
                && depots.iter().all(|(id, details)| {
                    decimal(id).is_ok_and(|id| id > 0)
                        && depot_gid(details).is_ok_and(|gid| gid > 0)
                })
        })
}

#[cfg(test)]
#[path = "steam_install_probe_tests.rs"]
mod tests;
