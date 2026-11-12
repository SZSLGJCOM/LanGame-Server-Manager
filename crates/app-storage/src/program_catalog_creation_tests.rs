#![cfg(test)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

use app_core::{CreateInstanceInput, InstallState, InstanceProgramMode, InstanceProgramSource};
use app_modules::{ModuleDescriptor, ModuleProgramSharing};

use crate::{
    CreateInstanceResult, GameInstallSyncRecord, InstanceCreationOptions, ProgramInstallScope,
    StoragePaths, bootstrap_storage_with_paths, create_instance_with_options, initialize_database,
    inspect_module_programs, read_instance_details, read_instance_program_install,
    record_library_program_baseline, sync_game_installs, sync_modules,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const PAYLOAD: &str = "catalog-program-payload.bin";
const PAYLOAD_BYTES: &[u8] = b"synthetic package content, never executable";

fn catalog() -> &'static [ModuleDescriptor] {
    static CATALOG: OnceLock<Vec<ModuleDescriptor>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        app_modules::discover_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
            .expect("load the real module catalog")
    })
}

struct Fixture {
    directory: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
    source: PathBuf,
}

impl Fixture {
    async fn new(module_id: &str) -> TestResult<Self> {
        let descriptor = catalog()
            .iter()
            .find(|module| module.summary.id == module_id)
            .expect("the matrix names a real module")
            .clone();
        let directory =
            std::env::temp_dir().join(format!("pc-{:016x}", uuid::Uuid::new_v4().as_u128() as u64));
        fs::create_dir(&directory)?;
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let paths = bootstrap_storage_with_paths(StoragePaths {
            app_data_root: directory.join("app-data"),
            settings_path: directory.join("app-data/settings.json"),
            database_path: directory.join("app-data/db/lgs.db"),
            logs_root: directory.join("logs"),
            modules_root: repository.join("modules"),
            migrations_root: repository.join("migrations"),
            steamcmd_root: directory.join("steamcmd"),
            games_root: directory.join("games"),
            instances_root: directory.join("instances"),
            archives_root: directory.join("instances/.trash"),
        })?
        .paths;
        let install = descriptor
            .install
            .as_ref()
            .expect("catalog installation contract");
        let process = descriptor
            .process
            .as_ref()
            .expect("catalog process contract");
        let source = paths.games_root.join(&install.shared_game_dir);
        let verification = install
            .verification_path
            .as_deref()
            .unwrap_or(&process.executable);
        assert!(
            !verification.contains("{{"),
            "{module_id}: concrete package verification path"
        );
        // These files exercise real module layout, templates, storage ownership
        // and copy rules. No game binary is downloaded or run by this matrix.
        write_file(&source.join(verification), PAYLOAD_BYTES)?;
        if !process.executable.contains("{{") && !process.executable.ends_with(".bat") {
            write_file(&source.join(&process.executable), PAYLOAD_BYTES)?;
        }
        if module_id == "necesse" {
            write_file(&source.join("Server.jar"), PAYLOAD_BYTES)?;
        }
        if module_id == "barotrauma" {
            for relative in ["Content", "Data"] {
                fs::create_dir_all(source.join(relative))?;
            }
        }
        write_file(&source.join(PAYLOAD), PAYLOAD_BYTES)?;
        initialize_database(&paths).await?;
        sync_modules(&paths, std::slice::from_ref(&descriptor)).await?;
        Ok(Self {
            directory,
            paths,
            descriptor,
            source,
        })
    }

    async fn record(&self) -> TestResult {
        record_library_program_baseline(&self.source, &self.descriptor, true, None)?;
        sync_game_installs(
            &self.paths,
            &[GameInstallSyncRecord {
                module_id: self.descriptor.summary.id.clone(),
                install_root: self.source.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("catalog-layout-fixture".into()),
                mark_verified: true,
            }],
        )
        .await?;
        Ok(())
    }

    async fn inventory(
        &self,
        mode: InstanceProgramMode,
    ) -> TestResult<crate::ModuleProgramInventory> {
        Ok(inspect_module_programs(
            &self.paths,
            &self.descriptor,
            Some(mode),
            InstanceProgramSource::Verified,
            Arc::new(AtomicBool::new(false)),
            false,
        )
        .await?)
    }

