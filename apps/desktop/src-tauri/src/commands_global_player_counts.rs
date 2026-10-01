use super::*;
use app_core::ModulePlayerCountSource;
use std::collections::VecDeque;
use std::future::Future;

use super::commands_player_counts::collect_player_list_count;
use super::commands_runtime_observability::resolve_player_query_target;

const QUERY_CONCURRENCY: usize = 4;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct GlobalPlayerCountSnapshot {
    pub(super) total_online_players: usize,
    pub(super) total_player_capacity: usize,
    pub(super) queried_instances: usize,
    pub(super) queryable_instances: usize,
}

#[derive(Debug)]
enum CountSource {
    PlayerQuery {
        protocol: String,
        host: String,
        port: u16,
    },
    PlayerList {
        instance_id: String,
    },
}

#[derive(Debug)]
struct CountJob {
    source: CountSource,
    use_query_capacity: bool,
}

struct CountResult {
    current_players: usize,
    max_players: Option<usize>,
    use_query_capacity: bool,
}

pub(super) async fn collect_global_player_counts<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    instances: &[InstanceSummary],
) -> Result<GlobalPlayerCountSnapshot, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let runtime_by_module = load_module_runtime_capability_map(&storage.paths.modules_root)?;
    let mut snapshot = GlobalPlayerCountSnapshot::default();
    let mut jobs = VecDeque::new();

    for instance in instances
        .iter()
        .filter(|instance| matches!(instance.status, InstanceStatus::Running))
    {
        let Ok(details) = read_instance_details(&storage.paths, &instance.id).await else {
            continue;
        };
        if !matches!(details.summary.status, InstanceStatus::Running) {
            continue;
        }
        let configured_capacity = extract_instance_player_capacity(&details.settings_json);
        snapshot.total_player_capacity += configured_capacity.unwrap_or(0);
        let Some(runtime) = runtime_by_module.get(&details.summary.module_id) else {
            continue;
        };
        let source = match runtime.player_count_source {
            ModulePlayerCountSource::PlayerList => CountSource::PlayerList {
                instance_id: details.summary.id,
            },
            ModulePlayerCountSource::PlayerQuery => {
                let Some((host, port)) =
                    resolve_player_query_target(&details, runtime.player_query.as_ref())
                else {
                    continue;
                };
                let Some(query) = runtime.player_query.as_ref() else {
                    continue;
                };
                CountSource::PlayerQuery {
                    protocol: query.protocol.clone(),
                    host,
                    port,
                }
            }
        };
        jobs.push_back(CountJob {
            source,
            use_query_capacity: configured_capacity.is_none(),
        });
    }
    snapshot.queryable_instances = jobs.len();
    let app = app.clone();
    collect_jobs(&mut snapshot, jobs, move |job| {
        let app = app.clone();
        async move { collect_job(app, job).await }
    })
    .await;
    Ok(snapshot)
}

async fn collect_job<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    job: CountJob,
) -> Option<CountResult> {
    let (current_players, max_players) = match job.source {
        CountSource::PlayerQuery {
            protocol,
            host,
            port,
        } => {
            let queried = tokio::task::spawn_blocking(move || {
                app_storage::query_live_player_count(&protocol, &host, port)
            })
            .await
            .ok()
            .flatten()?;
            (
                queried.current_players,
                (queried.max_players > 0).then_some(queried.max_players),
            )
        }
        CountSource::PlayerList { instance_id } => {
            let state = app.state::<DesktopState>();
            let snapshot = collect_player_list_count(&state, &instance_id).await.ok()?;
            (snapshot.current_players?, snapshot.max_players)
        }
    };
    Some(CountResult {
        current_players,
        max_players,
        use_query_capacity: job.use_query_capacity,
    })
}

async fn collect_jobs<Collector, Collected>(
    snapshot: &mut GlobalPlayerCountSnapshot,
    mut jobs: VecDeque<CountJob>,
    mut collect: Collector,
) where
    Collector: FnMut(CountJob) -> Collected,
    Collected: Future<Output = Option<CountResult>> + Send + 'static,
{
    let mut active = tokio::task::JoinSet::new();
    while active.len() < QUERY_CONCURRENCY {
        let Some(job) = jobs.pop_front() else {
            break;
        };
        active.spawn(collect(job));
    }
    while let Some(result) = active.join_next().await {
        if let Some(job) = jobs.pop_front() {
            active.spawn(collect(job));
        }
        if let Ok(Some(result)) = result {
            snapshot.total_online_players += result.current_players;
            snapshot.queried_instances += 1;
            if result.use_query_capacity {
                snapshot.total_player_capacity += result.max_players.unwrap_or(0);
            }
        }
    }
}

#[cfg(test)]
#[path = "commands_global_player_counts_tests.rs"]
mod tests;
