use super::*;

const MAX_ONLINE_PACKAGES: usize = 32;

pub(super) fn validate_runtime(
    module_id: &str,
    provider: &str,
    install_root: &Path,
    target_path: &Path,
) -> Result<(), String> {
    mod_inventory::validate_root_chain(install_root)?;
    mod_inventory::validate_root_chain(target_path)?;
    match provider {
        "modrinth" => Err(runtime_error("minecraft_loader", "Automatic Modrinth installation requires a verified loader and Minecraft version for this instance. Configure a compatible server loader, then import matching local JAR files.")),
        "thunderstore" if matches!(module_id, "valheim" | "vrising") => {
            if target_path != install_root.join("BepInEx/plugins") {
                return Err("The Thunderstore target does not match this instance's BepInEx runtime".into());
            }
            verify_bepinex(install_root).map_err(|error| runtime_error("bepinex", &format!(
                "BepInEx is not verified in this instance's server runtime. {error}. Install and enable the game's compatible BepInEx loader in this instance before installing plugins."
            )))
        }
        "thunderstore" if module_id == "corekeeper" => {
            if target_path != install_root.join("CoreKeeperServer_Data/StreamingAssets/Mods") {
                return Err("The Thunderstore target does not match this instance's Core Keeper runtime".into());
            }
            require_binary(&install_root.join("CoreKeeperServer.exe"))?;
            let data = install_root.join("CoreKeeperServer_Data/StreamingAssets");
            mod_inventory::validate_root_chain(&data)?;
            if !fs::symlink_metadata(&data).map_err(|error| format!("Core Keeper runtime is not verified: {error}"))?.is_dir() {
                return Err("Core Keeper's native Mod loading directory is not available".into());
            }
            Ok(())
        }
        "thunderstore" => Err("This module has no verified Thunderstore server loader; use local package import after configuring its runtime".into()),
        _ => Ok(()),
    }
}

fn runtime_error(reason: &str, message: &str) -> String {
    serde_json::json!({"code": "mod_runtime_unverified", "reason": reason, "message": message})
        .to_string()
}

fn require_binary(path: &Path) -> Result<(), String> {
    mod_inventory::validate_root_chain(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() < 2 {
        return Err(format!(
            "{} is not a nonempty runtime binary",
            path.display()
        ));
    }
    let mut header = [0u8; 2];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if header != *b"MZ" {
        return Err(format!(
            "{} is not a Windows runtime binary",
            path.display()
        ));
    }
    Ok(())
}

fn verify_bepinex(root: &Path) -> Result<(), String> {
    require_binary(&root.join("winhttp.dll"))?;
    let config_path = root.join("doorstop_config.ini");
    mod_inventory::validate_root_chain(&config_path)?;
    let metadata = fs::symlink_metadata(&config_path)
        .map_err(|error| format!("cannot inspect doorstop_config.ini: {error}"))?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err("invalid doorstop_config.ini".into());
    }
    let mut config = String::new();
    fs::File::open(&config_path)
        .and_then(|file| file.take(64 * 1024 + 1).read_to_string(&mut config))
        .map_err(|error| format!("cannot read doorstop_config.ini: {error}"))?;
    if config.len() > 64 * 1024 {
        return Err("doorstop_config.ini exceeds its size limit".into());
    }
    let assembly = doorstop_assembly(&config)?;
    require_binary(&root.join(assembly))
}

fn doorstop_assembly(config: &str) -> Result<PathBuf, String> {
    let mut relevant = false;
    let mut enabled = None;
    let mut assembly = None;
    for line in config
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';']))
    {
        if line.starts_with('[') && line.ends_with(']') {
            relevant = matches!(
                line.to_ascii_lowercase().as_str(),
                "[unitydoorstop]" | "[general]"
            );
            continue;
        }
        if !relevant {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "enabled" => {
                if enabled
                    .replace(value.eq_ignore_ascii_case("true"))
                    .is_some()
                {
                    return Err("ambiguous Doorstop enablement".into());
                }
            }
            "targetassembly" | "target_assembly"
                if assembly.replace(value.to_string()).is_some() =>
            {
                return Err("ambiguous Doorstop target assembly".into());
            }
            _ => {}
        }
    }
    if enabled != Some(true) {
        return Err("Doorstop is disabled or has no explicit enabled=true setting".into());
    }
    let value = assembly
        .ok_or("Doorstop has no target assembly")?
        .replace('\\', "/");
    let normalized = value.to_ascii_lowercase();
    let parts = normalized.split('/').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0] != "bepinex"
        || parts[1] != "core"
        || !parts[2].ends_with(".dll")
        || parts[2].contains(':')
    {
        return Err("Doorstop does not target a BepInEx/core loader assembly".into());
    }
    Ok(PathBuf::from(value))
}

