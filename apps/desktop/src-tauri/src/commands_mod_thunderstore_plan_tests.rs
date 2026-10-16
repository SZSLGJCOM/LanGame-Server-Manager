#![cfg(test)]
use super::*;

fn metadata(name: &str, version: &str, dependencies: &[&str]) -> ThunderstorePackageMetadata {
    serde_json::from_value(serde_json::json!({
        "full_name": name, "community_listings": [{"community":"valheim"}],
        "latest": {"version_number":version, "download_url":"https://thunderstore.io/package/download/Fixture/Plugin/1.0.0/", "dependencies":dependencies}
    })).unwrap()
}

#[tokio::test]
async fn valheim_plan_fetches_transitive_exact_dependencies_but_provides_loader_separately() {
    let mut requested = Vec::new();
    let packages = resolve_packages(
        thunderstore_game_profile("valheim").unwrap(),
        &["https://thunderstore.io/c/valheim/p/Fixture/Plugin/".into()],
        |package| {
            requested.push(package.clone());
            std::future::ready(Ok(match package.package.as_str() {
                "Plugin" => metadata(
                    "Fixture-Plugin",
                    "2.0.0",
                    &[
                        "Fixture-Library-1.2.3",
                        "denikson-BepInExPack_Valheim-5.4.2202",
                    ],
                ),
                "Library" => metadata("Fixture-Library", "1.2.3", &["Fixture-Core-1.0.0"]),
                "Core" => metadata("Fixture-Core", "1.0.0", &[]),
                _ => panic!("Unexpected package request"),
            }))
        },
    )
    .await
    .unwrap();
    assert_eq!(packages.len(), 3);
    assert_eq!(
        requested
            .iter()
            .map(|item| item.package.as_str())
            .collect::<Vec<_>>(),
        ["Plugin", "Library", "Core"]
    );
    assert_eq!(requested[1].version.as_deref(), Some("1.2.3"));
    assert_eq!(requested[2].version.as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn valheim_plan_deduplicates_cycles_and_rejects_conflicting_versions() {
    let link = "https://thunderstore.io/c/valheim/p/Fixture/Plugin/".to_string();
    let mut calls = 0;
    let packages = resolve_packages(
        thunderstore_game_profile("valheim").unwrap(),
        std::slice::from_ref(&link),
        |package| {
            calls += 1;
            std::future::ready(Ok(if package.package == "Plugin" {
                metadata("Fixture-Plugin", "1.0.0", &["Fixture-Library-1.0.0"])
            } else {
                metadata("Fixture-Library", "1.0.0", &["Fixture-Plugin-1.0.0"])
            }))
        },
    )
    .await
    .unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(calls, 2);
    let result = resolve_packages(
        thunderstore_game_profile("valheim").unwrap(),
        &[link],
        |package| {
            std::future::ready(Ok(if package.package == "Plugin" {
                metadata(
                    "Fixture-Plugin",
                    "1.0.0",
                    &["Fixture-Library-1.0.0", "Fixture-Library-2.0.0"],
                )
            } else {
                metadata("Fixture-Library", "1.0.0", &[])
            }))
        },
    )
    .await;
    assert!(result.unwrap_err().contains("conflicting"));
}

#[tokio::test]
async fn valheim_plan_rejects_wrong_community_identity_and_requested_version() {
    for variant in ["community", "identity", "version"] {
        let error = resolve_packages(
            thunderstore_game_profile("valheim").unwrap(),
            &["https://thunderstore.io/p/Fixture/Plugin/1.0.0/".into()],
            |_| {
                let mut item = metadata("Fixture-Plugin", "1.0.0", &[]);
                match variant {
                    "community" => item.community_listings[0].community = "v-rising".into(),
                    "identity" => item.full_name = "Other-Plugin".into(),
                    _ => item.latest.version_number = "2.0.0".into(),
                }
                std::future::ready(Ok(item))
            },
        )
        .await
        .unwrap_err();
        assert!(!error.is_empty());
    }
}

#[tokio::test]
async fn valheim_plan_bounds_depth_and_rejects_malformed_or_unsupported_loader_dependencies() {
    let link = "https://thunderstore.io/p/Fixture/Plugin/".to_string();
    for dependency in [
        "bad",
        "Fixture-Plugin-../outside",
        "denikson-BepInExPack_Valheim-5.4.9999",
    ] {
        assert!(
            resolve_packages(
                thunderstore_game_profile("valheim").unwrap(),
                std::slice::from_ref(&link),
                |_| { std::future::ready(Ok(metadata("Fixture-Plugin", "1.0.0", &[dependency]))) }
            )
            .await
            .is_err()
        );
    }
    let error = resolve_packages(
        thunderstore_game_profile("valheim").unwrap(),
        &[link],
        |package| {
            let depth = package
                .package
                .strip_prefix("Node")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let name = format!("{}-{}", package.namespace, package.package);
            std::future::ready(Ok(metadata(
                &name,
                "1.0.0",
                &[&format!("Fixture-Node{}-1.0.0", depth + 1)],
            )))
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("limit"));
    let loader = thunderstore_game_profile("valheim")
        .unwrap()
        .loader
        .unwrap();
    assert!(loader.supports_dependency(&loader.project, "5.4.2100"));
    assert!(!loader.supports_dependency(&loader.project, "6.0.0"));
    assert!(!loader.supports_dependency("Other-BepInEx", &loader.version));
}

async fn real_package_installation_acceptance(module_id: &str, reference: &str, project: &str) {
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let profile = thunderstore_game_profile(module_id).unwrap();
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "langame-thunderstore-mod-chain-{}",
        uuid::Uuid::new_v4()
    )));
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(
        fixture.0.join(profile.executable),
        b"MZisolated-runtime-fixture",
    )
    .unwrap();
    let source = app_core::ModuleModSourceSpec {
        provider: "thunderstore".into(),
        label: "Thunderstore".into(),
        url: format!("https://thunderstore.io/c/{}/", profile.community),
        install_note: None,
        loaders: Vec::new(),
        game_versions: Vec::new(),
    };
    let prepared = tokio::time::timeout(
        Duration::from_secs(900),
        prepare_thunderstore_install(module_id, &source, &[reference.into()], &fixture.0),
    )
    .await
    .unwrap()
    .unwrap();
    let target = ResolvedManualModTarget {
        instance_id: format!("{module_id}-chain"),
        module_id: module_id.into(),
        source_label: "Thunderstore".into(),
        target_label: profile.target_relative.into(),
        target_path: fixture.0.join(profile.target_relative),
        accepts: vec!["dll".into(), "folder".into(), "zip".into()],
        id_strategy: None,
    };
    let runtime = fixture.0.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::commands::commands_mod_staging::stage_thunderstore_online_mod_sources(
            target,
            prepared.sources(),
            prepared.loader_archive(),
        )
    })
    .await
    .unwrap()
    .unwrap();
    validate_runtime(
        module_id,
        "thunderstore",
        &runtime,
        &runtime.join(profile.target_relative),
    )
    .unwrap();
    assert!(
        runtime
            .join(profile.target_relative)
            .join(format!("thunderstore-{project}"))
            .is_dir()
    );
    assert!(!runtime_loader_requires_archive(module_id, &runtime).unwrap());
    assert!(result.copied_file_count > 0);
}

