use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use app_core::{InstanceDetails, InstanceProcessState};
use base64::Engine;
use serde::Deserialize;

use super::commands_stdin_dispatch::dispatch_managed_stdin_command;
use crate::runtime_log_stream::{read_runtime_log_delta_bounded, runtime_log_tail_at_end};
use crate::state::DesktopState;

#[path = "dst_map_protocol.rs"]
mod protocol;
pub use protocol::DstMapSnapshot;

// Stable command-error prefixes survive both Tauri rejection and LAN transport.
// Classification is assigned at its source, never inferred from game text.
#[derive(Clone, Copy)]
enum MapErrorCode {
    Busy,
    ShardNotRunning,
    ShardNotEnabled,
    ShardChanged,
    ConsoleUnavailable,
    WorldNotReady,
    GeometryChanged,
    ResponseTimeout,
    ArtworkWorkerFailed,
}

fn map_error(code: MapErrorCode, message: impl AsRef<str>) -> String {
    let code = match code {
        MapErrorCode::Busy => "BUSY",
        MapErrorCode::ShardNotRunning => "SHARD_NOT_RUNNING",
        MapErrorCode::ShardNotEnabled => "SHARD_NOT_ENABLED",
        MapErrorCode::ShardChanged => "SHARD_CHANGED",
        MapErrorCode::ConsoleUnavailable => "CONSOLE_UNAVAILABLE",
        MapErrorCode::WorldNotReady => "WORLD_NOT_READY",
        MapErrorCode::GeometryChanged => "GEOMETRY_CHANGED",
        MapErrorCode::ResponseTimeout => "RESPONSE_TIMEOUT",
        MapErrorCode::ArtworkWorkerFailed => "ARTWORK_WORKER_FAILED",
    };
    let message: String = message
        .as_ref()
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect();
    format!("DST_MAP_{code}: {message}")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DstMapInput {
    pub instance_id: String,
    pub shard: String,
    pub include_terrain: bool,
}

pub(super) fn collection_permit() -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            map_error(
                MapErrorCode::Busy,
                "World map collection is busy. Try again shortly.",
            )
        })
}

// Both map reads and artwork imports collect the same internal engine evidence.
// The caller owns the storage-context lease and bounded collection permit.
pub(super) async fn collect_native_map(
    state: &DesktopState,
    input: &DstMapInput,
) -> Result<(DstMapSnapshot, InstanceDetails), String> {
    let mutation = state.acquire_instance_mutation(&input.instance_id).await;
    let storage = state.bootstrap_storage()?;
    let details = app_storage::read_instance_details(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())?;
    validate_target(&details, &input.shard)?;
    let run = details.active_run.as_ref().ok_or_else(|| {
        map_error(
            MapErrorCode::ShardNotRunning,
            "Start the shard before reading its world map.",
        )
    })?;
    let process = run
        .processes
        .iter()
        .find(|p| p.process_key == input.shard && p.status == "running")
        .ok_or_else(|| {
            map_error(
                MapErrorCode::ShardNotRunning,
                "The selected shard is not running.",
            )
        })?;
    let path = PathBuf::from(process.log_path.as_deref().ok_or_else(|| {
        map_error(
            MapErrorCode::ConsoleUnavailable,
            "The selected shard has no console log.",
        )
    })?);
    verify_process(state, &input.instance_id, process)?;
    let snapshot = collect(state, input, run.run_id, process, path, mutation).await?;
    Ok((snapshot, details))
}

pub(super) fn verify_snapshot_process(
    state: &DesktopState,
    snapshot: &DstMapSnapshot,
    details: &InstanceDetails,
) -> Result<(), String> {
    let run = details
        .active_run
        .as_ref()
        .filter(|run| run.run_id == snapshot.run_id)
        .ok_or_else(|| {
            map_error(
                MapErrorCode::ShardChanged,
                "The shard process changed while collecting its map.",
            )
        })?;
    let process = run
        .processes
        .iter()
        .find(|p| p.process_key == snapshot.shard && p.status == "running")
        .ok_or_else(|| {
            map_error(
                MapErrorCode::ShardNotRunning,
                "The selected shard is no longer running.",
            )
        })?;
    verify_process(state, &snapshot.instance_id, process)
}

