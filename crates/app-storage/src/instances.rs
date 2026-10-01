use super::*;
use crate::atomic_file::read_optional_file_to_string;
use crate::instance_creation_io::check_creation_cancelled;
#[cfg(test)]
use crate::instance_creation_io::{copy_creation_file, publish_creation_directory};
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instance_native_settings::{MoriaPermissionsSnapshot, reject_active_sensitive_changes};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
    acquire_module_instance_creation_lock_blocking,
    acquire_module_instance_creation_lock_cancellable,
};
use crate::player_access_normalization::normalize_module_player_access_settings_strict;
#[cfg(test)]
use crate::private_runtime::PRIVATE_RUNTIME_MARKER;
use crate::private_runtime::{
    is_reparse_point, path_starts_with, prepare_private_runtime_projection,
};
#[cfg(test)]
use crate::private_runtime_refresh::PackageTree;
use crate::program_adoption::{ADOPTION_JOURNAL, ProgramAdoption};
use crate::program_runtime::{InstanceProgramMode, prepare_shared_program_reference};
use crate::runtime::load_active_instance_run;
use crate::save_paths::{
    InstanceSavePathContext, effective_instance_saves_dir, load_module_descriptor,
    materialized_instance_saves_dir, module_declares_saves_path, planned_instance_saves_dir,
};
use crate::settings_validation::{
    SettingsValidationPhase, normalize_dontstarve_operational_settings,
    validate_settings_against_schema,
};
use crate::storage_db::{
    connect_pool, fetch_instance_record, load_instance_ports, load_instances,
    load_module_instance_records, resolve_installed_game_install_id_from_pool,
    resolve_module_install_root_from_executor, resolve_module_install_root_from_pool,
};
use crate::templates::{
    PreparedWorkshopConfiguration, prepare_workshop_configuration_in_worker,
    write_pending_instance_configuration_in_worker,
};
#[path = "instance_configuration_packages.rs"]
mod configuration_packages;
use crate::templates::{
    InstanceConfigInput, ManagedConfigMutation, ModuleSupportMaterializationContext,
    ModuleTemplateRenderInput, SchemaDefaultContext, apply_module_prestart_support,
    collect_schema_defaults_from_schema_json, write_pending_instance_configuration,
};
use sqlx::QueryBuilder;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub const MAX_INSTANCE_PORT_PROJECTION_INSTANCES: usize = 128;
const MAX_INSTANCE_PORT_PROJECTION_ROWS: usize = 4_096;
const DST_LAN_DISCOVERY_PORT_MIN: u16 = 10_998;
const DST_LAN_DISCOVERY_PORT_MAX: u16 = 11_018;

#[derive(Debug, Clone)]
pub struct InstancePortProjection {
    pub instance_id: String,
    pub module_id: String,
    pub bind_ip: String,
    pub ports: Vec<PortBinding>,
}

pub async fn list_instances(paths: &StoragePaths) -> Result<Vec<InstanceSummary>, StorageError> {
    let pool = connect_pool(paths).await?;
    let instances = load_instances(&pool).await?;
    pool.close().await;
    Ok(instances)
}

pub async fn read_instance_port_projections(
    paths: &StoragePaths,
    instance_ids: &[String],
) -> Result<Vec<InstancePortProjection>, StorageError> {
    if instance_ids.len() > MAX_INSTANCE_PORT_PROJECTION_INSTANCES {
        return Err(StorageError::InstancePortProjectionLimitExceeded {
            max: MAX_INSTANCE_PORT_PROJECTION_INSTANCES,
            actual: instance_ids.len(),
        });
    }
    if instance_ids.is_empty() {
        return Ok(Vec::new());
    }

    let requested_ids = instance_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let pool = connect_pool(paths).await?;
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT instances.id AS instance_id, instances.module_id, instances.bind_ip, \
         instance_ports.name, instance_ports.protocol, instance_ports.port \
         FROM instances LEFT JOIN instance_ports ON instance_ports.instance_id = instances.id \
         WHERE instances.id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for instance_id in &requested_ids {
            separated.push_bind(instance_id);
        }
    }
    query.push(" ) ORDER BY instances.id ASC, instance_ports.id ASC LIMIT ");
    query.push_bind(i64::try_from(MAX_INSTANCE_PORT_PROJECTION_ROWS + 1).unwrap_or(i64::MAX));
    let rows_result = query.build().fetch_all(&pool).await;
    pool.close().await;
    let rows = rows_result?;
    if rows.len() > MAX_INSTANCE_PORT_PROJECTION_ROWS {
        return Err(StorageError::InstancePortProjectionRowLimitExceeded {
            max: MAX_INSTANCE_PORT_PROJECTION_ROWS,
        });
    }

    let mut projections = std::collections::BTreeMap::<String, InstancePortProjection>::new();
    for row in rows {
        let instance_id = row.try_get::<String, _>("instance_id")?;
        let projection = projections
            .entry(instance_id.clone())
            .or_insert(InstancePortProjection {
                instance_id: instance_id.clone(),
                module_id: row.try_get("module_id")?,
                bind_ip: row.try_get("bind_ip")?,
                ports: Vec::new(),
            });
        if let Some(stored_port) = row.try_get::<Option<i64>, _>("port")? {
            let name = row.try_get::<String, _>("name")?;
            let port = u16::try_from(stored_port).map_err(|_| {
                StorageError::InvalidStoredInstancePort {
                    instance_id: instance_id.clone(),
                    name: name.clone(),
                    port: stored_port,
                }
            })?;
            projection.ports.push(PortBinding {
                name,
                protocol: row.try_get("protocol")?,
                port,
            });
        }
    }

    for instance_id in requested_ids {
        if !projections.contains_key(instance_id) {
            return Err(StorageError::MissingInstance {
                id: instance_id.clone(),
            });
        }
    }
    Ok(projections.into_values().collect())
}

#[derive(Debug, Clone)]
pub struct CreateInstanceResult {
    pub provisioning: InstanceProvisioning,
    pub effective_install_root: PathBuf,
}

pub async fn read_instance_details(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceDetails, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        let ports = load_instance_ports(&pool, instance_id).await?;
        let active_run = load_active_instance_run(&pool, instance_id).await?;
        let config_file_path = record.config_dir.join("instance.json");
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let mut settings = parse_settings_object(&read_instance_settings_json(&config_file_path)?)?;
        normalize_module_player_access_settings_strict(&record.summary.module_id, &mut settings)?;
        crate::templates::dst_world_settings::project(descriptor.as_ref(), &mut settings)?;
        let install_root = effective_instance_install_root(&record)?;
        if let Some(snapshot) =
            MoriaPermissionsSnapshot::read(&record.summary.module_id, &install_root)?
        {
            snapshot.project(&mut settings);
        }
        let settings_json = serde_json::to_string_pretty(&Value::Object(settings))?;
        let backup_uses_declared_saves_path = module_declares_saves_path(descriptor.as_ref());
        let saves_path = effective_instance_saves_dir(descriptor.as_ref(), &install_root, &record)?;

        Ok(InstanceDetails {
            summary: record.summary,
            config_file_path: config_file_path.to_string_lossy().into_owned(),
            saves_path: saves_path.to_string_lossy().into_owned(),
            backup_uses_declared_saves_path,
            auto_backup_on_stop: record.auto_backup_on_stop,
            backup_retention_count: record.backup_retention_count,
            settings_json,
            ports,
            active_run,
        })
    }
    .await;
    // Dropping a pool does not wait for SQLite's worker to release Windows file handles.
    // Close on both success and error before callers can remove or replace the database.
    pool.close().await;
    result
}

pub async fn create_instance(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    input: CreateInstanceInput,
) -> Result<InstanceProvisioning, StorageError> {
    Ok(
        create_instance_with_options(paths, descriptor, input, InstanceCreationOptions::default())
            .await?
            .provisioning,
    )
}

pub async fn create_instance_with_options(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    input: CreateInstanceInput,
    options: InstanceCreationOptions,
) -> Result<CreateInstanceResult, StorageError> {
    let module_creation_lock = acquire_module_instance_creation_lock(
        paths,
        &input.module_id,
        options.cancellation.clone(),
    )
    .await?;
    let paths = paths.clone();
    let descriptor = descriptor.clone();
    let transaction_lock = module_creation_lock.clone();
    module_creation_lock
        .complete_mutation("creating instance", async move {
            create_instance_transaction(&paths, &descriptor, input, options, &transaction_lock)
                .await
        })
        .await
}

async fn create_instance_transaction(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    input: CreateInstanceInput,
    options: InstanceCreationOptions,
    module_creation_lock: &InstanceSettingsLock,
) -> Result<CreateInstanceResult, StorageError> {
    check_creation_cancelled(options.cancellation.as_deref())?;
    let pool = connect_pool(paths).await?;
    let result = create_instance_in_pool(
        paths,
        descriptor,
        input,
        options,
        module_creation_lock,
        &pool,
    )
    .await;
    pool.close().await;
    result
}