    async fn create(
        &self,
        mode: InstanceProgramMode,
        label: &str,
    ) -> TestResult<CreateInstanceResult> {
        Ok(create_instance_with_options(
            &self.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: format!("{} {label}", self.descriptor.summary.id),
                module_id: self.descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                prefer_existing_install: true,
                require_clean_program: true,
                program_mode: Some(mode),
                ..Default::default()
            },
        )
        .await?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(
            self.directory.parent(),
            Some(std::env::temp_dir().as_path())
        );
        fs::remove_dir_all(&self.directory).expect("remove only the owned catalog fixture");
    }
}

fn write_file(path: &Path, contents: &[u8]) -> TestResult {
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, contents)?;
    Ok(())
}

async fn verify_first_and_second_instance(module_id: &str, independent: bool) -> TestResult {
    let fixture = Fixture::new(module_id).await?;
    fixture.record().await?;
    assert_eq!(
        crate::program_launch_files_match_baseline(&fixture.source, &fixture.descriptor, None)?,
        Some(true),
        "{module_id}: an intact official layout must pass launch preparation"
    );
    if fixture.descriptor.summary.steam_app_id.is_some() {
        fs::create_dir_all(fixture.source.join("steamapps/downloading"))?;
        fs::create_dir_all(fixture.source.join("steamapps/temp/empty"))?;
    }
    let shared =
        !independent && fixture.descriptor.storage.program_sharing == ModuleProgramSharing::Shared;
    let mode = if shared {
        InstanceProgramMode::Shared
    } else {
        InstanceProgramMode::Independent
    };
    let inventory = fixture.inventory(mode).await?;
    assert!(
        inventory.creation.can_create,
        "{module_id}: {:?}",
        inventory.creation.reason
    );
    assert_eq!(
        inventory.creation.action,
        if shared {
            "shared_install"
        } else {
            "existing_install"
        }
    );
    assert_eq!(
        inventory.creation.additional_bytes,
        Some(0),
        "first instance must not copy a clean program"
    );
    let first = fixture.create(mode, "first").await?;
    assert_eq!(
        fs::canonicalize(&first.effective_install_root)?,
        fs::canonicalize(&fixture.source)?
    );
    let first_id = &first.provisioning.summary.id;
    assert!(
        !fixture
            .paths
            .instances_root
            .join(first_id)
            .join("runtime")
            .join(PAYLOAD)
            .exists()
    );
    let first_binding = read_instance_program_install(&fixture.paths, first_id)
        .await?
        .unwrap();
    assert_eq!(first_binding.install.scope, ProgramInstallScope::Library);
    let first_details = read_instance_details(&fixture.paths, first_id).await?;
    let first_config = fs::read(&first.provisioning.config_file_path)?;
    let first_save = Path::new(&first_details.saves_path).join("catalog-user-world.sentinel");
    write_file(&first_save, b"first user's world")?;
    let first_mod = fixture
        .source
        .join("catalog-user-mod/only-first-instance.bin");
    if !shared {
        write_file(&first_mod, b"first user's Mod")?;
    }

    let next = fixture.inventory(mode).await?;
    assert!(
        next.creation.can_create,
        "{module_id}: second instance unavailable"
    );
    assert_eq!(
        next.creation.action,
        if shared {
            "shared_install"
        } else {
            "independent_install"
        }
    );
    if shared {
        assert_eq!(next.creation.additional_bytes, Some(0));
    } else {
        assert!(
            next.creation
                .additional_bytes
                .is_some_and(|bytes| bytes > 0)
        );
    }
    let second = fixture.create(mode, "second").await?;
    let second_binding =
        read_instance_program_install(&fixture.paths, &second.provisioning.summary.id)
            .await?
            .unwrap();
    let second_details =
        read_instance_details(&fixture.paths, &second.provisioning.summary.id).await?;
    if shared {
        assert_eq!(second_binding.install.id, first_binding.install.id);
        assert_eq!(second_binding.install.scope, ProgramInstallScope::Library);
    } else {
        assert_ne!(
            fs::canonicalize(&second.effective_install_root)?,
            fs::canonicalize(&fixture.source)?
        );
        assert_eq!(second_binding.install.scope, ProgramInstallScope::Instance);
        assert_eq!(
            second_binding.install.owner_instance_id.as_deref(),
            Some(second.provisioning.summary.id.as_str())
        );
        assert!(
            !second
                .effective_install_root
                .join("catalog-user-mod/only-first-instance.bin")
                .exists()
        );
        assert_eq!(fs::read(first_mod)?, b"first user's Mod");
    }
    assert_ne!(first_details.saves_path, second_details.saves_path);
    assert!(
        !Path::new(&second_details.saves_path)
            .join("catalog-user-world.sentinel")
            .exists()
    );
    assert_eq!(fs::read(&first_save)?, b"first user's world");
    assert_eq!(
        fs::read(&first.provisioning.config_file_path)?,
        first_config
    );
    assert_eq!(fs::read(fixture.source.join(PAYLOAD))?, PAYLOAD_BYTES);
    assert_eq!(
        fs::read(second.effective_install_root.join(PAYLOAD))?,
        PAYLOAD_BYTES
    );
    Ok(())
}