#[tauri::command]
pub async fn read_dontstarve_world_map(
    state: tauri::State<'_, DesktopState>,
    input: DstMapInput,
) -> Result<DstMapSnapshot, String> {
    let operation = state.begin_storage_context_operation("DST world map")?;
    let permit = collection_permit()?;
    let (mut snapshot, details) = collect_native_map(&state, &input).await?;
    if input.include_terrain {
        let storage = state.bootstrap_storage()?;
        let install =
            app_storage::read_instance_program_install(&storage.paths, &input.instance_id)
                .await
                .map_err(|error| error.to_string())?;
        let instance_root = Path::new(&details.config_file_path)
            .parent()
            .and_then(Path::parent)
            .ok_or("Cannot locate this instance's Mod directory.")?
            .to_path_buf();
        let shard_directory = app_core::dst_shards::DST_SHARDS
            .iter()
            .find(|spec| spec.process_key == input.shard)
            .ok_or("Unknown native shard role.")?
            .directory;
        snapshot = crate::state::spawn_blocking_storage_context_task(&operation, move || {
            let _permit = permit;
            let enabled_mods = std::mem::take(&mut snapshot.enabled_mods);
            let registered_atlases = std::mem::take(&mut snapshot.registered_atlases);
            let collection_issues = std::mem::take(&mut snapshot.collection_issues);
            let textures: Vec<_> = snapshot
                .tile_textures
                .iter()
                .filter(|(id, _)| !snapshot.tile_colors.contains_key(*id))
                .map(|(_, path)| path.clone())
                .chain(
                    protocol::final_terrain_layers(&snapshot)
                        .filter(|layer| layer.sample_style.is_none_or(|style| style == 0))
                        .filter(|layer| protocol::uses_texture_material(&snapshot, layer.tile_id))
                        .filter_map(|layer| layer.noise_texture.clone()),
                )
                .chain(std::iter::once("images/minimap_paper.tex".to_string()))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let icons: Vec<_> = snapshot
                .landmarks
                .iter()
                .flatten()
                .map(|item| item.icon.clone())
                .chain(
                    snapshot
                        .players
                        .iter()
                        .filter_map(|player| player.icon.clone()),
                )
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            // A bad atlas may have claimed any icon before later registrations.
            // Request no icons, including indexed completions, in this case.
            let icons = if snapshot.icons_uncertain {
                Vec::new()
            } else {
                icons
            };
            let base_paths: Vec<_> = snapshot
                .tile_base_textures
                .values()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let edge_requests = terrain_edge_requests(&snapshot);
            let entity_inventory = entity_inventory_icon_sources(&snapshot);
            let result = install
                .ok_or_else(|| "The instance has no installed map artwork.".to_string())
                .and_then(|binding| {
                    let sources = map_mod_sources(
                        &binding.install.install_root,
                        &instance_root,
                        shard_directory,
                        &enabled_mods,
                    )?;
                    app_modules::load_dst_map_assets(
                        &binding.install.install_root,
                        &sources,
                        app_modules::DstMapIconSources {
                            registered_atlases: &registered_atlases,
                            entity_inventory: &entity_inventory,
                        },
                        &textures,
                        &base_paths,
                        &icons,
                        &edge_requests,
                    )
                });
            let mut art = match result {
                Ok(art) => protocol::DstMapArt {
                    textures: encode_art(art.textures),
                    icons: encode_art(art.icons),
                    icon_fallbacks: art.icon_fallbacks,
                    issues: art.issues,
                    terrain_base_color: art.terrain_base_color,
                    terrain_base_colors: art.terrain_base_colors,
                    terrain_edges: art
                        .terrain_edges
                        .into_iter()
                        .map(|edge| protocol::DstMapTerrainEdge {
                            atlas: edge.atlas,
                            texture: edge.texture,
                            image: format!(
                                "data:image/png;base64,{}",
                                base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    edge.image
                                )
                            ),
                            opaque: edge.opaque,
                            samples: edge.samples,
                        })
                        .collect(),
                },
                Err(error) => protocol::DstMapArt {
                    issues: vec![error],
                    ..Default::default()
                },
            };
            art.issues.extend(collection_issues);
            art.issues
                .extend(protocol::terrain_material_issues(&snapshot));
            if art.issues.len() > 1024 {
                let omitted = art.issues.len() - 1023;
                art.issues.truncate(1023);
                art.issues.push(format!(
                    "{omitted} additional map artwork issues were omitted."
                ));
            }
            snapshot.art = Some(art);
            snapshot
        })
        .await
        .map_err(|error| {
            map_error(
                MapErrorCode::ArtworkWorkerFailed,
                format!("Map artwork worker failed: {error}"),
            )
        })?;
        verify_snapshot_process(&state, &snapshot, &details)?;
        protocol::constrain_response(&mut snapshot)?;
    }
    Ok(snapshot)
}