async fn create_instance_in_pool(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    input: CreateInstanceInput,
    options: InstanceCreationOptions,
    module_creation_lock: &InstanceSettingsLock,
    pool: &SqlitePool,
) -> Result<CreateInstanceResult, StorageError> {
    check_creation_cancelled(options.cancellation.as_deref())?;

    let instance_id = new_instance_id(&input.name);
    let root = paths.instances_root.join(&instance_id);
    let data_path = root.join("data");
    let config_path = root.join("config");
    let logs_path = root.join("logs");

    let (install_id, shared_install_root) =
        if let Some(selected) = options.program_install_root.as_deref() {
            let root = crate::instance_isolation::paths::normalize_path(selected)?;
            let owner = crate::read_program_install_owner(paths, &root)
                .await?
                .ok_or_else(|| {
                    crate::program_runtime::invalid(
                        &root,
                        "selected program source is not a registered library installation",
                    )
                })?;
            if owner.scope != crate::ProgramInstallScope::Library
                || owner.owner_instance_id.is_some()
                || owner.module_id != input.module_id
                || descriptor.summary.id != input.module_id
                || owner.install_state != InstallState::Installed
                || !root.is_dir()
            {
                return Err(crate::program_runtime::invalid(
                    &root,
                    "selected program source must be an installed library owned by this module",
                ));
            }
            (Some(owner.id), root)
        } else {
            let id = resolve_installed_game_install_id_from_pool(pool, &input.module_id).await?;
            let root = resolve_module_install_root_from_pool(pool, &input.module_id)
                .await?
                .map(PathBuf::from)
                .or_else(|| {
                    descriptor
                        .install
                        .as_ref()
                        .map(|install| paths.games_root.join(&install.shared_game_dir))
                })
                .unwrap_or_else(|| paths.games_root.join(&input.module_id));
            (id, root)
        };
    let existing_instances = load_module_instance_records(pool, &input.module_id).await?;
    let save_descriptor = descriptor.clone();
    let existing_save_paths = module_creation_lock
        .spawn_blocking(move || {
            existing_instances
                .iter()
                .map(|record| {
                    // Resolve the current declaration and settings, as backup/deletion do.
                    // The stored path may predate a corrected module save boundary.
                    let install_root = effective_instance_install_root(record)?;
                    effective_instance_saves_dir(Some(&save_descriptor), &install_root, record)
                })
                .collect::<Result<Vec<_>, StorageError>>()
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "resolving existing instance save boundaries",
            message: error.to_string(),
        })??;
    let mut settings = merge_schema_defaults_with_settings(
        Some(descriptor),
        Map::new(),
        SchemaDefaultContext {
            instance_id: Some(&instance_id),
            instance_name: Some(&input.name),
        },
    )?;
    settings.insert(
        String::from("bind_ip"),
        Value::String(String::from(INSTANCE_CREATION_DEFAULT_BIND_IP)),
    );
    if descriptor.summary.id == "humanitz" {
        crate::player_access_normalization::validate_humanitz_roster_update(
            &Map::new(),
            &settings,
        )?;
    }
    validate_settings_against_schema(
        Some(descriptor),
        &settings,
        SettingsValidationPhase::Creation,
    )?;
    let mut excluded_paths = existing_save_paths
        .into_iter()
        .filter(|path| path_starts_with(path, &shared_install_root))
        .collect::<Vec<_>>();
    if let Some(template) = descriptor
        .storage
        .saves_path_template
        .as_deref()
        .filter(|template| template.contains("paths.install_root"))
    {
        excluded_paths.push(
            install_save_directory_prefix(template, &shared_install_root)
                .unwrap_or_else(|| shared_install_root.clone()),
        );
    }
    descriptor
        .storage
        .validate_runtime_copy_exclusions()
        .map_err(|message| StorageError::InvalidPrivateRuntimeProjection {
            path: shared_install_root.clone(),
            message,
        })?;
    excluded_paths.extend(
        descriptor
            .storage
            .runtime_copy_exclusions
            .iter()
            .map(|relative| shared_install_root.join(relative)),
    );
    // A user may place managed instances below a game installation. Never
    // traverse the directory being created while copying the shared package.
    if path_starts_with(&paths.instances_root, &shared_install_root) {
        excluded_paths.push(paths.instances_root.clone());
    }
    excluded_paths.push(shared_install_root.join("steamapps").join("workshop"));
    if descriptor.summary.id == "dontstarve" {
        excluded_paths.extend(dst_workshop_mod_paths(&shared_install_root)?);
    }
    let library_references: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE install_id = ?1")
            .bind(install_id)
            .fetch_one(pool)
            .await?;
    let exclusive_references: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instances WHERE install_id = ?1 AND runtime_mode = 'independent'",
    )
    .bind(install_id)
    .fetch_one(pool)
    .await?;
    if options.use_local_program
        && (library_references != 0
            || crate::program_exclusive::library_was_exclusively_used(&shared_install_root)?)
    {
        return Err(crate::program_runtime::invalid(
            &shared_install_root,
            "this installation contains managed instance data; use a verified independent program source instead of importing its files",
        ));
    }
    let take_program_ownership = options.take_program_ownership;
    if take_program_ownership
        && (!options.require_clean_program
            || options.use_local_program
            || options.private_runtime.is_some()
            || options.program_mode == Some(InstanceProgramMode::Shared)
            || install_id.is_none()
            || library_references != 0
            || !crate::is_instance_program_acquisition(
                paths,
                &input.module_id,
                &shared_install_root,
            )?)
    {
        return Err(crate::program_runtime::invalid(
            &shared_install_root,
            "only an unused managed acquisition can become an independent instance program",
        ));
    }
    let use_existing_install = if !take_program_ownership
        && options.prefer_existing_install
        && options.require_clean_program
        && !options.use_local_program
        && options.private_runtime.is_none()
        && install_id.is_some()
        && library_references == 0
        && options.program_mode != Some(InstanceProgramMode::Shared)
    {
        crate::program_exclusive::unused_library_can_be_reused(
            paths,
            &shared_install_root,
            &input.module_id,
            options.cancellation.clone(),
        )
        .await?
    } else {
        false
    };
    let share_program = !take_program_ownership
        && !use_existing_install
        && exclusive_references == 0
        && !crate::program_exclusive::library_was_exclusively_used(&shared_install_root)?
        && options.private_runtime.is_none()
        && descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared
        && options.program_mode != Some(InstanceProgramMode::Independent);
    if options.use_local_program
        && (share_program || options.require_clean_program || options.private_runtime.is_some())
    {
        return Err(crate::program_runtime::invalid(
            &shared_install_root,
            "local program import requires an independent instance and explicit local selection",
        ));
    }
    if options.program_mode == Some(InstanceProgramMode::Shared) && !share_program {
        return Err(crate::program_runtime::invalid(
            &shared_install_root,
            "this module does not support shared program files",
        ));
    }
    let runtime_preparation = if take_program_ownership {
        InstanceRuntimePreparation::Adopt {
            excluded_paths,
            exclude_dst_workshop_mods: descriptor.summary.id == "dontstarve",
        }
    } else if use_existing_install {
        InstanceRuntimePreparation::Existing {
            module_id: input.module_id.clone(),
        }
    } else if share_program {
        if install_id.is_none() {
            return Err(crate::program_runtime::invalid(
                &shared_install_root,
                "a verified registered library installation is required for program sharing",
            ));
        }
        InstanceRuntimePreparation::Shared {
            module_id: input.module_id.clone(),
        }
    } else if let Some(projection) = options.private_runtime {
        InstanceRuntimePreparation::Projection {
            projection,
            source_generation: options.source_generation,
        }
    } else {
        InstanceRuntimePreparation::Copy {
            excluded_paths,
            source_generation: options.source_generation,
            exclude_dst_workshop_mods: descriptor.summary.id == "dontstarve",
        }
    };
    let (pending_directory, template_install_root) = prepare_pending_instance_directory(
        paths.instances_root.clone(),
        root.clone(),
        shared_install_root.clone(),
        runtime_preparation,
        options
            .require_clean_program
            .then(|| input.module_id.clone()),
        options.use_local_program,
        options.cancellation.clone(),
    )
    .await?;
    let save_paths_result = (|| {
        let save_path_context = InstanceSavePathContext {
            install_root: &template_install_root,
            instance_root: &root,
            config_dir: &config_path,
            instance_id: &instance_id,
            instance_name: &input.name,
            module_id: &input.module_id,
            settings: Some(&settings),
        };
        Ok::<_, StorageError>((
            planned_instance_saves_dir(Some(descriptor), &save_path_context)?,
            materialized_instance_saves_dir(Some(descriptor), &save_path_context)?,
        ))
    })();
    let (saves_path, materialized_saves_path) = match save_paths_result {
        Ok(paths) => paths,
        Err(error) => {
            let error = rollback_pending_instance_directory(pending_directory, error).await;
            return Err(error);
        }
    };
    let config_file_path = config_path.join("instance.json");
    let creation_result = async {
        // Preparation is cancellable; once admitted, the native files and
        // database must reach commit or compensation under the same lease.
        check_creation_cancelled(options.cancellation.as_deref())?;
        let mut admission = pool.begin().await?;
        ensure_instance_paths_available(
            paths,
            &mut admission,
            &instance_id,
            &input.module_id,
            &template_install_root,
            &config_path,
            &saves_path,
        )
        .await?;
        admission.rollback().await?;
        let prepared_packages = prepare_workshop_configuration_in_worker(
            &ModuleSupportMaterializationContext {
                storage_paths: paths,
                module_id: &input.module_id,
                install_root: &template_install_root,
                shared_install_root: &shared_install_root,
                config_dir: &config_path,
                saves_dir: &saves_path,
                instance_id: &instance_id,
                instance_running: false,
                settings: &settings,
            },
            module_creation_lock,
        )
        .await?;
        check_creation_cancelled(options.cancellation.as_deref())?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_instance_paths_available(
            paths,
            &mut tx,
            &instance_id,
            &input.module_id,
            &template_install_root,
            &config_path,
            &saves_path,
        )
        .await?;

        sqlx::query(
            r#"
            INSERT INTO instances (
                id, name, module_id, bind_ip, install_id, status,
                data_path, config_path, logs_path, saves_path, autostart, runtime_mode
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )
        .bind(&instance_id)
        .bind(&input.name)
        .bind(&input.module_id)
        .bind(INSTANCE_CREATION_DEFAULT_BIND_IP)
        .bind(install_id)
        .bind("stopped")
        .bind(data_path.to_string_lossy().into_owned())
        .bind(config_path.to_string_lossy().into_owned())
        .bind(logs_path.to_string_lossy().into_owned())
        .bind(saves_path.to_string_lossy().into_owned())
        .bind(if INSTANCE_CREATION_DEFAULT_AUTOSTART {
            1_i64
        } else {
            0_i64
        })
        .bind(if share_program {
            "shared"
        } else {
            "independent"
        })
        .execute(&mut *tx)
        .await?;

        if take_program_ownership {
            crate::program_install_records::adopt_library_install_for_instance(
                &mut tx,
                &input.module_id,
                install_id.ok_or_else(|| {
                    crate::program_runtime::invalid(
                        &shared_install_root,
                        "instance acquisition registration is missing",
                    )
                })?,
                &instance_id,
                &template_install_root,
            )
            .await?;
        } else if use_existing_install {
            crate::program_install_records::bind_exclusive_install_to_instance(
                &mut tx,
                &input.module_id,
                install_id.ok_or_else(|| {
                    crate::program_runtime::invalid(
                        &shared_install_root,
                        "library installation is missing",
                    )
                })?,
                &instance_id,
            )
            .await?;
        } else if share_program {
            let library_id = install_id.ok_or_else(|| {
                crate::program_runtime::invalid(
                    &shared_install_root,
                    "shared library registration is missing",
                )
            })?;
            crate::bind_shared_install_to_instance(
                &mut tx,
                &input.module_id,
                library_id,
                &instance_id,
            )
            .await?;
        } else {
            let current_version: Option<String> = sqlx::query_scalar(
                "SELECT current_version FROM game_installs WHERE id = ?1 AND scope = 'library'",
            )
            .bind(install_id)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
            crate::register_instance_install(
                &mut tx,
                &input.module_id,
                &instance_id,
                &template_install_root,
                InstallState::Installed,
                current_version.as_deref(),
            )
            .await?;
        }

        let ports = allocate_default_ports(
            &mut tx,
            &instance_id,
            &descriptor.summary.id,
            &descriptor.default_ports,
            &descriptor.runtime.port_groups,
        )
        .await?;

        if settings.contains_key("public_port")
            && let Some(game_port) = ports.iter().find(|binding| binding.name == "game")
        {
            settings.insert(
                String::from("public_port"),
                Value::Number(serde_json::Number::from(u64::from(game_port.port))),
            );
        }

        let config_mutation = materialize_new_instance_files(NewInstanceFilesystemPlan {
            paths: paths.clone(),
            templates_root: descriptor.root.join("templates"),
            data_path: data_path.clone(),
            config_path: config_path.clone(),
            logs_path: logs_path.clone(),
            saves_path: saves_path.clone(),
            materialized_saves_path: materialized_saves_path.clone(),
            install_root: template_install_root.clone(),
            shared_install_root: shared_install_root.clone(),
            instance_id: instance_id.clone(),
            instance_name: input.name.clone(),
            module_id: input.module_id.clone(),
            settings: settings.clone(),
            ports: ports.clone(),
            prepared_packages,
        })
        .await?;
        commit_instance_transaction(tx, config_mutation, module_creation_lock).await?;

        Ok::<_, StorageError>(CreateInstanceResult {
            provisioning: InstanceProvisioning {
                summary: InstanceSummary {
                    id: instance_id.clone(),
                    name: input.name.clone(),
                    module_id: input.module_id.clone(),
                    status: InstanceStatus::Stopped,
                    active_process_count: 0,
                    bind_ip: String::from(INSTANCE_CREATION_DEFAULT_BIND_IP),
                    port_count: ports.len(),
                    autostart: INSTANCE_CREATION_DEFAULT_AUTOSTART,
                },
                config_file_path: config_file_path.to_string_lossy().into_owned(),
                ports,
            },
            effective_install_root: template_install_root.clone(),
        })
    }
    .await;

    match creation_result {
        Ok(result) => {
            pending_directory.commit();
            Ok(result)
        }
        Err(error) => {
            let error = rollback_pending_instance_directory(pending_directory, error).await;
            Err(error)
        }
    }
}