macro_rules! catalog_cases {
    ($($module:ident),+ $(,)?) => {
        const COVERED_MODULES: &[&str] = &[$(stringify!($module)),+];
        $(#[tokio::test]
        async fn $module() -> TestResult {
            verify_first_and_second_instance(stringify!($module), false).await
        })+
    };
}

catalog_cases!(
    abioticfactor,
    arksurvivalascended,
    arksurvivalevolved,
    astroneer,
    barotrauma,
    conanexiles,
    corekeeper,
    dontstarve,
    enshrouded,
    humanitz,
    minecraft,
    necesse,
    nightingale,
    palworld,
    projectzomboid,
    returntomoria,
    rimworld,
    romestead,
    runescapedragonwilds,
    rust,
    satisfactory,
    scum,
    sevendaystodie,
    sonsoftheforest,
    soulmask,
    squad,
    terraria,
    theforest,
    unturned,
    valheim,
    vrising,
    windrose
);

#[test]
fn matrix_covers_the_current_catalog() {
    let expected: BTreeSet<_> = catalog()
        .iter()
        .map(|module| module.summary.id.as_str())
        .collect();
    assert_eq!(expected, COVERED_MODULES.iter().copied().collect());
}

#[tokio::test]
async fn minecraft_explicit_independent_first_instance_reuses_without_sharing() -> TestResult {
    verify_first_and_second_instance("minecraft", true).await
}

#[tokio::test]
async fn certified_defaults_under_each_catalog_copy_exclusion_allow_only_clean_first_reuse()
-> TestResult {
    let mut covered = Vec::new();
    for descriptor in catalog() {
        let Some(exclusion) = descriptor
            .storage
            .runtime_copy_exclusions
            .iter()
            .chain(&descriptor.storage.retained_paths)
            .find(|path| {
                !path
                    .split(['/', '\\'])
                    .any(|part| part.starts_with(".langame-"))
            })
        else {
            continue;
        };
        let fixture = Fixture::new(&descriptor.summary.id).await?;
        let path = Path::new(exclusion);
        let relative = if path.extension().is_some() {
            path.to_owned()
        } else {
            path.join("catalog-shipped-default.txt")
        };
        // This is a certified package-layout default, not a claim that the live
        // publisher ships this test file. It exercises the real exclusion rules.
        let default = fixture.source.join(&relative);
        write_file(&default, b"shipped default one")?;
        fixture.record().await?;
        let clean = fixture.inventory(InstanceProgramMode::Independent).await?;
        assert_eq!(
            clean.creation.action,
            "existing_install",
            "{}: {}",
            descriptor.summary.id,
            relative.display()
        );
        assert_eq!(clean.creation.additional_bytes, Some(0));
        fs::write(&default, b"operator edited two")?;
        let changed = fixture.inventory(InstanceProgramMode::Independent).await?;
        assert_eq!(
            changed.creation.action, "independent_install",
            "{}: edited defaults cannot be inherited",
            descriptor.summary.id
        );
        assert_eq!(fs::read(&default)?, b"operator edited two");
        covered.push(descriptor.summary.id.clone());
    }
    assert!(!covered.is_empty());
    eprintln!("CATALOG_COPY_EXCLUSION_DEFAULTS {}", covered.join(","));
    Ok(())
}
