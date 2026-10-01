use super::*;
use app_core::InstanceProgramMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Acquisition {
    LocalPackage,
    Official,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstanceMode {
    Default,
    SecondPrivate,
    SharedPair,
}

#[derive(Debug)]
pub(super) struct CreationMode {
    pub acquisition: Acquisition,
    pub instances: InstanceMode,
}

impl CreationMode {
    pub fn from_env(descriptor: &ModuleDescriptor) -> Result<Self, String> {
        Self::parse(
            std::env::var("LANGAME_NATIVE_ACQUISITION").ok().as_deref(),
            std::env::var("LANGAME_NATIVE_INSTANCE_MODE")
                .ok()
                .as_deref(),
            descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared,
        )
    }

    fn parse(
        acquisition: Option<&str>,
        instances: Option<&str>,
        shared: bool,
    ) -> Result<Self, String> {
        let acquisition = match acquisition {
            None | Some("local_package") => Acquisition::LocalPackage,
            Some("official_acquisition") => Acquisition::Official,
            Some(_) => return Err("unsupported native acquisition mode".into()),
        };
        let instances = match instances {
            None => InstanceMode::Default,
            Some("second_private") => InstanceMode::SecondPrivate,
            Some("shared_pair") if shared => InstanceMode::SharedPair,
            Some("shared_pair") => {
                return Err("native shared_pair requires a shared-capable module".into());
            }
            Some(_) => return Err("unsupported native instance mode".into()),
        };
        if instances == InstanceMode::SecondPrivate && acquisition != Acquisition::Official {
            return Err("native second_private requires LANGAME_NATIVE_ACQUISITION=official_acquisition; an unverified local package cannot seed a clean second installation".into());
        }
        Ok(Self {
            acquisition,
            instances,
        })
    }

    pub fn creation_scope(&self) -> &'static str {
        match self.acquisition {
            Acquisition::LocalPackage => "storage",
            Acquisition::Official => "install_then_desktop_create",
        }
    }

    pub fn acquisition_label(&self) -> &'static str {
        match self.acquisition {
            Acquisition::LocalPackage => "existing_local_package",
            Acquisition::Official => "official_acquisition",
        }
    }

    pub async fn create(
        &self,
        state: tauri::State<'_, DesktopState>,
        storage: &StorageBootstrap,
        descriptor: &ModuleDescriptor,
        package: &package::NativePackage,
        name: &str,
    ) -> Result<InstanceDetails, Box<dyn std::error::Error>> {
        let started = Instant::now();
        if self.acquisition == Acquisition::Official {
            Box::pin(prepare_official_library(
                state.clone(),
                storage,
                descriptor,
                package,
            ))
            .await?;
        }
        let source =
            app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id)
                .await?;
        let source_programs = source
            .as_ref()
            .map(|source| inventory::program_files(descriptor, &source.install_root))
            .transpose()?;
        // A copied local package retains its durable ownership history. Even
        // shared-capable modules must copy a previously exclusive source.
        let source_was_used = source
            .as_ref()
            .map(|source| {
                match fs::symlink_metadata(source.install_root.join(".langame-program-usage.json"))
                {
                    Ok(_) => Ok(true),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                    Err(error) => Err(error),
                }
            })
            .transpose()?
            .unwrap_or(false);
        let input = CreateInstanceInput {
            name: name.into(),
            module_id: descriptor.summary.id.clone(),
        };
        let program_mode = match self.instances {
            InstanceMode::Default => None,
            InstanceMode::SecondPrivate => Some(InstanceProgramMode::Independent),
            InstanceMode::SharedPair => Some(InstanceProgramMode::Shared),
        };
        let first_official = self.acquisition == Acquisition::Official
            && program_mode != Some(InstanceProgramMode::Shared)
            && app_storage::read_module_instance_installs(&storage.paths, &descriptor.summary.id)
                .await?
                .is_empty();
        let first_official_plan = if first_official {
            Some(
                app_storage::inspect_instance_program_creation(
                    &storage.paths,
                    descriptor,
                    program_mode,
                    app_core::InstanceProgramSource::Verified,
                    None,
                )
                .await?,
            )
        } else {
            None
        };
        let provisioning = if self.acquisition == Acquisition::Official {
            // Exercise the production coordinator, including automatic official
            // repair when a previously used source no longer matches its package.
            crate::commands::commands_storage::create_instance_record(state, input, program_mode)
                .await?
        } else {
            app_storage::create_instance_with_options(
                &storage.paths,
                descriptor,
                input,
                app_storage::InstanceCreationOptions {
                    program_mode,
                    ..Default::default()
                },
            )
            .await?
            .provisioning
        };
        let instance = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
        let root = program_root(&instance)?;
        let mode = app_storage::instance_program_mode(instance_root(&instance)?)?;
        let exclusive = app_storage::instance_uses_exclusive_program(instance_root(&instance)?)?;
        if let Some(plan) = first_official_plan
            && exclusive != (plan.action == "existing_install")
        {
            return Err(format!(
                "native first official instance violated source reuse policy: action={} exclusive={exclusive}",
                plan.action,
            ).into());
        }
        let expected_mode = program_mode.unwrap_or_else(|| {
            if exclusive || source_was_used {
                return InstanceProgramMode::Independent;
            }
            if descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared {
                InstanceProgramMode::Shared
            } else {
                InstanceProgramMode::Independent
            }
        });
        if mode != expected_mode {
            return Err(format!(
                "native creation did not use the requested or declared program mode: expected={expected_mode:?} actual={mode:?} source_was_used={source_was_used} exclusive={exclusive}"
            ).into());
        }
        let registered =
            app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
                .await?
                .ok_or("native instance has no program ownership record")?;
        let owner_correct = match mode {
            InstanceProgramMode::Independent => {
                if exclusive {
                    registered.install.scope == app_storage::ProgramInstallScope::Library
                        && registered.install.owner_instance_id.is_none()
                } else {
                    registered.install.scope == app_storage::ProgramInstallScope::Instance
                        && registered.install.owner_instance_id.as_deref()
                            == Some(instance.summary.id.as_str())
                }
            }
            InstanceProgramMode::Shared => {
                registered.install.scope == app_storage::ProgramInstallScope::Library
                    && registered.install.owner_instance_id.is_none()
            }
        };
        if !owner_correct
            || registered.install.module_id != descriptor.summary.id
            || fs::canonicalize(&registered.install.install_root)? != fs::canonicalize(&root)?
        {
            return Err("native program filesystem and database ownership disagree".into());
        }
        let library =
            app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id)
                .await?
                .ok_or("native creation removed its installed library record")?;
        let source = source.ok_or("native creation had no installed library source")?;
        let retained_source =
            app_storage::read_program_install_owner(&storage.paths, &source.install_root)
                .await?
                .ok_or("native creation removed its previous library record")?;
        if !library.install_root.is_dir()
            || retained_source.id != source.id
            || retained_source.current_version != source.current_version
            || retained_source.install_state != source.install_state
            || Some(inventory::program_files(descriptor, &source.install_root)?) != source_programs
        {
            return Err(
                "native creation changed the installed library or its program bytes".into(),
            );
        }
        if library.id != source.id
            && (self.acquisition != Acquisition::Official
                || library.install_state != InstallState::Installed
                || !app_storage::library_program_is_pristine(
                    &library.install_root,
                    descriptor,
                    None,
                )?)
        {
            return Err(
                "native creation did not establish an intact official replacement source".into(),
            );
        }
        let fixture_root = fs::canonicalize(&storage.paths.games_root)?;
        let canonical_root = fs::canonicalize(&root)?;
        let relative_root = canonical_root
            .strip_prefix(&fixture_root)
            .map_err(|_| "native instance program escaped the disposable fixture")?;
        if exclusive
            && (fs::canonicalize(&root)? != fs::canonicalize(&library.install_root)?
                || instance_root(&instance)?
                    .join("runtime")
                    .join(
                        descriptor
                            .process
                            .as_ref()
                            .ok_or("missing native process")?
                            .executable
                            .as_str(),
                    )
                    .exists())
        {
            return Err("native first instance copied or moved its library program".into());
        }
        let preparation = match mode {
            InstanceProgramMode::Shared => "shared_reference",
            InstanceProgramMode::Independent if exclusive => "existing_installation",
            InstanceProgramMode::Independent => "isolated_copy",
        };
        println!(
            "NATIVE_LIFECYCLE module={} phase=instance_created elapsed_ms={} program_mode={mode:?} preparation={preparation} library=preserved ownership=verified effective_root=fixture/{} creation_scope={}",
            descriptor.summary.id,
            started.elapsed().as_millis(),
            relative_root.display(),
            self.creation_scope()
        );
        Ok(instance)
    }

    pub fn verify_pair(
        &self,
        first: &InstanceDetails,
        second: &InstanceDetails,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first_root = program_root(first)?;
        let second_root = program_root(second)?;
        if first.config_file_path == second.config_file_path
            || first.saves_path == second.saves_path
        {
            return Err("native pair shares instance configuration or save paths".into());
        }
        if first.ports.iter().any(|a| {
            second
                .ports
                .iter()
                .any(|b| a.protocol == b.protocol && a.port == b.port)
        }) {
            return Err("native pair has conflicting allocated ports".into());
        }
        match self.instances {
            InstanceMode::SharedPair if first_root != second_root => {
                return Err("native shared pair did not reuse one program root".into());
            }
            InstanceMode::SecondPrivate if first_root == second_root => {
                return Err("native independent pair shares its program root".into());
            }
            _ => {}
        }
        let expected = if self.instances == InstanceMode::SharedPair {
            InstanceProgramMode::Shared
        } else {
            InstanceProgramMode::Independent
        };
        for instance in [first, second] {
            if app_storage::instance_program_mode(instance_root(instance)?)? != expected {
                return Err("native pair did not receive the requested program mode".into());
            }
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=pair_created program_mode={expected:?} config_saves_ports=isolated peer=unstarted simultaneous_running=not_tested",
            second.summary.module_id
        );
        Ok(())
    }
}