async fn acquire_module_instance_creation_lock(
    paths: &StoragePaths,
    module_id: &str,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<InstanceSettingsLock, StorageError> {
    let paths = paths.clone();
    let module_id = module_id.to_owned();
    tokio::task::spawn_blocking(move || match cancellation.as_deref() {
        Some(cancellation) => acquire_module_instance_creation_lock_cancellable(
            &paths,
            &module_id,
            Some(cancellation),
        ),
        None => acquire_module_instance_creation_lock_blocking(&paths, &module_id),
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "acquiring the module instance creation lock",
        message: error.to_string(),
    })?
}

#[derive(Debug)]
enum InstanceRuntimePreparation {
    Adopt {
        excluded_paths: Vec<PathBuf>,
        exclude_dst_workshop_mods: bool,
    },
    Existing {
        module_id: String,
    },
    Copy {
        excluded_paths: Vec<PathBuf>,
        source_generation: Option<String>,
        exclude_dst_workshop_mods: bool,
    },
    Projection {
        projection: PrivateRuntimeProjection,
        source_generation: Option<String>,
    },
    Shared {
        module_id: String,
    },
}

async fn prepare_pending_instance_directory(
    instances_root: PathBuf,
    instance_root: PathBuf,
    shared_install_root: PathBuf,
    runtime_preparation: InstanceRuntimePreparation,
    required_clean_module: Option<String>,
    use_local_program: bool,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<(PendingInstanceDirectory, PathBuf), StorageError> {
    tokio::task::spawn_blocking(move || {
        check_creation_cancelled(cancellation.as_deref())?;
        let selection = if use_local_program {
            crate::program_runtime::ProgramFileSelection::Local
        } else if let Some(module_id) = required_clean_module.as_deref() {
            crate::program_runtime::ProgramFileSelection::Verified(module_id)
        } else {
            crate::program_runtime::ProgramFileSelection::Automatic
        };
        let mut pending_directory =
            PendingInstanceDirectory::create(instances_root, instance_root.clone())?;
        let verify_clean_program = || {
            if let Some(module_id) = required_clean_module.as_deref() {
                crate::program_seed::require_clean_package_tree(
                    &shared_install_root,
                    module_id,
                    cancellation.as_deref(),
                )?;
            }
            Ok::<(), StorageError>(())
        };
        let install_root = match runtime_preparation {
            InstanceRuntimePreparation::Adopt {
                excluded_paths,
                exclude_dst_workshop_mods,
            } => ProgramAdoption::begin(
                &shared_install_root,
                &instance_root,
                &excluded_paths,
                exclude_dst_workshop_mods,
                selection,
                cancellation.as_deref(),
            )
            .and_then(|adoption| {
                let adoption = adoption.ok_or_else(|| {
                    crate::program_runtime::invalid(
                        &shared_install_root,
                        "instance acquisition must be on the instance volume",
                    )
                })?;
                let runtime = adoption.runtime_root();
                pending_directory.adoption = Some(adoption);
                Ok(runtime)
            }),
            InstanceRuntimePreparation::Existing { module_id } => {
                crate::program_exclusive::prepare_exclusive_reference(
                    &shared_install_root,
                    &instance_root,
                    &module_id,
                    cancellation.as_deref(),
                )
            }
            InstanceRuntimePreparation::Copy {
                excluded_paths,
                source_generation,
                exclude_dst_workshop_mods,
            } => crate::program_adoption::validate_library_copy_source(
                &shared_install_root,
                &instance_root,
                &excluded_paths,
                exclude_dst_workshop_mods,
                selection,
                cancellation.as_deref(),
            )
            .and_then(|()| {
                prepare_private_runtime_root(
                    &shared_install_root,
                    &instance_root,
                    &excluded_paths,
                    source_generation.as_deref(),
                    exclude_dst_workshop_mods,
                    selection,
                    cancellation.as_deref(),
                )
            }),
            InstanceRuntimePreparation::Projection {
                projection,
                source_generation,
            } => verify_clean_program().and_then(|()| {
                prepare_private_runtime_projection(
                    &shared_install_root,
                    &instance_root,
                    &projection,
                    source_generation.as_deref(),
                    cancellation.as_deref(),
                )
            }),
            InstanceRuntimePreparation::Shared { module_id } => {
                verify_clean_program().and_then(|()| {
                    check_creation_cancelled(cancellation.as_deref())?;
                    prepare_shared_program_reference(
                        &shared_install_root,
                        &instance_root,
                        &module_id,
                    )
                })
            }
        };
        match install_root {
            Ok(install_root) => Ok((pending_directory, install_root)),
            Err(error) => Err(rollback_pending_instance_directory_blocking(
                pending_directory,
                error,
            )),
        }
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "preparing the instance runtime directory",
        message: error.to_string(),
    })?
}

struct NewInstanceFilesystemPlan {
    paths: StoragePaths,
    templates_root: PathBuf,
    data_path: PathBuf,
    config_path: PathBuf,
    logs_path: PathBuf,
    saves_path: PathBuf,
    materialized_saves_path: PathBuf,
    install_root: PathBuf,
    shared_install_root: PathBuf,
    instance_id: String,
    instance_name: String,
    module_id: String,
    settings: Map<String, Value>,
    ports: Vec<PortBinding>,
    prepared_packages: Option<PreparedWorkshopConfiguration>,
}

async fn materialize_new_instance_files(
    plan: NewInstanceFilesystemPlan,
) -> Result<ManagedConfigMutation, StorageError> {
    tokio::task::spawn_blocking(move || {
        for path in [
            &plan.data_path,
            &plan.config_path,
            &plan.logs_path,
            &plan.materialized_saves_path,
        ] {
            fs::create_dir_all(path).map_err(|source| StorageError::CreatePath {
                path: path.to_path_buf(),
                source,
            })?;
        }

        write_pending_instance_configuration(
            &plan.templates_root,
            &ModuleTemplateRenderInput {
                config_dir: &plan.config_path,
                install_root: &plan.install_root,
                saves_dir: &plan.saves_path,
                instance_id: &plan.instance_id,
                instance_name: &plan.instance_name,
                module_id: &plan.module_id,
                bind_ip: INSTANCE_CREATION_DEFAULT_BIND_IP,
                autostart: INSTANCE_CREATION_DEFAULT_AUTOSTART,
                settings: &plan.settings,
                ports: &plan.ports,
            },
            &ModuleSupportMaterializationContext {
                storage_paths: &plan.paths,
                module_id: &plan.module_id,
                install_root: &plan.install_root,
                shared_install_root: &plan.shared_install_root,
                config_dir: &plan.config_path,
                saves_dir: &plan.saves_path,
                instance_id: &plan.instance_id,
                instance_running: false,
                settings: &plan.settings,
            },
            &plan.config_path.join("instance.json"),
            InstanceConfigInput {
                instance_id: &plan.instance_id,
                instance_name: &plan.instance_name,
                module_id: &plan.module_id,
                bind_ip: INSTANCE_CREATION_DEFAULT_BIND_IP,
                autostart: INSTANCE_CREATION_DEFAULT_AUTOSTART,
                settings: plan.settings.clone(),
                ports: &plan.ports,
            },
            plan.prepared_packages,
        )
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "materializing new instance files",
        message: error.to_string(),
    })?
}

#[derive(Debug)]
struct PendingInstanceDirectory {
    instances_root: PathBuf,
    instance_root: PathBuf,
    armed: bool,
    adoption: Option<ProgramAdoption>,
}

impl PendingInstanceDirectory {
    fn create(instances_root: PathBuf, instance_root: PathBuf) -> Result<Self, StorageError> {
        validate_managed_instance_root(&instance_root, &instances_root)?;
        fs::create_dir(&instance_root).map_err(|source| StorageError::CreatePath {
            path: instance_root.clone(),
            source,
        })?;
        Ok(Self {
            instances_root,
            instance_root,
            armed: true,
            adoption: None,
        })
    }

    fn commit(mut self) {
        self.armed = false;
        if let Some(adoption) = &self.adoption
            && let Err(error) = adoption.commit()
        {
            eprintln!("committed program adoption left a recovery journal: {error}");
        }
    }

    fn rollback(mut self) -> Result<(), StorageError> {
        if let Some(adoption) = &self.adoption {
            // A failed restore leaves the journal and every file for recovery.
            self.armed = false;
            adoption.rollback()?;
        }
        let result = cleanup_owned_instance_directory(&self.instance_root, &self.instances_root);
        self.armed = false;
        result
    }
}

impl Drop for PendingInstanceDirectory {
    fn drop(&mut self) {
        if self.armed {
            if let Some(adoption) = &self.adoption
                && adoption.rollback().is_err()
            {
                return;
            }
            let _ = cleanup_owned_instance_directory(&self.instance_root, &self.instances_root);
        }
    }
}

async fn rollback_pending_instance_directory(
    pending_directory: PendingInstanceDirectory,
    creation_error: StorageError,
) -> StorageError {
    let path = pending_directory.instance_root.clone();
    let creation_error_message = creation_error.to_string();
    match tokio::task::spawn_blocking(move || {
        rollback_pending_instance_directory_blocking(pending_directory, creation_error)
    })
    .await
    {
        Ok(error) => error,
        Err(cleanup_error) => StorageError::InstanceCreationRollback {
            path,
            creation_error: creation_error_message,
            cleanup_error: cleanup_error.to_string(),
        },
    }
}

fn rollback_pending_instance_directory_blocking(
    pending_directory: PendingInstanceDirectory,
    creation_error: StorageError,
) -> StorageError {
    let path = pending_directory.instance_root.clone();
    match pending_directory.rollback() {
        Ok(()) => creation_error,
        Err(cleanup_error) => StorageError::InstanceCreationRollback {
            path,
            creation_error: creation_error.to_string(),
            cleanup_error: cleanup_error.to_string(),
        },
    }
}

fn cleanup_owned_instance_directory(
    instance_root: &Path,
    instances_root: &Path,
) -> Result<(), StorageError> {
    validate_managed_instance_root(instance_root, instances_root)?;
    if fs::symlink_metadata(instance_root.join(ADOPTION_JOURNAL)).is_ok() {
        return Err(crate::program_runtime::invalid(
            instance_root,
            "an unresolved installation adoption was preserved; recover it before removing this directory",
        ));
    }
    let metadata = match fs::symlink_metadata(instance_root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: instance_root.to_path_buf(),
                source,
            });
        }
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
    {
        return Err(StorageError::UnsafeManagedPath {
            path: instance_root.to_path_buf(),
            root: instances_root.to_path_buf(),
        });
    }

    let canonical_instances_root =
        fs::canonicalize(instances_root).map_err(|source| StorageError::ReadPath {
            path: instances_root.to_path_buf(),
            source,
        })?;
    let canonical_instance_root =
        fs::canonicalize(instance_root).map_err(|source| StorageError::ReadPath {
            path: instance_root.to_path_buf(),
            source,
        })?;
    if canonical_instance_root.parent() != Some(canonical_instances_root.as_path()) {
        return Err(StorageError::UnsafeManagedPath {
            path: instance_root.to_path_buf(),
            root: instances_root.to_path_buf(),
        });
    }

    fs::remove_dir_all(instance_root).map_err(|source| StorageError::DeletePath {
        path: instance_root.to_path_buf(),
        source,
    })
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

pub async fn update_instance(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
) -> Result<InstanceDetails, StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, &input.id)?;
    update_instance_locked(paths, input, &settings_lock).await
}