fn entity_inventory_icon_sources(
    snapshot: &DstMapSnapshot,
) -> Vec<app_modules::DstMapEntityInventoryIcon> {
    if snapshot.landmarks_truncated || snapshot.icons_uncertain {
        return Vec::new();
    }
    let avatars: std::collections::BTreeSet<_> = snapshot
        .players
        .iter()
        .filter_map(|player| player.icon.as_ref())
        .map(|icon| icon.to_ascii_lowercase())
        .collect();
    let mut groups =
        std::collections::BTreeMap::<String, Option<&protocol::DstMapInventoryIcon>>::new();
    for landmark in snapshot.landmarks.iter().flatten() {
        let key = landmark.icon.to_ascii_lowercase();
        let declared = landmark.inventory_icon.as_ref();
        groups.entry(key).and_modify(|prior| {
            if !matches!((*prior, declared), (Some(a),Some(b)) if a.atlas == b.atlas && a.element.eq_ignore_ascii_case(&b.element)) {
                *prior = None;
            }
        }).or_insert(declared);
    }
    groups
        .into_iter()
        .filter(|(icon, _)| !avatars.contains(icon))
        .filter_map(|(icon, source)| {
            source.map(|source| app_modules::DstMapEntityInventoryIcon {
                icon,
                atlas: source.atlas.clone(),
                element: source.element.clone(),
            })
        })
        .collect()
}

fn map_mod_sources(
    install: &Path,
    instance: &Path,
    shard: &str,
    enabled: &[String],
) -> Result<Vec<app_modules::DstMapModSource>, String> {
    let mut sources = Vec::new();
    for name in enabled {
        crate::dst_mods::validate_dst_mod_directory_name(name)?;
        let local = install.join("mods").join(name);
        let workshop = name.strip_prefix("workshop-").map(|id| {
            instance
                .join("data/ugc")
                .join(shard)
                .join("content/322330")
                .join(id)
        });
        // Only the selected shard's deployed copy and its owned runtime count.
        // Duplicate roots remain explicit so the decoder rejects ambiguity.
        if std::fs::symlink_metadata(&local).is_ok() {
            sources.push(app_modules::DstMapModSource {
                name: name.clone(),
                root: local,
            });
        }
        if let Some(root) = workshop
            && std::fs::symlink_metadata(&root).is_ok()
        {
            sources.push(app_modules::DstMapModSource {
                name: name.clone(),
                root,
            });
        }
    }
    Ok(sources)
}

fn terrain_edge_requests(snapshot: &protocol::DstMapSnapshot) -> Vec<(String, String)> {
    protocol::final_terrain_layers(snapshot)
        .filter(|layer| protocol::uses_texture_material(snapshot, layer.tile_id))
        .filter(|layer| {
            layer.sample_style.is_none_or(|style| style == 0) && layer.noise_texture.is_some()
        })
        .filter_map(|layer| Some((layer.atlas.clone()?, layer.texture.clone()?)))
        .collect()
}

fn encode_art(
    images: std::collections::BTreeMap<String, Vec<u8>>,
) -> std::collections::BTreeMap<String, String> {
    images
        .into_iter()
        .map(|(key, png)| {
            (
                key,
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                ),
            )
        })
        .collect()
}