async fn prepare_official_library(
    state: tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    package: &package::NativePackage,
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = fs::canonicalize(&package.root)?;
    if fs::canonicalize(&storage.paths.games_root)? != fixture
        || !package.cleanup_allowed.load(Ordering::SeqCst)
    {
        return Err("native installation requires its settled disposable fixture".into());
    }
    let registered =
        app_storage::read_library_program_install(&storage.paths, &descriptor.summary.id).await?;
    if let Some(registered) = &registered {
        let referenced =
            app_storage::read_module_instance_installs(&storage.paths, &descriptor.summary.id)
                .await?
                .iter()
                .any(|record| record.install.id == registered.id);
        let previously_used = registered
            .install_root
            .join(".langame-program-usage.json")
            .try_exists()?;
        if referenced || previously_used {
            // This is the first instance's actual program, not a spare mother
            // copy. The production creator must repair into its own new root;
            // the native fixture must never replace or download over these files.
            seed_probe::checked_path(&package.root, &registered.install_root, true)?;
            println!(
                "NATIVE_LIFECYCLE module={} phase=official_install_retained instance_used=true acquisition=not_started creation_validation=pending",
                descriptor.summary.id
            );
            return Ok(());
        }
    }
    if let Some(registered) = &registered
        && registered.install_root.try_exists()?
    {
        let root = seed_probe::checked_path(&package.root, &registered.install_root, true)?;
        if official_library_is_pristine(root, descriptor).await? {
            println!(
                "NATIVE_LIFECYCLE module={} phase=official_install_reused official_pristine=true",
                descriptor.summary.id
            );
            return Ok(());
        }
    }

    // The install command resolves missing installations to the module's default
    // directory. Preserve this fixture's unknown copy before creating an empty
    // default target; a NotInstalled registration cannot redirect that command.
    let install = descriptor
        .install
        .as_ref()
        .ok_or("native module has no installation")?;
    let target = package.root.join(&install.shared_game_dir);
    if target.parent() != Some(package.root.as_path()) {
        return Err("native official library must be a direct child of its fixture".into());
    }
    if target.try_exists()? {
        seed_probe::checked_path(&package.root, &target, true)?;
        if !crate::commands::commands_program_storage::shared_program_references(
            &storage.paths,
            &descriptor.summary.id,
            &target,
        )
        .await?
        .is_empty()
        {
            return Err(
                "native official installation refuses to relocate a shared instance program".into(),
            );
        }
        let retained = create_retained_package_root(&package.root)?;
        fs::rename(&target, retained.join("package"))?;
    }
    fs::create_dir(&target)?;
    let mut records = Vec::new();
    if let Some(registered) = registered
        && registered.install_root != target
    {
        records.push(app_storage::GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: registered.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::NotInstalled,
            current_version: None,
            mark_verified: false,
        });
    }
    records.push(app_storage::GameInstallSyncRecord {
        module_id: descriptor.summary.id.clone(),
        install_root: target.to_string_lossy().into_owned(),
        install_state: InstallState::NotInstalled,
        current_version: None,
        mark_verified: false,
    });
    app_storage::sync_game_installs(&storage.paths, &records).await?;
    if app_storage::resolve_module_install_root(&storage.paths, &descriptor.summary.id)
        .await?
        .is_some()
    {
        return Err(
            "native official installation has an unexpected installed library override".into(),
        );
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=official_install_started target=owned_empty_library",
        descriptor.summary.id
    );
    package.cleanup_allowed.store(false, Ordering::SeqCst);
    if install.source != Some(app_core::InstallSource::MinecraftJava)
        && install.download_url_windows.is_none()
        && descriptor.summary.steam_app_id.is_some_and(|id| id > 0)
    {
        app_steamcmd::ensure_steamcmd_installed(&storage.settings).await?;
    }
    let result = Box::pin(install_module_game(state, descriptor.summary.id.clone())).await?;
    // Only successful installer completion establishes that its children have
    // settled. On failure the package owner retains the complete fixture.
    package.cleanup_allowed.store(true, Ordering::SeqCst);
    if result.module_id != descriptor.summary.id
        || result.install_state != InstallState::Installed
        || !result.executable_exists
        || fs::canonicalize(&result.install_root)? != fs::canonicalize(&target)?
        || !official_library_is_pristine(target, descriptor).await?
    {
        return Err("native explicit installation did not establish an intact official package in its owned library".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={} phase=official_install_completed official_pristine=true installer=settled",
        descriptor.summary.id
    );
    Ok(())
}