pub async fn update_instance_if_current(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
    expected_settings_json: &str,
) -> Result<InstanceDetails, StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, &input.id)?;
    let current = read_instance_details(paths, &input.id).await?;
    if !settings_json_matches(&current.settings_json, expected_settings_json) {
        return Err(StorageError::InstanceSettingsPreconditionFailed {
            id: input.id.clone(),
        });
    }
    update_instance_with_baseline_locked(paths, input, &settings_lock, Some(current.settings_json))
        .await
}

fn settings_json_matches(current: &str, expected: &str) -> bool {
    match (
        serde_json::from_str::<Value>(current),
        serde_json::from_str::<Value>(expected),
    ) {
        (Ok(current), Ok(expected)) => current == expected,
        _ => current == expected,
    }
}

#[cfg(test)]
mod settings_precondition_tests {
    use super::settings_json_matches;

    #[test]
    fn compares_json_semantically() {
        assert!(settings_json_matches(
            r#"{"name":"server","players":["one"],"enabled":true}"#,
            r#"{ "enabled": true, "players": ["one"], "name": "server" }"#,
        ));
    }

    #[test]
    fn detects_changed_nested_values() {
        assert!(!settings_json_matches(
            r#"{"access":{"banned":["76561198000000001"]}}"#,
            r#"{"access":{"banned":[]}}"#,
        ));
    }
}

pub(crate) async fn update_instance_locked(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
    settings_lock: &InstanceSettingsLock,
) -> Result<InstanceDetails, StorageError> {
    update_instance_with_baseline_locked(paths, input, settings_lock, None).await
}

async fn update_instance_with_baseline_locked(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
    settings_lock: &InstanceSettingsLock,
    expected_settings_json: Option<String>,
) -> Result<InstanceDetails, StorageError> {
    let paths = paths.clone();
    let transaction_lock = settings_lock.clone();
    settings_lock
        .complete_mutation("updating instance", async move {
            update_instance_transaction(&paths, input, &transaction_lock, expected_settings_json)
                .await
        })
        .await
}

async fn update_instance_transaction(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
    settings_lock: &InstanceSettingsLock,
    expected_settings_json: Option<String>,
) -> Result<InstanceDetails, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin().await?;
        let record = fetch_instance_record(&mut *tx, &input.id).await?;
        let config_file_path = record.config_dir.join("instance.json");
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let backup_uses_declared_saves_path = module_declares_saves_path(descriptor.as_ref());
        let shared_install_root =
            resolve_module_install_root_from_executor(&mut *tx, &record.summary.module_id)
                .await?
                .map(PathBuf::from)
                .or_else(|| {
                    descriptor.as_ref().and_then(|descriptor| {
                        descriptor
                            .install
                            .as_ref()
                            .map(|install| paths.games_root.join(&install.shared_game_dir))
                    })
                })
                .unwrap_or_else(|| paths.games_root.join(&record.summary.module_id));
        let template_install_root = effective_instance_install_root(&record)?;
        let instance_root = record
            .config_dir
            .parent()
            .unwrap_or(record.config_dir.as_path())
            .to_path_buf();
        let mut incoming_settings = parse_settings_object(&input.settings_json)?;
        let persisted_settings =
            parse_settings_object(&read_instance_settings_json(&config_file_path)?)?;
        let native_permissions =
            MoriaPermissionsSnapshot::read(&record.summary.module_id, &template_install_root)?;
        if let Some(snapshot) = &native_permissions {
            snapshot.reconcile(
                &record.summary.id,
                &persisted_settings,
                &mut incoming_settings,
                expected_settings_json.as_deref(),
            )?;
        }
        let has_active_run = load_active_instance_run(&mut *tx, &input.id)
            .await?
            .is_some();
        crate::settings_validation::validate_program_update_policy_change(
            &record.summary.module_id,
            &persisted_settings,
            &incoming_settings,
            has_active_run
                || record.summary.active_process_count > 0
                || matches!(
                    record.summary.status,
                    InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
                ),
        )?;
        crate::ark_maps::validate_changes(
            &record.summary.module_id,
            &persisted_settings,
            &incoming_settings,
            has_active_run
                || record.summary.active_process_count > 0
                || matches!(
                    record.summary.status,
                    InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
                ),
        )?;
        reject_active_sensitive_changes(
            &record.summary.module_id,
            &record.summary.status,
            has_active_run,
            record.summary.active_process_count,
            &persisted_settings,
            &incoming_settings,
            native_permissions.as_ref(),
        )?;
        let sensitive_baseline =
            matches!(record.summary.module_id.as_str(), "returntomoria" | "scum")
                .then(|| persisted_settings.clone());
        let humanitz_roster_baseline =
            (record.summary.module_id == "humanitz").then(|| persisted_settings.clone());
        preserve_generated_secrets(
            descriptor.as_ref(),
            &persisted_settings,
            &mut incoming_settings,
        )?;
        if record.summary.module_id == "rimworld" {
            crate::templates::preserve_rimworld_retired_settings(
                &persisted_settings,
                &mut incoming_settings,
            );
        }
        if record.summary.module_id == "projectzomboid" {
            crate::templates::preserve_projectzomboid_retired_settings(
                &persisted_settings,
                &mut incoming_settings,
            )?;
        }
        let incoming_settings = if record.summary.module_id == "dontstarve" {
            crate::templates::dst_world_settings::reconcile(
                descriptor.as_ref(),
                persisted_settings,
                incoming_settings,
            )?
        } else {
            incoming_settings
        };
        let settings = normalize_complete_instance_settings(
            descriptor.as_ref(),
            incoming_settings,
            &record.summary.id,
            &record.summary.name,
            &input.bind_ip,
        )?;
        if let Some(baseline) = &humanitz_roster_baseline {
            crate::player_access_normalization::validate_humanitz_roster_update(
                baseline, &settings,
            )?;
        }
        let save_path_context = InstanceSavePathContext {
            install_root: &template_install_root,
            instance_root: &instance_root,
            config_dir: &record.config_dir,
            instance_id: &record.summary.id,
            instance_name: &record.summary.name,
            module_id: &record.summary.module_id,
            settings: Some(&settings),
        };
        let saves_path = if backup_uses_declared_saves_path {
            planned_instance_saves_dir(descriptor.as_ref(), &save_path_context)?
        } else {
            record.saves_dir.clone()
        };
        if let Err(error) = ensure_instance_paths_available(
            paths,
            &mut tx,
            &record.summary.id,
            &record.summary.module_id,
            &template_install_root,
            &record.config_dir,
            &saves_path,
        )
        .await
        {
            let rollback = tx.rollback().await;
            rollback.map_err(|source| StorageError::BlockingTaskFailed {
                operation: "rolling back rejected instance paths",
                message: format!("{error}; rollback failed: {source}"),
            })?;
            return Err(error);
        }
        let port_groups = crate::ark_maps::port_groups(descriptor.as_ref(), &settings)?;
        let instance_running = matches!(
            &record.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        ) || has_active_run
            || record.summary.active_process_count > 0;
        let current_ports = load_instance_ports(&mut *tx, &input.id).await?;
        let mut requested_ports = crate::ark_maps::requested_ports(
            descriptor.as_ref(),
            &settings,
            &input.ports,
            &current_ports,
        )?;
        // Existing clusters gain the newly supported native roles at the same
        // transactional port allocation boundary as new instances.
        if record.summary.module_id == "dontstarve"
            && !instance_running
            && let Some(descriptor) = descriptor.as_ref()
        {
            for port in &descriptor.default_ports {
                if !requested_ports
                    .iter()
                    .any(|existing| existing.name == port.name)
                {
                    requested_ports.push(port.clone());
                }
            }
        }
        if instance_running
            && (record.summary.bind_ip != input.bind_ip
                || canonical_port_bindings(&current_ports)
                    != canonical_port_bindings(&requested_ports))
        {
            return Err(StorageError::ActiveInstanceNetworkMutation {
                id: input.id.clone(),
            });
        }
        tx.rollback().await?;
        let prepared_packages = prepare_workshop_configuration_in_worker(
            &ModuleSupportMaterializationContext {
                storage_paths: paths,
                module_id: &record.summary.module_id,
                install_root: &template_install_root,
                shared_install_root: &shared_install_root,
                config_dir: &record.config_dir,
                saves_dir: &saves_path,
                instance_id: &record.summary.id,
                instance_running,
                settings: &settings,
            },
            settings_lock,
        )
        .await?;
        #[cfg(test)]
        if sensitive_baseline.is_some() {
            let database = paths.database_path.clone();
            settings_lock
                .spawn_blocking(move || {
                    crate::instance_archive::test_gate::pause(
                        &database,
                        crate::instance_archive::test_gate::Point::NativeSettingsReady,
                    );
                })
                .await
                .map_err(|error| StorageError::BlockingTaskFailed {
                    operation: "pausing native settings publication test",
                    message: error.to_string(),
                })?;
        }
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        configuration_packages::revalidate(&mut tx, &record, &current_ports, instance_running)
            .await?;
        if let Some(baseline) = &sensitive_baseline {
            let current = fetch_instance_record(&mut *tx, &input.id).await?;
            reject_active_sensitive_changes(
                &record.summary.module_id,
                &current.summary.status,
                load_active_instance_run(&mut *tx, &input.id)
                    .await?
                    .is_some(),
                current.summary.active_process_count,
                baseline,
                &settings,
                native_permissions.as_ref(),
            )?;
        }
        ensure_instance_paths_available(
            paths,
            &mut tx,
            &record.summary.id,
            &record.summary.module_id,
            &template_install_root,
            &record.config_dir,
            &saves_path,
        )
        .await?;
        let ports = replace_instance_ports(
            &mut tx,
            &input.id,
            &record.summary.module_id,
            &requested_ports,
            &port_groups,
        )
        .await?;
        let backup_retention_count = input.backup_retention_count.max(1);

        sqlx::query(
            r#"
        UPDATE instances
        SET bind_ip = ?2,
            auto_backup_on_stop = ?3,
            backup_retention_count = ?4,
            saves_path = ?5,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        )
        .bind(&input.id)
        .bind(&input.bind_ip)
        .bind(if input.auto_backup_on_stop {
            1_i64
        } else {
            0_i64
        })
        .bind(i64::from(backup_retention_count))
        .bind(saves_path.to_string_lossy().into_owned())
        .execute(&mut *tx)
        .await?;

        let templates_root = paths
            .modules_root
            .join(&record.summary.module_id)
            .join("templates");
        let settings_json = serde_json::to_string_pretty(&Value::Object(settings.clone()))?;
        let config_input = InstanceConfigInput {
            instance_id: &record.summary.id,
            instance_name: &record.summary.name,
            module_id: &record.summary.module_id,
            bind_ip: &input.bind_ip,
            autostart: record.summary.autostart,
            settings: settings.clone(),
            ports: &ports,
        };
        let config_mutation = write_pending_instance_configuration_in_worker(
            &templates_root,
            &ModuleSupportMaterializationContext {
                storage_paths: paths,
                module_id: &record.summary.module_id,
                install_root: &template_install_root,
                shared_install_root: &shared_install_root,
                config_dir: &record.config_dir,
                saves_dir: &saves_path,
                instance_id: &record.summary.id,
                instance_running,
                settings: &settings,
            },
            &config_file_path,
            config_input,
            settings_lock,
            prepared_packages,
            native_permissions,
        )
        .await?;
        let active_run = load_active_instance_run(&mut *tx, &input.id).await?;
        commit_instance_transaction(tx, config_mutation, settings_lock).await?;

        Ok(InstanceDetails {
            summary: InstanceSummary {
                id: record.summary.id,
                name: record.summary.name,
                module_id: record.summary.module_id,
                status: record.summary.status,
                active_process_count: record.summary.active_process_count,
                bind_ip: input.bind_ip,
                port_count: ports.len(),
                autostart: record.summary.autostart,
            },
            config_file_path: config_file_path.to_string_lossy().into_owned(),
            saves_path: saves_path.to_string_lossy().into_owned(),
            backup_uses_declared_saves_path,
            auto_backup_on_stop: input.auto_backup_on_stop,
            backup_retention_count,
            settings_json,
            ports,
            active_run,
        })
    }
    .await;
    // Await SQLite worker shutdown before releasing the mutation lease on errors too.
    pool.close().await;
    result
}