#[tokio::test]
#[ignore = "Downloads real Thunderstore packages into an isolated temporary runtime; does not execute the game"]
async fn valheim_thunderstore_real_package_installation_acceptance() {
    real_package_installation_acceptance(
        "valheim",
        "https://thunderstore.io/c/valheim/p/ValheimModding/Jotunn/",
        "ValheimModding-Jotunn",
    )
    .await;
}

#[tokio::test]
#[ignore = "Downloads real V Rising Thunderstore packages into an isolated temporary runtime; does not execute the game"]
async fn vrising_thunderstore_real_package_installation_acceptance() {
    real_package_installation_acceptance(
        "vrising",
        "https://thunderstore.io/c/v-rising/p/deca/VampireCommandFramework/",
        "deca-VampireCommandFramework",
    )
    .await;
}

#[tokio::test]
async fn thunderstore_plan_uses_one_dependency_flow_for_all_reviewed_games() {
    for module_id in ["valheim", "vrising", "corekeeper"] {
        let profile = thunderstore_game_profile(module_id).unwrap();
        let loader_dependency = profile
            .loader
            .map(|loader| format!("{}-{}", loader.project, loader.version));
        let mut requested = Vec::new();
        let packages = resolve_packages(
            profile,
            &[format!(
                "https://thunderstore.io/c/{}/p/Fixture/Plugin/",
                profile.community
            )],
            |package| {
                requested.push(package.clone());
                let mut item = if package.package == "Plugin" {
                    let mut dependencies = vec!["Fixture-Library-1.2.3"];
                    if let Some(dependency) = loader_dependency.as_deref() {
                        dependencies.push(dependency);
                    }
                    metadata("Fixture-Plugin", "2.0.0", &dependencies)
                } else {
                    metadata("Fixture-Library", "1.2.3", &[])
                };
                item.community_listings[0].community = profile.community.into();
                std::future::ready(Ok(item))
            },
        )
        .await
        .unwrap();
        assert_eq!(packages.len(), 2, "{module_id}");
        assert_eq!(
            requested
                .iter()
                .map(|item| item.package.as_str())
                .collect::<Vec<_>>(),
            ["Plugin", "Library"]
        );
        assert_eq!(requested[1].version.as_deref(), Some("1.2.3"));
    }
    assert!(
        thunderstore_game_profile("corekeeper")
            .unwrap()
            .loader
            .is_none()
    );
}

