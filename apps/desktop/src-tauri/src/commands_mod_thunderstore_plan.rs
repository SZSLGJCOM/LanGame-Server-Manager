use super::*;

use crate::commands::commands_mod_staging::{
    ThunderstoreGameProfile, runtime_loader_requires_archive, thunderstore_game_profile,
};

pub(in crate::commands) struct PreparedThunderstoreInstall {
    pub(super) sources: DownloadedManualModSourcesCleanup,
    pub(super) loader_archive: Option<PathBuf>,
}

impl PreparedThunderstoreInstall {
    pub(in crate::commands) fn sources(&self) -> &[DownloadedManualModSource] {
        &self.sources
    }
    pub(in crate::commands) fn loader_archive(&self) -> Option<&Path> {
        self.loader_archive.as_deref()
    }
}

impl Drop for PreparedThunderstoreInstall {
    fn drop(&mut self) {
        if let Some(path) = self.loader_archive.take() {
            cleanup_downloaded_mod_source(&path);
        }
    }
}

pub(in crate::commands) async fn prepare_thunderstore_install(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
) -> Result<PreparedThunderstoreInstall, String> {
    let profile = thunderstore_game_profile(module_id)?;
    if !source.provider.trim().eq_ignore_ascii_case("thunderstore")
        || thunderstore_community(&source.url)? != profile.community
    {
        return Err(format!(
            "{} Mod installation requires its own Thunderstore community ({})",
            profile.module_id, profile.community
        ));
    }
    let owned_root = root.to_path_buf();
    let needs_archive = tokio::task::spawn_blocking(move || {
        if profile.loader.is_some() {
            runtime_loader_requires_archive(profile.module_id, &owned_root)
        } else {
            validate_runtime(
                profile.module_id,
                "thunderstore",
                &owned_root,
                &owned_root.join(profile.target_relative),
            )?;
            Ok(false)
        }
    })
    .await
    .map_err(|error| format!("Thunderstore runtime inspection worker failed: {error}"))??;
    let client = crate::commands::commands_mods::mod_download::mod_package_client()?;
    let packages = resolve_packages(profile, references, |package| {
        let client = &client;
        async move {
            let mut metadata =
                fetch_thunderstore_package_metadata(client, &package.namespace, &package.package)
                    .await?;
            if let Some(version) = package.version.as_deref() {
                metadata.latest = fetch_thunderstore_version(client, &package, version).await?;
            }
            Ok(metadata)
        }
    })
    .await?;
    let mut prepared = PreparedThunderstoreInstall {
        sources: DownloadedManualModSourcesCleanup::default(),
        loader_archive: None,
    };
    if needs_archive {
        let loader = profile
            .loader
            .ok_or("Native Thunderstore runtime requested a loader archive")?;
        let download_root = std::env::temp_dir();
        prepared.loader_archive = Some(
            download_mod_site_file_with_limit(
                &client,
                ModDownloadRequest {
                    download_url: loader.download_url.as_str(),
                    package_name: loader.project.as_str(),
                    version: loader.version.as_str(),
                    preferred_filename: None,
                    fallback_extension: "zip",
                    integrity: None,
                    max_bytes: Some(loader.archive_bytes),
                    download_root: &download_root,
                },
            )
            .await?,
        );
    }
    for metadata in packages {
        let archive = download_mod_site_archive(
            &client,
            &metadata.latest.download_url,
            &metadata.full_name,
            &metadata.latest.version_number,
        )
        .await?;
        let path = prepare_downloaded_thunderstore_archive(
            archive,
            &metadata.full_name,
            &metadata.latest.version_number,
            profile.module_id,
        )
        .await?;
        prepared.sources.push(DownloadedManualModSource {
            path,
            label: metadata.full_name.clone(),
            identity: OnlineModIdentity {
                provider: "thunderstore".into(),
                project: metadata.full_name.clone(),
                historical_names: vec![metadata.full_name],
                version: metadata.latest.version_number,
                dependencies: metadata.latest.dependencies,
            },
        });
    }
    Ok(prepared)
}

