use super::*;
use crate::live_players::cache::CachedLivePlayerSnapshot;
use crate::live_players::service::build_cache_key;
use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};
use std::sync::atomic::{AtomicUsize, Ordering};

const PLAYER_KEY: &str = "fixture-player";
const ACTION: &str = "kick_userid";
const USERID: &str = "KU_1234567890";

fn roster(
    instance_id: &str,
    snapshot_id: &str,
    at: u64,
    with_player: bool,
) -> CachedLivePlayerSnapshot {
    let entries = if with_player {
        vec![RuntimeLivePlayerEntry {
            player_key: PLAYER_KEY.into(),
            display_name: "Fixture player".into(),
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::KleiUserId,
                value: USERID.into(),
                stable: true,
            }],
            available_action_ids: vec![ACTION.into()],
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: None,
            attributes: Vec::new(),
        }]
    } else {
        Vec::new()
    };
    let private_action_bindings = if with_player {
        HashMap::from([((PLAYER_KEY.into(), ACTION.into()), USERID.into())])
    } else {
        HashMap::new()
    };
    CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: snapshot_id.into(),
            instance_id: instance_id.into(),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::StructuredLog),
            observed_at_unix_ms: Some(at),
            expires_at_unix_ms: None,
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(entries.len()),
            max_players: Some(6),
            entries,
            issue: None,
        },
        private_action_bindings,
        collected_at: at,
    }
}

#[tokio::test]
async fn dst_confirmed_stop_revokes_player_roster_and_allows_fresh_run() {
    let mut fixture = Fixture::new("normal").await;
    fixture.ready().await.unwrap();
    let active = fixture.register().await;
    let instance_id = fixture.details.summary.id.clone();
    let descriptors = discover_modules(&fixture.storage.paths.modules_root).unwrap();
    let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
    let mut running = fixture.details.clone();
    running.active_run = Some(active.clone());
    let old_key = build_cache_key(&running, descriptor).unwrap();
    let registry = fixture.state.live_player_registry.clone();
    let now = registry.now_unix_ms();
    registry
        .refresh_if_expired(old_key.clone(), now, || async {
            Ok(roster(&instance_id, "old-roster", now, true))
        })
        .await;
    assert_eq!(
        registry.resolve_action_binding(&old_key, "old-roster", PLAYER_KEY, ACTION),
        Some(USERID.into())
    );

    fixture.shutdown().await.unwrap();
    let stopped = finish_confirmed_stop(&fixture.state, &fixture.storage, &instance_id, &active)
        .await
        .unwrap();
    assert_eq!(stopped[0].exit_code, Some(0));
    assert!(registry.read(&old_key).is_none());
    assert!(
        registry
            .resolve_action_binding(&old_key, "old-roster", PLAYER_KEY, ACTION)
            .is_none()
    );

    // Start another real managed fixture process for the same instance. Its
    // new persisted run and process identity must reach the collector once.
    for (plan, process) in &mut fixture.processes {
        *process = spawn_launch_plan(&plan.launch_plan, &plan.log_path).unwrap();
    }
    fixture.ready().await.unwrap();
    let next_active = fixture.register().await;
    running.active_run = Some(next_active.clone());
    let next_key = build_cache_key(&running, descriptor).unwrap();
    assert_ne!(old_key.run_id, next_key.run_id);
    let collected = AtomicUsize::new(0);
    let now = registry.now_unix_ms();
    let snapshot = registry
        .refresh_if_expired(next_key.clone(), now, || async {
            collected.fetch_add(1, Ordering::SeqCst);
            Ok(roster(&instance_id, "new-roster", now, false))
        })
        .await;
    assert_eq!(collected.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Ready);
    assert!(snapshot.complete && !snapshot.stale && !snapshot.truncated);
    assert_eq!(snapshot.current_players, Some(0));
    assert!(registry.read(&old_key).is_none());
    assert!(
        registry
            .resolve_action_binding(&old_key, "old-roster", PLAYER_KEY, ACTION)
            .is_none()
    );

    fixture.shutdown().await.unwrap();
    finish_confirmed_stop(&fixture.state, &fixture.storage, &instance_id, &next_active)
        .await
        .unwrap();
}