#[tokio::test]
async fn thunderstore_loader_support_is_bound_to_the_selected_game() {
    for (module_id, other_id) in [("valheim", "vrising"), ("vrising", "valheim")] {
        let profile = thunderstore_game_profile(module_id).unwrap();
        let loader = profile.loader.unwrap();
        let other = thunderstore_game_profile(other_id).unwrap();
        let other_loader = other.loader.unwrap();
        assert!(loader.supports_dependency(&loader.project, &loader.version));
        assert!(!loader.supports_dependency(&other_loader.project, &other_loader.version));
        let (namespace, package) = loader.project.split_once('-').unwrap();
        let mut calls = 0;
        let packages = resolve_packages(
            profile,
            &[format!(
                "https://thunderstore.io/c/{}/p/{namespace}/{package}/",
                profile.community
            )],
            |_| {
                calls += 1;
                std::future::ready(Err::<ThunderstorePackageMetadata, String>(
                    "loader must not become a plugin".into(),
                ))
            },
        )
        .await
        .unwrap();
        assert!(packages.is_empty());
        assert_eq!(calls, 0);
        let (namespace, package) = other_loader.project.split_once('-').unwrap();
        let error = resolve_packages(
            profile,
            &[format!(
                "https://thunderstore.io/c/{}/p/{namespace}/{package}/",
                other.community
            )],
            |_| {
                let mut item = metadata(&other_loader.project, &other_loader.version, &[]);
                item.community_listings[0].community = other.community.into();
                std::future::ready(Ok(item))
            },
        )
        .await
        .unwrap_err();
        assert!(error.contains("mod_community_mismatch"));
    }
}

#[tokio::test]
async fn thunderstore_prepare_rejects_wrong_source_before_network_or_runtime_changes() {
    let source = app_core::ModuleModSourceSpec {
        provider: "thunderstore".into(),
        label: "Thunderstore".into(),
        url: "https://thunderstore.io/c/valheim/".into(),
        install_note: None,
        loaders: Vec::new(),
        game_versions: Vec::new(),
    };
    let error = prepare_thunderstore_install("vrising", &source, &[], Path::new("missing-runtime"))
        .await
        .err()
        .expect("source community must be bound to the module");
    assert!(error.contains("v-rising"));
}

#[tokio::test]
async fn thunderstore_native_corekeeper_rejects_old_bepinex_before_download() {
    let error = resolve_packages(
        thunderstore_game_profile("corekeeper").unwrap(),
        &["https://thunderstore.io/c/core-keeper/p/BepInEx/BepInExPack_Core_Keeper/".into()],
        |_| std::future::ready(Err("metadata fetch must not be reached".into())),
    )
    .await
    .unwrap_err();
    assert!(error.contains("old BepInEx"), "{error}");
}