fn dependency_reference(value: &str) -> Result<ThunderstorePackageReference, String> {
    let (project, version) = value
        .rsplit_once('-')
        .ok_or("Invalid Thunderstore dependency identity")?;
    let (namespace, package) = project
        .split_once('-')
        .ok_or("Invalid Thunderstore dependency identity")?;
    let reference = extract_thunderstore_package_reference(&format!(
        "https://thunderstore.io/package/{namespace}/{package}/{version}/"
    ))
    .ok_or("Invalid Thunderstore dependency identity")?;
    if format!(
        "{}-{}-{}",
        reference.namespace,
        reference.package,
        reference.version.as_deref().unwrap_or_default()
    ) != value
    {
        return Err("Invalid Thunderstore dependency identity".into());
    }
    Ok(reference)
}

/// Fetch every declared plugin dependency at its stated version, with a bounded
/// graph. Explicit conflicting versions are rejected before modifying runtime.
async fn resolve_packages<F, Fut>(
    profile: &ThunderstoreGameProfile,
    references: &[String],
    mut fetch: F,
) -> Result<Vec<ThunderstorePackageMetadata>, String>
where
    F: FnMut(ThunderstorePackageReference) -> Fut,
    Fut: std::future::Future<Output = Result<ThunderstorePackageMetadata, String>>,
{
    if references.is_empty() || references.len() > MAX_ONLINE_PACKAGES {
        return Err(
            "A Thunderstore Mod installation requires between 1 and 32 package links".into(),
        );
    }
    let mut pending = std::collections::VecDeque::new();
    for reference in references {
        pending.push_back((
            extract_thunderstore_package_reference(reference).ok_or_else(|| {
                format!("Thunderstore link did not identify a package: {reference}")
            })?,
            0,
        ));
    }
    let mut selected = HashMap::<String, String>::new();
    let mut packages = Vec::new();
    while let Some((package, depth)) = pending.pop_front() {
        if depth > 16 || pending.len() > 128 {
            return Err(
                "Thunderstore dependency graph exceeds its 128-package or 16-level limit".into(),
            );
        }
        let project = format!("{}-{}", package.namespace, package.package);
        if profile.loader.is_none() && package.package.starts_with("BepInExPack") {
            return Err("This Core Keeper package requires the old BepInEx loading system. The current server uses native Mods; choose a compatible native package from mod.io or Workshop.".into());
        }
        if let Some(loader) = profile.loader.filter(|loader| loader.project == project) {
            if depth == 0
                && package
                    .version
                    .as_deref()
                    .is_some_and(|version| version != loader.version)
            {
                return Err(format!(
                    "LGSM supports the verified {} loader version {}; the explicitly selected loader version was preserved",
                    profile.module_id, loader.version
                ));
            }
            if package
                .version
                .as_deref()
                .is_some_and(|version| !loader.supports_dependency(&project, version))
            {
                return Err(format!(
                    "This {} Mod requires an unsupported loader version; LGSM provides {}-{}",
                    profile.module_id, loader.project, loader.version
                ));
            }
            continue;
        }
        if let Some(version) = selected.get(&project) {
            if package
                .version
                .as_ref()
                .is_some_and(|required| required != version)
            {
                return Err(format!(
                    "Thunderstore Mods select conflicting versions of dependency {project}"
                ));
            }
            continue;
        }
        if selected.len() >= 128 {
            return Err(
                "Thunderstore dependency graph exceeds its 128-package or 16-level limit".into(),
            );
        }
        let requested = package.version.clone();
        let metadata = fetch(package).await?;
        validate_thunderstore_metadata(&metadata, profile.community, &project)?;
        if requested.is_some_and(|version| version != metadata.latest.version_number) {
            return Err(
                "Thunderstore returned a different explicitly selected dependency version".into(),
            );
        }
        selected.insert(project, metadata.latest.version_number.clone());
        for dependency in &metadata.latest.dependencies {
            if pending.len() >= 128 {
                return Err(
                    "Thunderstore dependency graph exceeds its 128-package or 16-level limit"
                        .into(),
                );
            }
            pending.push_back((dependency_reference(dependency)?, depth + 1));
        }
        packages.push(metadata);
    }
    Ok(packages)
}

#[cfg(test)]
#[path = "commands_mod_thunderstore_plan_tests.rs"]
mod tests;