pub(super) async fn download_thunderstore_mod_sources(
    client: &reqwest::Client,
    source: &app_core::ModuleModSourceSpec,
    references: &[String],
    target: &Path,
) -> Result<Vec<DownloadedManualModSource>, String> {
    if references.len() > MAX_ONLINE_PACKAGES {
        return Err("An online installation supports at most 32 packages".into());
    }
    let community = thunderstore_community(&source.url)?;
    let mut packages = Vec::new();
    let mut seen = HashMap::new();
    for reference in references {
        let package = extract_thunderstore_package_reference(reference)
            .ok_or_else(|| format!("Thunderstore link did not identify a package: {reference}"))?;
        let mut metadata =
            fetch_thunderstore_package_metadata(client, &package.namespace, &package.package)
                .await?;
        if let Some(version) = package.version.as_deref() {
            metadata.latest = fetch_thunderstore_version(client, &package, version).await?;
        }
        validate_thunderstore_metadata(
            &metadata,
            &community,
            &format!("{}-{}", package.namespace, package.package),
        )?;
        if let Some(version) = seen.get(&metadata.full_name) {
            if version != &metadata.latest.version_number {
                return Err(
                    "The online request selects conflicting versions of one Thunderstore package"
                        .into(),
                );
            }
        } else {
            seen.insert(
                metadata.full_name.clone(),
                metadata.latest.version_number.clone(),
            );
            packages.push(metadata);
        }
    }
    let identities = packages
        .iter()
        .map(|metadata| OnlineModIdentity {
            provider: "thunderstore".into(),
            project: metadata.full_name.clone(),
            historical_names: vec![metadata.full_name.clone()],
            version: metadata.latest.version_number.clone(),
            dependencies: metadata.latest.dependencies.clone(),
        })
        .collect::<Vec<_>>();
    let verification_target = target.to_path_buf();
    let verification_identities = identities.clone();
    tokio::task::spawn_blocking(move || {
        verify_online_mod_dependencies(&verification_target, &verification_identities)
    })
    .await
    .map_err(|error| format!("Mod dependency inspection worker failed: {error}"))??;
    let mut sources = DownloadedManualModSourcesCleanup::default();
    for (metadata, identity) in packages.into_iter().zip(identities) {
        let archive_path = download_mod_site_archive(
            client,
            &metadata.latest.download_url,
            &metadata.full_name,
            &metadata.latest.version_number,
        )
        .await?;
        let path = prepare_downloaded_thunderstore_archive(
            archive_path,
            &metadata.full_name,
            &metadata.latest.version_number,
        )
        .await?;
        sources.push(DownloadedManualModSource {
            path,
            identity,
            label: metadata.full_name,
        });
    }
    Ok(sources.into_sources())
}

fn thunderstore_community(url: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(url)
        .map_err(|_| "The module has no valid Thunderstore community URL")?;
    let parts = url
        .path_segments()
        .ok_or("The module has no Thunderstore community")?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 || parts[0] != "c" {
        return Err("The module has no explicit Thunderstore community".into());
    }
    Ok(parts[1].to_string())
}

fn validate_thunderstore_metadata(
    metadata: &ThunderstorePackageMetadata,
    community: &str,
    expected: &str,
) -> Result<(), String> {
    if metadata.full_name != expected
        || metadata.full_name.len() > 257
        || !valid_thunderstore_version(&metadata.latest.version_number)
    {
        return Err("Thunderstore returned mismatched package identity or version".into());
    }
    if metadata.latest.dependencies.len() > 128 {
        return Err("Thunderstore dependency graph exceeds its 128-package limit".into());
    }
    if !metadata
        .community_listings
        .iter()
        .any(|listing| listing.community == community)
    {
        return Err(serde_json::json!({
            "code": "mod_community_mismatch", "community": community, "package": metadata.full_name,
            "message": format!("Thunderstore package does not belong to this game's community: {} ({community})", metadata.full_name),
        }).to_string());
    }
    Ok(())
}

pub(super) fn valid_thunderstore_version(version: &str) -> bool {
    version.len() <= 32
        && version.split('.').count() == 3
        && version.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}

async fn fetch_thunderstore_version(
    client: &reqwest::Client,
    package: &ThunderstorePackageReference,
    version: &str,
) -> Result<ThunderstorePackageVersion, String> {
    let url = format!(
        "https://thunderstore.io/api/experimental/package/{}/{}/{version}/",
        package.namespace, package.package
    );
    let request = client.get(url).build().map_err(|error| error.to_string())?;
    let response = app_network::read_public_bytes(
        client,
        request,
        Duration::from_secs(12),
        512 * 1024,
        app_network::SourcePreference::InternationalFirst,
    )
    .await
    .map_err(|error| format!("failed to resolve the selected Thunderstore version: {error}"))?;
    let selected: ThunderstorePackageVersion = serde_json::from_slice(&response.bytes)
        .map_err(|error| format!("invalid Thunderstore version metadata: {error}"))?;
    if selected.version_number != version {
        return Err(
            "Thunderstore returned a different version from the explicitly selected package".into(),
        );
    }
    Ok(selected)
}

// Root loaders require a different installation transaction from a plugin payload.
pub(super) fn reject_loader_archive(path: &Path) -> Result<(), String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot inspect Thunderstore archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("invalid Thunderstore archive: {error}"))?;
    if archive.len() > 20_000 {
        return Err("Thunderstore archive exceeds its entry limit".into());
    }
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect Thunderstore archive entry: {error}"))?;
        let name = entry.name().replace('\\', "/").to_ascii_lowercase();
        if name.ends_with("/doorstop_config.ini")
            || name == "doorstop_config.ini"
            || name.ends_with("/winhttp.dll")
            || name == "winhttp.dll"
            || name.contains("bepinex/core/")
        {
            return Err("This Thunderstore package contains a server loader. Install it separately in this instance's server root; it cannot be installed as a plugin".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_mod_online_tests.rs"]
mod tests;
