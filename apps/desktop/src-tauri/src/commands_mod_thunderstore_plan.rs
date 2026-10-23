use super::*;
#[cfg(test)]
#[path = "commands_thunderstore_start_provider_fixture.rs"]
pub(in crate::commands) mod provider_fixture;

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

pub(in crate::commands) struct ThunderstorePreparationFailure {
    pub(in crate::commands) message: String,
    kind: PreparationFailureKind,
}

enum PreparationFailureKind {
    Local,
    Remote(app_core::RuntimeModUpdateReason),
    Cancelled,
}

impl ThunderstorePreparationFailure {
    pub(in crate::commands) fn remote(
        message: String,
        reason: app_core::RuntimeModUpdateReason,
    ) -> Self {
        Self {
            message,
            kind: PreparationFailureKind::Remote(reason),
        }
    }

    pub(in crate::commands) fn cancelled() -> Self {
        Self {
            message: "Thunderstore startup Mod preparation was cancelled".into(),
            kind: PreparationFailureKind::Cancelled,
        }
    }

    pub(in crate::commands) fn reason(&self) -> Option<app_core::RuntimeModUpdateReason> {
        match self.kind {
            PreparationFailureKind::Remote(reason) => Some(reason),
            _ => None,
        }
    }

    pub(in crate::commands) fn is_cancelled(&self) -> bool {
        matches!(self.kind, PreparationFailureKind::Cancelled)
    }
}

impl From<String> for ThunderstorePreparationFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            kind: PreparationFailureKind::Local,
        }
    }
}

fn download_failure(
    error: crate::commands::commands_mods::mod_download::ModDownloadFailure,
) -> ThunderstorePreparationFailure {
    use crate::commands::commands_mods::mod_download::ModDownloadFailure;
    let reason = match error {
        ModDownloadFailure::Network(_) | ModDownloadFailure::Interrupted(_) => {
            Some(app_core::RuntimeModUpdateReason::Network)
        }
        ModDownloadFailure::Http(_) => Some(app_core::RuntimeModUpdateReason::Http),
        ModDownloadFailure::Terminal(_) => None,
        ModDownloadFailure::Cancelled => return preparation_cancelled(),
    };
    let message =
        "Thunderstore Mod package download was not completed; no new package was published".into();
    match reason {
        Some(reason) => ThunderstorePreparationFailure::remote(message, reason),
        None => message.into(),
    }
}

fn preparation_cancelled() -> ThunderstorePreparationFailure {
    ThunderstorePreparationFailure::cancelled()
}

async fn provider_result<T, F: std::future::Future<Output = Result<T, String>>>(
    future: F,
    cancellation: Option<&app_steamcmd::InstallCancellation>,
    reason: app_core::RuntimeModUpdateReason,
) -> Result<T, ThunderstorePreparationFailure> {
    let result = tokio::select! {
        biased;
        _=async {match cancellation {Some(cancellation)=>cancellation.cancelled().await,None=>std::future::pending().await}}=>return Err(preparation_cancelled()),
        result=future=>result,
    };
    result.map_err(|_| {
        ThunderstorePreparationFailure::remote(
            "Thunderstore 模组版本查询或包准备未完成；尚未发布新的模组。".into(),
            reason,
        )
    })
}

#[cfg(test)]
async fn prepare_thunderstore_install(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
) -> Result<PreparedThunderstoreInstall, String> {
    prepare_thunderstore_install_inner(
        module_id,
        source,
        references,
        root,
        PreparationOptions {
            #[cfg(test)]
            fixture: None,
            installed: &[],
            cancellation: None,
            existing_runtime_only: false,
            version_policy: None,
        },
    )
    .await
    .map_err(|error| error.message)
}