async fn official_library_is_pristine(
    root: PathBuf,
    descriptor: &ModuleDescriptor,
) -> Result<bool, Box<dyn std::error::Error>> {
    let descriptor = descriptor.clone();
    Ok(tokio::task::spawn_blocking(move || {
        app_storage::library_program_is_pristine(&root, &descriptor, None)
    })
    .await??)
}

fn create_retained_package_root(fixture: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Two creations are sufficient for the private-pair probe. Short bounded
    // candidates leave room for native DLL paths and never reuse a collision.
    for index in 0..8 {
        let target = fixture.join(format!("p{index}"));
        match fs::create_dir(&target) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("native official installation has no unused short library directory".into())
}

pub(super) fn instance_root(instance: &InstanceDetails) -> Result<&Path, String> {
    Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "native instance config has no managed root".into())
}

pub(super) fn program_root(
    instance: &InstanceDetails,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(app_storage::resolve_instance_runtime_root(instance_root(
        instance,
    )?)?)
}

#[test]
fn native_creation_requires_official_provenance_for_second_private() {
    assert!(
        CreationMode::parse(None, Some("second_private"), false)
            .unwrap_err()
            .contains("official_acquisition")
    );
    assert_eq!(
        CreationMode::parse(Some("official_acquisition"), Some("second_private"), false)
            .unwrap()
            .creation_scope(),
        "install_then_desktop_create"
    );
}