fn canonical_port_bindings(ports: &[PortBinding]) -> Vec<(&str, &str, u16)> {
    let mut bindings = ports
        .iter()
        .map(|port| (port.name.as_str(), port.protocol.as_str(), port.port))
        .collect::<Vec<_>>();
    bindings.sort_unstable();
    bindings
}

pub async fn update_instance_ports(
    paths: &StoragePaths,
    instance_id: &str,
    ports: &[PortBinding],
) -> Result<Vec<PortBinding>, StorageError> {
    let _settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let pool = connect_pool(paths).await?;
    let mut tx = pool.begin().await?;

    let record = fetch_instance_record(&mut *tx, instance_id).await?;
    let current_ports = load_instance_ports(&mut *tx, instance_id).await?;
    let instance_running = matches!(
        record.summary.status,
        InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
    ) || load_active_instance_run(&mut *tx, instance_id)
        .await?
        .is_some();
    if instance_running {
        if canonical_port_bindings(&current_ports) != canonical_port_bindings(ports) {
            return Err(StorageError::ActiveInstanceNetworkMutation {
                id: instance_id.to_owned(),
            });
        }
        tx.rollback().await?;
        pool.close().await;
        return Ok(current_ports);
    }
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let settings = parse_settings_object(&read_instance_settings_json(
        &record.config_dir.join("instance.json"),
    )?)?;
    let port_groups = crate::ark_maps::port_groups(descriptor.as_ref(), &settings)?;
    let requested_ports =
        crate::ark_maps::requested_ports(descriptor.as_ref(), &settings, ports, &current_ports)?;
    let ports = replace_instance_ports(
        &mut tx,
        instance_id,
        &record.summary.module_id,
        &requested_ports,
        &port_groups,
    )
    .await?;
    sqlx::query(
        "UPDATE instances
         SET updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
    )
    .bind(instance_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    pool.close().await;

    Ok(ports)
}

pub async fn materialize_instance_configuration(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceDetails, StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    materialize_instance_configuration_locked(paths, instance_id, &settings_lock).await
}

pub async fn materialize_instance_configuration_for_start(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceDetails, StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    materialize_instance_configuration_with_policy(paths, instance_id, &settings_lock, true).await
}

pub(crate) async fn materialize_instance_configuration_locked(
    paths: &StoragePaths,
    instance_id: &str,
    settings_lock: &InstanceSettingsLock,
) -> Result<InstanceDetails, StorageError> {
    materialize_instance_configuration_with_policy(paths, instance_id, settings_lock, false).await
}

async fn materialize_instance_configuration_with_policy(
    paths: &StoragePaths,
    instance_id: &str,
    settings_lock: &InstanceSettingsLock,
    apply_prestart_support: bool,
) -> Result<InstanceDetails, StorageError> {
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let transaction_lock = settings_lock.clone();
    settings_lock
        .complete_mutation("materializing instance configuration", async move {
            materialize_instance_configuration_transaction(
                &paths,
                &instance_id,
                &transaction_lock,
                apply_prestart_support,
            )
            .await
        })
        .await
}

async fn materialize_instance_configuration_transaction(
    paths: &StoragePaths,
    instance_id: &str,
    settings_lock: &InstanceSettingsLock,
    apply_prestart_support: bool,
) -> Result<InstanceDetails, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin().await?;
        let record = fetch_instance_record(&mut *tx, instance_id).await?;
        let ports = load_instance_ports(&mut *tx, instance_id).await?;
        let config_file_path = record.config_dir.join("instance.json");
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let backup_uses_declared_saves_path = module_declares_saves_path(descriptor.as_ref());
        let shared_install_root =
            resolve_module_install_root_from_executor(&mut *tx, &record.summary.module_id)
                .await?
                .map(PathBuf::from)
                .or_else(|| {
                    descriptor.as_ref().and_then(|descriptor| {
                        descriptor
                            .install
                            .as_ref()
                            .map(|install| paths.games_root.join(&install.shared_game_dir))
                    })
                })
                .unwrap_or_else(|| paths.games_root.join(&record.summary.module_id));
        let template_install_root = effective_instance_install_root(&record)?;
        let instance_root = record
            .config_dir
            .parent()
            .unwrap_or(record.config_dir.as_path())
            .to_path_buf();
        let mut persisted_settings =
            parse_settings_object(&read_instance_settings_json(&config_file_path)?)?;
        let native_permissions =
            MoriaPermissionsSnapshot::read(&record.summary.module_id, &template_install_root)?;
        if let Some(snapshot) = &native_permissions {
            snapshot.project(&mut persisted_settings);
        }
        let settings = normalize_complete_instance_settings(
            descriptor.as_ref(),
            persisted_settings,
            &record.summary.id,
            &record.summary.name,
            &record.summary.bind_ip,
        )?;
        let settings_json = serde_json::to_string_pretty(&Value::Object(settings.clone()))?;
        let save_path_context = InstanceSavePathContext {
            install_root: &template_install_root,
            instance_root: &instance_root,
            config_dir: &record.config_dir,
            instance_id: &record.summary.id,
            instance_name: &record.summary.name,
            module_id: &record.summary.module_id,
            settings: Some(&settings),
        };
        let saves_path = if backup_uses_declared_saves_path {
            planned_instance_saves_dir(descriptor.as_ref(), &save_path_context)?
        } else {
            record.saves_dir.clone()
        };
        if let Err(error) = ensure_instance_paths_available(
            paths,
            &mut tx,
            &record.summary.id,
            &record.summary.module_id,
            &template_install_root,
            &record.config_dir,
            &saves_path,
        )
        .await
        {
            let rollback = tx.rollback().await;
            rollback.map_err(|source| StorageError::BlockingTaskFailed {
                operation: "rolling back rejected instance paths",
                message: format!("{error}; rollback failed: {source}"),
            })?;
            return Err(error);
        }

        let instance_running = matches!(
            &record.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        ) || load_active_instance_run(&mut *tx, instance_id)
            .await?
            .is_some()
            || record.summary.active_process_count > 0;
        let support_context = ModuleSupportMaterializationContext {
            storage_paths: paths,
            module_id: &record.summary.module_id,
            install_root: &template_install_root,
            shared_install_root: &shared_install_root,
            config_dir: &record.config_dir,
            saves_dir: &saves_path,
            instance_id: &record.summary.id,
            instance_running,
            settings: &settings,
        };
        tx.rollback().await?;
        let prepared_packages =
            prepare_workshop_configuration_in_worker(&support_context, settings_lock).await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        configuration_packages::revalidate(&mut tx, &record, &ports, instance_running).await?;
        ensure_instance_paths_available(
            paths,
            &mut tx,
            &record.summary.id,
            &record.summary.module_id,
            &template_install_root,
            &record.config_dir,
            &saves_path,
        )
        .await?;
        if record.saves_dir != saves_path {
            sqlx::query(
                r#"
            UPDATE instances
            SET saves_path = ?2,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#,
            )
            .bind(&record.summary.id)
            .bind(saves_path.to_string_lossy().into_owned())
            .execute(&mut *tx)
            .await?;
        }

        let config_mutation = write_pending_instance_configuration_in_worker(
            &paths
                .modules_root
                .join(&record.summary.module_id)
                .join("templates"),
            &support_context,
            &config_file_path,
            InstanceConfigInput {
                instance_id: &record.summary.id,
                instance_name: &record.summary.name,
                module_id: &record.summary.module_id,
                bind_ip: &record.summary.bind_ip,
                autostart: record.summary.autostart,
                settings: settings.clone(),
                ports: &ports,
            },
            settings_lock,
            prepared_packages,
            native_permissions,
        )
        .await?;

        commit_instance_transaction(tx, config_mutation, settings_lock).await?;
        let active_run = load_active_instance_run(&pool, instance_id).await?;
        if apply_prestart_support {
            let prestart_context = ModuleSupportMaterializationContext {
                instance_running: instance_running || active_run.is_some(),
                ..support_context
            };
            apply_module_prestart_support(&prestart_context, settings_lock).await?;
        }

        Ok(InstanceDetails {
            summary: record.summary,
            config_file_path: config_file_path.to_string_lossy().into_owned(),
            saves_path: saves_path.to_string_lossy().into_owned(),
            backup_uses_declared_saves_path,
            auto_backup_on_stop: record.auto_backup_on_stop,
            backup_retention_count: record.backup_retention_count,
            settings_json,
            ports,
            active_run,
        })
    }
    .await;
    // Await SQLite worker shutdown before releasing the mutation lease on errors too.
    pool.close().await;
    result
}