/// Automatic save loaders use the same pinned release preparation, retaining
/// remote failure kinds and cooperative cancellation before any runtime write.
pub(in crate::commands) async fn prepare_thunderstore_save_loader(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<PreparedThunderstoreInstall, ThunderstorePreparationFailure> {
    prepare_thunderstore_install_inner(
        module_id,
        source,
        references,
        root,
        PreparationOptions {
            #[cfg(test)]
            fixture: None,
            installed: &[],
            cancellation: Some(cancellation),
            existing_runtime_only: false,
            version_policy: None,
        },
    )
    .await
}

pub(in crate::commands) async fn prepare_thunderstore_install_with_policy(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
    version_policy: Option<&str>,
) -> Result<PreparedThunderstoreInstall, String> {
    prepare_thunderstore_install_inner(
        module_id,
        source,
        references,
        root,
        PreparationOptions {
            #[cfg(test)]
            fixture: None,
            installed: &[],
            cancellation: None,
            existing_runtime_only: false,
            version_policy,
        },
    )
    .await
    .map_err(|error| error.message)
}

struct PreparationOptions<'a> {
    #[cfg(test)]
    fixture: Option<&'a provider_fixture::FixtureProvider>,
    installed: &'a [crate::commands::commands_mod_staging::CachedThunderstorePackage],
    cancellation: Option<&'a app_steamcmd::InstallCancellation>,
    existing_runtime_only: bool,
    version_policy: Option<&'a str>,
}