#[test]
fn native_official_installation_reserves_roots_without_touching_earlier_copies() {
    let fixture = std::env::temp_dir().join(format!("native-official-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&fixture).unwrap();
    fs::create_dir(fixture.join("p0")).unwrap();
    fs::write(fixture.join("p0/program.bin"), b"unknown original package").unwrap();
    fs::write(fixture.join("p1"), b"existing unrelated file").unwrap();

    let first = create_retained_package_root(&fixture).unwrap();
    assert_eq!(first, fixture.join("p2"));
    assert!(fs::read_dir(&first).unwrap().next().is_none());
    fs::write(first.join("program.bin"), b"polluted first instance").unwrap();
    let second = create_retained_package_root(&fixture).unwrap();
    assert_eq!(second, fixture.join("p3"));
    assert!(fs::read_dir(second).unwrap().next().is_none());
    assert_eq!(
        fs::read(first.join("program.bin")).unwrap(),
        b"polluted first instance"
    );
    assert_eq!(
        fs::read(fixture.join("p0/program.bin")).unwrap(),
        b"unknown original package"
    );
    assert_eq!(
        fs::read(fixture.join("p1")).unwrap(),
        b"existing unrelated file"
    );

    for index in 4..8 {
        fs::create_dir(fixture.join(format!("p{index}"))).unwrap();
    }
    assert!(create_retained_package_root(&fixture).is_err());
    assert_eq!(fs::read_dir(&fixture).unwrap().count(), 8);
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn native_creation_shared_pair_requires_declared_capability() {
    assert!(CreationMode::parse(None, Some("shared_pair"), false).is_err());
    assert!(CreationMode::parse(None, Some("shared_pair"), true).is_ok());
    assert_eq!(
        CreationMode::parse(None, None, false)
            .unwrap()
            .creation_scope(),
        "storage"
    );
}