pub(crate) async fn commit_instance_transaction(
    tx: Transaction<'_, Sqlite>,
    config_mutation: ManagedConfigMutation,
    settings_lock: &InstanceSettingsLock,
) -> Result<(), StorageError> {
    match tx.commit().await {
        Ok(()) => {
            config_mutation.commit();
            Ok(())
        }
        Err(source) => {
            let original = StorageError::Sqlx(source);
            let original_message = original.to_string();
            match settings_lock
                .spawn_blocking(move || config_mutation.rollback_after(original))
                .await
            {
                Ok(error) => Err(error),
                Err(error) => Err(StorageError::BlockingTaskFailed {
                    operation: "rolling back instance files after a failed database commit",
                    message: format!("{original_message}; rollback task failed: {error}"),
                }),
            }
        }
    }
}

async fn allocate_default_ports(
    tx: &mut Transaction<'_, Sqlite>,
    instance_id: &str,
    module_id: &str,
    default_ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
) -> Result<Vec<PortBinding>, StorageError> {
    persist_instance_ports(tx, instance_id, module_id, default_ports, port_groups, None).await
}

async fn replace_instance_ports(
    tx: &mut Transaction<'_, Sqlite>,
    instance_id: &str,
    module_id: &str,
    ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
) -> Result<Vec<PortBinding>, StorageError> {
    let current_ports = load_instance_ports(&mut **tx, instance_id).await?;
    persist_instance_ports(
        tx,
        instance_id,
        module_id,
        ports,
        port_groups,
        Some(&current_ports),
    )
    .await
}

async fn persist_instance_ports(
    tx: &mut Transaction<'_, Sqlite>,
    instance_id: &str,
    module_id: &str,
    ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
    current_ports: Option<&[PortBinding]>,
) -> Result<Vec<PortBinding>, StorageError> {
    let normalized_ports = ports.iter().map(normalize_port_binding).collect::<Vec<_>>();
    let mut port_indexes = HashMap::new();
    for (index, port) in normalized_ports.iter().enumerate() {
        if port_indexes.insert(port.name.clone(), index).is_some() {
            return Err(StorageError::DuplicatePortNameInRequest {
                name: port.name.clone(),
            });
        }
    }

    let mut member_groups = HashMap::new();
    for (group_index, group) in port_groups.iter().enumerate() {
        if let Some(offsets) = group.member_offsets.as_ref() {
            if let Some(offset_member) = offsets
                .keys()
                .find(|member| !group.members.contains(member))
            {
                return Err(StorageError::InvalidPortGroupRequest {
                    group_id: group.id.clone(),
                    message: format!("offset references unknown binding `{offset_member}`"),
                });
            }
            if let Some(member) = group
                .members
                .iter()
                .find(|member| !offsets.contains_key(member.as_str()))
            {
                return Err(StorageError::InvalidPortGroupRequest {
                    group_id: group.id.clone(),
                    message: format!("binding `{member}` is missing an offset"),
                });
            }
        }

        let mut endpoint_shapes = HashSet::new();
        for member in &group.members {
            let Some(index) = port_indexes.get(member).copied() else {
                return Err(StorageError::InvalidPortGroupRequest {
                    group_id: group.id.clone(),
                    message: format!("missing binding `{member}`"),
                });
            };
            if member_groups.insert(member.clone(), group_index).is_some() {
                return Err(StorageError::InvalidPortGroupRequest {
                    group_id: group.id.clone(),
                    message: format!("binding `{member}` belongs to multiple groups"),
                });
            }
            let binding = &normalized_ports[index];
            let offset = port_group_member_offset(group, member);
            if !endpoint_shapes.insert((binding.protocol.clone(), offset)) {
                return Err(StorageError::InvalidPortGroupRequest {
                    group_id: group.id.clone(),
                    message: format!(
                        "protocol `{}` is repeated at offset {offset}",
                        binding.protocol
                    ),
                });
            }
        }
    }

    if current_ports.is_some() {
        let mut seen = HashMap::new();
        for port in &normalized_ports {
            let endpoint = (port.protocol.clone(), port.port);
            if let Some(previous_name) = seen.insert(endpoint, port.name.as_str()) {
                let previous_group = member_groups.get(previous_name);
                let current_group = member_groups.get(port.name.as_str());
                if previous_group.is_some() && previous_group == current_group {
                    continue;
                }
                return Err(StorageError::DuplicatePortInRequest {
                    protocol: port.protocol.clone(),
                    port: port.port,
                });
            }
        }
    }

    sqlx::query("DELETE FROM instance_ports WHERE instance_id = ?1")
        .bind(instance_id)
        .execute(&mut **tx)
        .await?;

    let mut reserved = HashSet::new();
    let mut assigned_ports = vec![None; normalized_ports.len()];
    let mut allocated_groups = HashSet::new();

    for (index, port) in normalized_ports.iter().enumerate() {
        if let Some(group_index) = member_groups.get(&port.name).copied() {
            if !allocated_groups.insert(group_index) {
                continue;
            }
            let group = &port_groups[group_index];
            let member_indexes = group
                .members
                .iter()
                .map(|member| port_indexes[member])
                .collect::<Vec<_>>();
            let requested_base_port = resolve_group_requested_base_port(
                group,
                &member_indexes,
                &normalized_ports,
                current_ports,
            )?;
            let has_nonzero_offset = port_group_has_nonzero_offset(group);
            let assigned_base_port =
                if current_ports.is_some() && requested_base_port == 0 && !has_nonzero_offset {
                    0
                } else {
                    next_available_port_for_group(
                        tx,
                        group,
                        &member_indexes,
                        &normalized_ports,
                        requested_base_port,
                        &reserved,
                    )
                    .await?
                };
            for member_index in member_indexes {
                let protocol = normalized_ports[member_index].protocol.clone();
                let member = &normalized_ports[member_index].name;
                let offset = port_group_member_offset(group, member);
                let assigned_port = assigned_base_port.checked_add(offset).ok_or_else(|| {
                    StorageError::InvalidPortGroupRequest {
                        group_id: group.id.clone(),
                        message: format!(
                            "base port {assigned_base_port} plus offset {offset} for `{member}` exceeds 65535"
                        ),
                    }
                })?;
                if !reserved.insert((protocol.clone(), assigned_port)) {
                    return Err(StorageError::PortAllocationExhausted {
                        protocol,
                        start_port: requested_base_port,
                    });
                }
                assigned_ports[member_index] = Some(assigned_port);
            }
            continue;
        }

        let assigned_port = if current_ports.is_some() && port.port == 0 {
            0
        } else {
            let allocation_range =
                automatic_port_allocation_range(module_id, port, current_ports.is_none());
            next_available_port_for_protocols(
                tx,
                std::slice::from_ref(&port.protocol),
                port.port,
                allocation_range,
                &reserved,
            )
            .await?
        };
        if !reserved.insert((port.protocol.clone(), assigned_port)) {
            return Err(StorageError::PortAllocationExhausted {
                protocol: port.protocol.clone(),
                start_port: port.port,
            });
        }
        assigned_ports[index] = Some(assigned_port);
    }

    let mut saved = Vec::with_capacity(normalized_ports.len());
    for (port, assigned_port) in normalized_ports.into_iter().zip(assigned_ports) {
        let assigned_port = assigned_port.ok_or_else(|| StorageError::PortBindingNotAllocated {
            name: port.name.clone(),
        })?;

        sqlx::query(
            r#"
            INSERT INTO instance_ports (instance_id, name, port, protocol)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(instance_id)
        .bind(&port.name)
        .bind(i64::from(assigned_port))
        .bind(&port.protocol)
        .execute(&mut **tx)
        .await?;

        saved.push(PortBinding {
            name: port.name,
            protocol: port.protocol,
            port: assigned_port,
        });
    }

    Ok(saved)
}

fn port_group_member_offset(group: &ModulePortGroupSpec, member: &str) -> u16 {
    group
        .member_offsets
        .as_ref()
        .and_then(|offsets| offsets.get(member))
        .copied()
        .unwrap_or(0)
}

fn port_group_has_nonzero_offset(group: &ModulePortGroupSpec) -> bool {
    group
        .member_offsets
        .as_ref()
        .is_some_and(|offsets| offsets.values().any(|offset| *offset != 0))
}

fn resolve_group_requested_base_port(
    group: &ModulePortGroupSpec,
    member_indexes: &[usize],
    requested_ports: &[PortBinding],
    current_ports: Option<&[PortBinding]>,
) -> Result<u16, StorageError> {
    if member_indexes
        .iter()
        .all(|index| requested_ports[*index].port == 0)
    {
        if port_group_has_nonzero_offset(group) {
            return Err(StorageError::InvalidPortGroupRequest {
                group_id: group.id.clone(),
                message: String::from("fixed-offset groups cannot use base port 0"),
            });
        }
        return Ok(0);
    }

    let mut requested_bases = HashSet::new();
    for member_index in member_indexes {
        let binding = &requested_ports[*member_index];
        if binding.port == 0 {
            continue;
        }
        let offset = port_group_member_offset(group, &binding.name);
        let base_port = binding.port.checked_sub(offset).ok_or_else(|| {
            StorageError::InvalidPortGroupRequest {
                group_id: group.id.clone(),
                message: format!(
                    "binding `{}` port {} is below its offset {offset}",
                    binding.name, binding.port
                ),
            }
        })?;
        requested_bases.insert(base_port);
    }
    if requested_bases.len() == 1 {
        let base_port = requested_bases.into_iter().next().unwrap_or(0);
        if base_port == 0 && port_group_has_nonzero_offset(group) {
            return Err(StorageError::InvalidPortGroupRequest {
                group_id: group.id.clone(),
                message: String::from("fixed-offset groups cannot use base port 0"),
            });
        }
        return Ok(base_port);
    }

    if let Some(current_ports) = current_ports {
        let current_by_name = current_ports
            .iter()
            .map(|port| (port.name.as_str(), port))
            .collect::<HashMap<_, _>>();
        let changed_members = member_indexes
            .iter()
            .filter(|index| {
                let requested = &requested_ports[**index];
                current_by_name
                    .get(requested.name.as_str())
                    .is_some_and(|current| current.port != requested.port)
            })
            .copied()
            .collect::<Vec<_>>();
        if changed_members.len() == 1 {
            let changed = &requested_ports[changed_members[0]];
            if changed.port != 0 {
                let offset = port_group_member_offset(group, &changed.name);
                let base_port = changed.port.checked_sub(offset).ok_or_else(|| {
                    StorageError::InvalidPortGroupRequest {
                        group_id: group.id.clone(),
                        message: format!(
                            "binding `{}` port {} is below its offset {offset}",
                            changed.name, changed.port
                        ),
                    }
                })?;
                if base_port == 0 && port_group_has_nonzero_offset(group) {
                    return Err(StorageError::InvalidPortGroupRequest {
                        group_id: group.id.clone(),
                        message: String::from("fixed-offset groups cannot use base port 0"),
                    });
                }
                return Ok(base_port);
            }
        }
    }

    if group.member_offsets.is_none() {
        return Ok(requested_bases.into_iter().max().unwrap_or(0));
    }

    Err(StorageError::InvalidPortGroupRequest {
        group_id: group.id.clone(),
        message: String::from("member ports resolve to conflicting base ports"),
    })
}

async fn next_available_port_for_group(
    tx: &mut Transaction<'_, Sqlite>,
    group: &ModulePortGroupSpec,
    member_indexes: &[usize],
    ports: &[PortBinding],
    start_base_port: u16,
    reserved: &HashSet<(String, u16)>,
) -> Result<u16, StorageError> {
    let mut candidate_base = start_base_port.max(1);

    loop {
        let mut available = true;
        for member_index in member_indexes {
            let binding = &ports[*member_index];
            let offset = port_group_member_offset(group, &binding.name);
            let Some(candidate_port) = candidate_base.checked_add(offset) else {
                available = false;
                break;
            };
            if reserved.contains(&(binding.protocol.clone(), candidate_port)) {
                available = false;
                break;
            }
            let occupied: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM instance_ports WHERE protocol = ?1 AND port = ?2)",
            )
            .bind(&binding.protocol)
            .bind(i64::from(candidate_port))
            .fetch_one(&mut **tx)
            .await?;
            if occupied != 0 {
                available = false;
                break;
            }
        }
        if available {
            return Ok(candidate_base);
        }

        if candidate_base == u16::MAX {
            break;
        }
        candidate_base = candidate_base.saturating_add(1);
    }

    Err(StorageError::PortAllocationExhausted {
        protocol: member_indexes
            .iter()
            .map(|index| ports[*index].protocol.as_str())
            .collect::<Vec<_>>()
            .join("+"),
        start_port: start_base_port,
    })
}