fn validate_target(details: &InstanceDetails, shard: &str) -> Result<(), String> {
    if details.summary.module_id != "dontstarve" {
        return Err("World map collection is only available for Don't Starve Together.".into());
    }
    let settings = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("Invalid world settings: {error}"))?;
    if !app_core::dst_shards::dst_shards(&settings)?
        .iter()
        .any(|spec| spec.process_key == shard)
    {
        return Err(map_error(
            MapErrorCode::ShardNotEnabled,
            "The selected DST shard is not enabled.",
        ));
    }
    Ok(())
}

fn verify_process(
    state: &DesktopState,
    id: &str,
    process: &InstanceProcessState,
) -> Result<(), String> {
    let pid = process.pid.ok_or_else(|| {
        map_error(
            MapErrorCode::ShardNotRunning,
            "The selected shard has no running process.",
        )
    })?;
    let matches = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "Runtime supervisor is unavailable.")?
        // Every shard has its own run row; the session's primary run ID is
        // reserved for the public snapshot and the stdin dispatch precondition.
        .matches_running_process(id, process.run_id, &process.process_key, pid)
        .map_err(|error| error.to_string())?;
    if !matches {
        return Err(map_error(
            MapErrorCode::ShardChanged,
            "The shard process changed or is no longer managed by LGSM.",
        ));
    }
    Ok(())
}

async fn collect(
    state: &DesktopState,
    input: &DstMapInput,
    owner_run_id: i64,
    process: &InstanceProcessState,
    path: PathBuf,
    mutation: tokio::sync::OwnedMutexGuard<()>,
) -> Result<DstMapSnapshot, String> {
    let log_path = path.clone();
    let mut tail = tokio::task::spawn_blocking(move || runtime_log_tail_at_end(&log_path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| {
            map_error(
                MapErrorCode::ConsoleUnavailable,
                format!("Cannot locate the shard console: {error}"),
            )
        })?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let mut capture = protocol::Capture::new(&nonce, input.include_terrain);
    let deadline = Instant::now() + Duration::from_secs(25);
    let command = protocol::query(&nonce, input.include_terrain)?;
    dispatch_managed_stdin_command(
        state,
        &input.instance_id,
        Some(&input.shard),
        &command,
        Some(owner_run_id),
    )
    .await?;
    // No world mutation: stopping/saving the server must not wait for map I/O.
    drop(mutation);
    let mut total = 0usize;
    loop {
        verify_process(state, &input.instance_id, process)?;
        let log_path = path.clone();
        let (next, delta) = tokio::task::spawn_blocking(move || {
            let result =
                read_runtime_log_delta_bounded(&log_path, &mut tail, 256 * 1024, 16 * 1024);
            (tail, result)
        })
        .await
        .map_err(|error| format!("Map log reader failed: {error}"))?;
        tail = next;
        let delta = delta.map_err(|error| format!("Cannot read the shard console: {error}"))?;
        if let Some(error) = delta.stream_error {
            return Err(format!("Incomplete map console capture: {error}"));
        }
        total = total.saturating_add(delta.bytes_read);
        if total > protocol::MAX_CAPTURE_BYTES {
            return Err("World map console capture exceeded its limit.".into());
        }
        for line in delta.lines {
            if let Some(mut snapshot) = capture.push(&line)? {
                verify_process(state, &input.instance_id, process)?;
                snapshot.instance_id = input.instance_id.clone();
                snapshot.shard = input.shard.clone();
                snapshot.run_id = owner_run_id;
                snapshot.captured_at_unix_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "The host clock is before the Unix epoch.")?
                    .as_millis()
                    .try_into()
                    .map_err(|_| "The host timestamp is out of range.")?;
                return Ok(snapshot);
            }
        }
        if Instant::now() >= deadline {
            return Err(map_error(
                MapErrorCode::ResponseTimeout,
                "The shard did not return a complete world map before the deadline.",
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
#[path = "commands_dst_map_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dst_map_observer_tests.rs"]
mod observer_tests;

#[cfg(test)]
#[path = "dst_map_render_observer_tests.rs"]
mod render_observer_tests;