pub(in crate::commands) async fn prepare_thunderstore_install_for_startup(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
    installed: &[crate::commands::commands_mod_staging::CachedThunderstorePackage],
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<PreparedThunderstoreInstall, ThunderstorePreparationFailure> {
    prepare_thunderstore_install_inner(
        module_id,
        source,
        references,
        root,
        PreparationOptions {
            #[cfg(test)]
            fixture: None,
            installed,
            cancellation: Some(cancellation),
            existing_runtime_only: true,
            version_policy: None,
        },
    )
    .await
}

async fn prepare_thunderstore_install_inner(
    module_id: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
    options: PreparationOptions<'_>,
) -> Result<PreparedThunderstoreInstall, ThunderstorePreparationFailure> {
    let PreparationOptions {
        #[cfg(test)]
        fixture,
        installed,
        cancellation,
        existing_runtime_only,
        version_policy,
    } = options;
    let profile = thunderstore_game_profile(module_id)?;
    if !source.provider.trim().eq_ignore_ascii_case("thunderstore")
        || thunderstore_community(&source.url)? != profile.community
    {
        return Err(format!(
            "{} Mod installation requires its own Thunderstore community ({})",
            profile.module_id, profile.community
        )
        .into());
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
    if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
        return Err(preparation_cancelled());
    }
    if existing_runtime_only && needs_archive {
        return Err(
            "Thunderstore's compatible runtime loader must be verified before startup"
                .to_owned()
                .into(),
        );
    }
    let intents = crate::commands::commands_mod_staging::root_thunderstore_intents_with_policy(
        references,
        version_policy,
    )?;
    let client = crate::commands::commands_mods::mod_download::mod_package_client()?;
    let packages = provider_result(
        resolve_packages(profile, references, |package| {
            let client = &client;
            async move {
                #[cfg(test)]
                if let Some(fixture) = fixture {
                    return fixture.metadata(&package, cancellation);
                }
                let mut metadata = fetch_thunderstore_package_metadata(
                    client,
                    &package.namespace,
                    &package.package,
                )
                .await?;
                if let Some(version) = package.version.as_deref() {
                    metadata.latest = fetch_thunderstore_version(client, &package, version).await?;
                }
                Ok(metadata)
            }
        }),
        cancellation,
        app_core::RuntimeModUpdateReason::Request,
    )
    .await?;
    let mut prepared = PreparedThunderstoreInstall {
        sources: DownloadedManualModSourcesCleanup::default(),
        loader_archive: None,
    };
    if needs_archive {
        let loader = profile
            .loader
            .ok_or_else(|| "Native Thunderstore runtime requested a loader archive".to_owned())?;
        let download_root = std::env::temp_dir();
        let request = ModDownloadRequest {
            download_url: loader.download_url.as_str(),
            package_name: loader.project.as_str(),
            version: loader.version.as_str(),
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: Some(loader.archive_bytes),
            download_root: &download_root,
        };
        prepared.loader_archive = Some(match cancellation {
            Some(cancellation)=>crate::commands::commands_mods::mod_download::download_mod_site_file_with_limit_and_cancellation(&client,request,cancellation).await,
            None=>download_mod_site_file_with_limit(
                &client,
                request,
            )
            .await.map_err(crate::commands::commands_mods::mod_download::ModDownloadFailure::Terminal),
        }.map_err(download_failure)?);
    }
    for metadata in packages {
        if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
            return Err(preparation_cancelled());
        }
        if !package_needs_download(installed, &metadata) {
            continue;
        }
        let download_root = std::env::temp_dir();
        let request = ModDownloadRequest {
            download_url: &metadata.latest.download_url,
            package_name: &metadata.full_name,
            version: &metadata.latest.version_number,
            preferred_filename: None,
            fallback_extension: "zip",
            integrity: None,
            max_bytes: None,
            download_root: &download_root,
        };
        // Cancellation is cooperative in the real file writer. Never drop its
        // future while it owns a staging thread or pending publication channel.
        let archive = {
            #[cfg(test)]
            if let Some(fixture) = fixture {
                fixture.archive(&metadata.full_name, &metadata.latest.version_number)?
            } else {
                download_selected_package(&client, request, cancellation).await?
            }
            #[cfg(not(test))]
            download_selected_package(&client, request, cancellation).await?
        };
        let path = prepare_downloaded_thunderstore_archive(
            archive,
            &metadata.full_name,
            &metadata.latest.version_number,
            profile.module_id,
        )
        .await
        .map_err(|_| {
            ThunderstorePreparationFailure::from(
                "Thunderstore Mod archive preparation was not completed".to_owned(),
            )
        })?;
        if cancellation.is_some_and(|cancellation| cancellation.is_cancelled()) {
            cleanup_downloaded_mod_source(&path);
            return Err(preparation_cancelled());
        }
        prepared.sources.push(DownloadedManualModSource {
            path,
            label: metadata.full_name.clone(),
            identity: OnlineModIdentity {
                thunderstore_intent:Some(intents.get(&format!("thunderstore-{}",metadata.full_name)).cloned().unwrap_or(crate::commands::commands_mod_staging::ThunderstoreInstallIntent::Dependency)),
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

async fn download_selected_package(
    client: &reqwest::Client,
    request: ModDownloadRequest<'_>,
    cancellation: Option<&app_steamcmd::InstallCancellation>,
) -> Result<PathBuf, ThunderstorePreparationFailure> {
    match cancellation {
        Some(cancellation)=>crate::commands::commands_mods::mod_download::download_mod_site_file_with_limit_and_cancellation(client,request,cancellation).await,
        None=>download_mod_site_file_with_limit(client,request).await.map_err(crate::commands::commands_mods::mod_download::ModDownloadFailure::Terminal),
    }.map_err(download_failure)
}

#[cfg(test)]
pub(in crate::commands) async fn prepare_thunderstore_fixture(
    module: &str,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    root: &Path,
    installed: &[crate::commands::commands_mod_staging::CachedThunderstorePackage],
    cancellation: &app_steamcmd::InstallCancellation,
    fixture: &provider_fixture::FixtureProvider,
) -> Result<PreparedThunderstoreInstall, ThunderstorePreparationFailure> {
    prepare_thunderstore_install_inner(
        module,
        source,
        references,
        root,
        PreparationOptions {
            installed,
            cancellation: Some(cancellation),
            existing_runtime_only: true,
            version_policy: None,
            fixture: Some(fixture),
        },
    )
    .await
}

fn package_needs_download(
    installed: &[crate::commands::commands_mod_staging::CachedThunderstorePackage],
    metadata: &ThunderstorePackageMetadata,
) -> bool {
    !installed.iter().any(|package| {
        package.project == metadata.full_name
            && package.version == metadata.latest.version_number
            && package.dependencies == metadata.latest.dependencies
    })
}

#[cfg(test)]
#[path = "commands_thunderstore_start_plan_tests.rs"]
mod startup_tests;

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