async fn next_available_port_for_protocols(
    tx: &mut Transaction<'_, Sqlite>,
    protocols: &[String],
    start_port: u16,
    allocation_range: Option<(u16, u16)>,
    reserved: &HashSet<(String, u16)>,
) -> Result<u16, StorageError> {
    let (minimum_port, maximum_port, wraps) = allocation_range
        .map(|(minimum, maximum)| (minimum, maximum, true))
        .unwrap_or((1, u16::MAX, false));
    let mut candidate = start_port.clamp(minimum_port, maximum_port);
    let first_candidate = candidate;

    loop {
        let mut available = true;
        for protocol in protocols {
            if reserved.contains(&(protocol.clone(), candidate)) {
                available = false;
                break;
            }
            let occupied: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM instance_ports WHERE protocol = ?1 AND port = ?2)",
            )
            .bind(protocol)
            .bind(i64::from(candidate))
            .fetch_one(&mut **tx)
            .await?;
            if occupied != 0 {
                available = false;
                break;
            }
        }
        if available {
            return Ok(candidate);
        }

        if candidate == maximum_port {
            if !wraps || minimum_port == first_candidate {
                break;
            }
            candidate = minimum_port;
        } else {
            candidate = candidate.saturating_add(1);
        }
        if candidate == first_candidate {
            break;
        }
    }

    Err(StorageError::PortAllocationExhausted {
        protocol: protocols.join("+"),
        start_port,
    })
}

fn automatic_port_allocation_range(
    module_id: &str,
    port: &PortBinding,
    allocating_defaults: bool,
) -> Option<(u16, u16)> {
    let dst_player_port = module_id == "dontstarve"
        && port.protocol.eq_ignore_ascii_case("udp")
        && matches!(
            port.name.as_str(),
            "master" | "caves" | "islands" | "volcano"
        );
    if dst_player_port
        && (allocating_defaults
            || (DST_LAN_DISCOVERY_PORT_MIN..=DST_LAN_DISCOVERY_PORT_MAX).contains(&port.port))
    {
        Some((DST_LAN_DISCOVERY_PORT_MIN, DST_LAN_DISCOVERY_PORT_MAX))
    } else {
        None
    }
}

#[cfg(test)]
mod dst_port_allocation_tests {
    use super::*;

    #[test]
    fn all_four_dst_player_ports_use_the_native_lan_discovery_range() {
        for name in ["master", "caves", "islands", "volcano"] {
            let port = PortBinding {
                name: name.into(),
                protocol: "udp".into(),
                port: 10999,
            };
            assert_eq!(
                automatic_port_allocation_range("dontstarve", &port, true),
                Some((DST_LAN_DISCOVERY_PORT_MIN, DST_LAN_DISCOVERY_PORT_MAX))
            );
            assert_eq!(
                automatic_port_allocation_range("dontstarve", &port, false),
                Some((DST_LAN_DISCOVERY_PORT_MIN, DST_LAN_DISCOVERY_PORT_MAX))
            );
            assert_eq!(automatic_port_allocation_range("other", &port, true), None);
        }
        let port = PortBinding {
            name: "islands_steam_query".into(),
            protocol: "udp".into(),
            port: 27019,
        };
        assert_eq!(
            automatic_port_allocation_range("dontstarve", &port, true),
            None
        );
    }
}

fn parse_settings_object(settings_json: &str) -> Result<Map<String, Value>, StorageError> {
    let value: Value = serde_json::from_str(settings_json)?;
    value
        .as_object()
        .cloned()
        .ok_or(StorageError::InvalidSettingsRoot)
}

pub(crate) fn merge_schema_defaults_with_settings(
    descriptor: Option<&ModuleDescriptor>,
    persisted_settings: Map<String, Value>,
    context: SchemaDefaultContext<'_>,
) -> Result<Map<String, Value>, StorageError> {
    let mut settings = collect_schema_defaults_from_schema_json(
        descriptor.and_then(|descriptor| descriptor.schema_json.as_deref()),
        context,
    )?;
    if descriptor.is_some_and(|descriptor| descriptor.summary.id == "satisfactory") {
        // These options remain native-owned until explicitly saved. Backfilling
        // UI defaults into an existing option map could re-enable telemetry.
        for key in [
            "auto_pause_when_empty",
            "network_quality",
            "send_gameplay_data",
        ] {
            settings.remove(key);
        }
    }
    settings.extend(persisted_settings);
    if let Some(descriptor) = descriptor {
        normalize_module_player_access_settings_strict(&descriptor.summary.id, &mut settings)?;
    }
    Ok(settings)
}

pub fn normalize_complete_instance_settings(
    descriptor: Option<&ModuleDescriptor>,
    persisted_settings: Map<String, Value>,
    instance_id: &str,
    instance_name: &str,
    bind_ip: &str,
) -> Result<Map<String, Value>, StorageError> {
    let mut settings = merge_schema_defaults_with_settings(
        descriptor,
        persisted_settings,
        SchemaDefaultContext {
            instance_id: Some(instance_id),
            instance_name: Some(instance_name),
        },
    )?;
    settings.insert(
        String::from("bind_ip"),
        Value::String(String::from(bind_ip)),
    );
    if descriptor.is_some_and(|descriptor| descriptor.summary.id == "dontstarve") {
        normalize_dontstarve_operational_settings(&mut settings);
    }
    crate::templates::dst_world_settings::project(descriptor, &mut settings)?;
    if let Some(descriptor) = descriptor {
        crate::ark_maps::normalize_settings(&descriptor.summary.id, instance_id, &mut settings)?;
    }
    validate_settings_against_schema(descriptor, &settings, SettingsValidationPhase::Complete)?;
    Ok(settings)
}

fn preserve_generated_secrets(
    descriptor: Option<&ModuleDescriptor>,
    persisted: &Map<String, Value>,
    incoming: &mut Map<String, Value>,
) -> Result<(), StorageError> {
    let Some(schema_json) = descriptor.and_then(|descriptor| descriptor.schema_json.as_deref())
    else {
        return Ok(());
    };
    let schema: Value = serde_json::from_str(schema_json)?;
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };

    // Missing a generated credential is not a request to rotate it. Only an
    // explicit replacement may change a credential on an existing instance.
    for (key, property) in properties {
        if property
            .get("x-lsgm-default-source")
            .and_then(Value::as_str)
            == Some("generated_secret")
            && !incoming.contains_key(key)
            && let Some(value) = persisted.get(key)
        {
            incoming.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

pub(crate) fn read_instance_settings_json(config_file_path: &Path) -> Result<String, StorageError> {
    let Some(content) = read_optional_file_to_string(config_file_path).map_err(|source| {
        StorageError::ReadConfig {
            path: config_file_path.to_path_buf(),
            source,
        }
    })?
    else {
        return Ok(String::from("{}"));
    };
    let document: Value =
        serde_json::from_str(&content).map_err(|source| StorageError::InvalidConfigJson {
            path: config_file_path.to_path_buf(),
            source,
        })?;
    let settings = document
        .get("settings")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));

    Ok(serde_json::to_string_pretty(&settings)?)
}

fn normalize_port_binding(port: &PortBinding) -> PortBinding {
    PortBinding {
        name: port.name.trim().to_string(),
        protocol: port.protocol.trim().to_ascii_lowercase(),
        port: port.port,
    }
}

fn slugify(value: &str) -> String {
    let base = value
        .chars()
        .flat_map(|char| char.to_lowercase())
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>();

    let collapsed = base
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if collapsed.is_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        format!("instance-{stamp}")
    } else {
        collapsed
    }
}

fn new_instance_id(name: &str) -> String {
    const MAX_SLUG_BYTES: usize = 32;

    let slug = slugify(name);
    let bounded_slug = slug
        .get(..slug.len().min(MAX_SLUG_BYTES))
        .unwrap_or(&slug)
        .trim_end_matches('-');
    let random = uuid::Uuid::new_v4().as_u128() as u64;
    format!("{bounded_slug}-{random:016x}")
}

#[cfg(test)]
mod instance_identifier_tests {
    use super::{
        PendingInstanceDirectory, StorageError, new_instance_id,
        rollback_pending_instance_directory_blocking,
    };

    #[test]
    fn instance_id_bounds_the_human_readable_component() {
        let id = new_instance_id(
            "A deliberately long server name that must not consume the Windows path budget",
        );
        let suffix_start = id.len() - 16;
        let slug = id[..suffix_start].trim_end_matches('-');

        assert!(slug.len() <= 32);
        assert!(!slug.ends_with('-'));
        u64::from_str_radix(&id[suffix_start..], 16).unwrap();
    }

    #[test]
    fn pending_directory_rollback_removes_only_the_directory_it_created() {
        let root = std::env::temp_dir().join(format!(
            "langame-instance-directory-rollback-{}",
            uuid::Uuid::new_v4()
        ));
        let instances_root = root.join("instances");
        std::fs::create_dir_all(&instances_root).unwrap();

        let preexisting = instances_root.join("preexisting");
        std::fs::create_dir(&preexisting).unwrap();
        std::fs::write(preexisting.join("sentinel"), b"keep").unwrap();
        PendingInstanceDirectory::create(instances_root.clone(), preexisting.clone())
            .expect_err("an existing directory must never be adopted for cleanup");
        assert!(preexisting.join("sentinel").is_file());

        let pending_root = instances_root.join("new-instance");
        let pending =
            PendingInstanceDirectory::create(instances_root.clone(), pending_root.clone()).unwrap();
        std::fs::create_dir(pending_root.join("runtime.staging")).unwrap();
        pending.rollback().unwrap();
        assert!(!pending_root.exists());
        assert!(preexisting.join("sentinel").is_file());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_pending_rollback_reports_cleanup_failure() {
        let root = std::env::temp_dir().join(format!(
            "langame-instance-directory-cleanup-error-{}",
            uuid::Uuid::new_v4()
        ));
        let instances_root = root.join("instances");
        let pending_root = instances_root.join("new-instance");
        std::fs::create_dir_all(&instances_root).unwrap();
        let pending =
            PendingInstanceDirectory::create(instances_root, pending_root.clone()).unwrap();

        std::fs::remove_dir(&pending_root).unwrap();
        std::fs::write(&pending_root, b"ownership changed").unwrap();
        let error = rollback_pending_instance_directory_blocking(
            pending,
            StorageError::InvalidPrivateRuntimeProjection {
                path: pending_root.join("runtime"),
                message: String::from("injected runtime preparation failure"),
            },
        );

        match error {
            StorageError::InstanceCreationRollback {
                path,
                creation_error,
                cleanup_error,
            } => {
                assert_eq!(path, pending_root);
                assert!(creation_error.contains("injected runtime preparation failure"));
                assert!(cleanup_error.contains("refusing to manage destructive operation"));
            }
            unexpected => panic!("unexpected rollback result: {unexpected}"),
        }
        assert_eq!(std::fs::read(&pending_root).unwrap(), b"ownership changed");

        std::fs::remove_file(&pending_root).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, windows))]
mod runtime_copy_tests {
    use super::{PackageTree, RuntimeCopyContext, StorageError, copy_runtime_directory};
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn full_runtime_copy_rejects_directory_junctions() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-copy-reparse-{}",
            uuid::Uuid::new_v4()
        ));
        let shared_root = root.join("shared");
        let external_root = root.join("external");
        let junction = shared_root.join("external-junction");
        let destination = root.join("destination");
        fs::create_dir_all(&shared_root).unwrap();
        fs::create_dir_all(&external_root).unwrap();
        fs::write(external_root.join("sentinel.bin"), b"outside runtime").unwrap();

        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&external_root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to create test junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let error = copy_runtime_directory(
            &shared_root,
            &destination,
            Path::new(""),
            &mut RuntimeCopyContext {
                excluded_paths: &[],
                expected: None,
                package: Some(PackageTree::default()),
                copied_paths: Vec::new(),
                cancellation: None,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            StorageError::UnsafeManagedPath { ref path, ref root }
                if path == &junction && root == &shared_root
        ));
        assert!(!destination.join("external-junction/sentinel.bin").exists());
        assert_eq!(
            fs::read(external_root.join("sentinel.bin")).unwrap(),
            b"outside runtime"
        );

        fs::remove_dir(&junction).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "program_adoption_tests.rs"]
mod program_adoption_tests;

pub(crate) fn effective_instance_install_root(
    record: &StoredInstanceRecord,
) -> Result<PathBuf, StorageError> {
    let instance_root = record
        .config_dir
        .parent()
        .unwrap_or(record.config_dir.as_path());
    crate::workshop_collection_removal::ensure_no_pending(instance_root)?;
    let expected_mode = match record.runtime_mode.as_str() {
        "shared" => InstanceProgramMode::Shared,
        "independent" => InstanceProgramMode::Independent,
        _ => {
            return Err(crate::program_runtime::invalid(
                instance_root,
                "unknown registered program mode",
            ));
        }
    };
    if crate::program_runtime::instance_program_mode(instance_root)? != expected_mode {
        return Err(crate::program_runtime::invalid(
            instance_root,
            "program files and registered ownership disagree; recover the installation before use",
        ));
    }
    let root = crate::program_runtime::resolve_instance_runtime_root(instance_root)?;
    let registered = record.program_install_root.as_ref().ok_or_else(|| {
        crate::program_runtime::invalid(
            instance_root,
            "instance program ownership is missing or inconsistent",
        )
    })?;
    if crate::instance_isolation::paths::normalize_path(registered)?
        != crate::instance_isolation::paths::normalize_path(&root)?
    {
        return Err(crate::program_runtime::invalid(
            &root,
            "program path does not match its registered installation",
        ));
    }
    Ok(root)
}

/// DST stores downloaded Workshop payloads beside packaged files in `mods`.
/// The numeric directories belong to an existing server, while other files
/// in that directory can be part of the game package.
fn dst_workshop_mod_paths(shared_root: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mods_root = shared_root.join("mods");
    let metadata = match fs::symlink_metadata(&mods_root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: mods_root,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || is_reparse_point(&mods_root)? {
        return Err(StorageError::UnsafeManagedPath {
            path: mods_root,
            root: shared_root.to_path_buf(),
        });
    }
    let mut excluded = Vec::new();
    for entry in fs::read_dir(&mods_root).map_err(|source| StorageError::ReadDirectory {
        path: mods_root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: mods_root.clone(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name
            .get(.."workshop-".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("workshop-"))
            && name
                .get("workshop-".len()..)
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        {
            excluded.push(entry.path());
        }
    }
    Ok(excluded)
}
#[path = "instance_program_copy.rs"]
mod program_copy;
pub(crate) use program_copy::prepare_private_runtime_root;
#[cfg(all(test, windows))]
use program_copy::{RuntimeCopyContext, copy_runtime_directory};

pub(crate) fn validate_managed_instance_root(
    instance_root: &Path,
    instances_root: &Path,
) -> Result<(), StorageError> {
    let relative = instance_root.strip_prefix(instances_root).map_err(|_| {
        StorageError::UnsafeManagedPath {
            path: instance_root.to_path_buf(),
            root: instances_root.to_path_buf(),
        }
    })?;
    let mut components = relative.components();
    let Some(first_component) = components.next() else {
        return Err(StorageError::UnsafeManagedPath {
            path: instance_root.to_path_buf(),
            root: instances_root.to_path_buf(),
        });
    };

    if components.next().is_some()
        || matches!(
            first_component,
            std::path::Component::CurDir
                | std::path::Component::ParentDir
                | std::path::Component::Prefix(_)
                | std::path::Component::RootDir
        )
        || first_component.as_os_str() == std::ffi::OsStr::new(".trash")
    {
        return Err(StorageError::UnsafeManagedPath {
            path: instance_root.to_path_buf(),
            root: instances_root.to_path_buf(),
        });
    }

    Ok(())
}

#[cfg(test)]
#[path = "instance_creation_copy_tests.rs"]
mod creation_copy_tests;

#[cfg(test)]
#[path = "instance_creation_source_tests.rs"]
mod creation_source_tests;
