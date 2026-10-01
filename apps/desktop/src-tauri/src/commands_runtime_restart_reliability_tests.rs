use super::*;
use crate::state::RuntimeRestartScheduler;

async fn set_backoff(fixture: &Fixture, milliseconds: u64) {
    let details = read_instance_details(&fixture.storage.paths, &fixture.instance_id)
        .await
        .unwrap();
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["runtime_restart"]["backoff_ms"] = json!(milliseconds);
    update_instance(
        &fixture.storage.paths,
        UpdateInstanceInput {
            id: fixture.instance_id.clone(),
            bind_ip: details.summary.bind_ip,
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            ports: details.ports,
            settings_json: settings.to_string(),
        },
    )
    .await
    .unwrap();
}

fn request(candidate: &RuntimeRestartCandidate) -> RuntimeRestartScheduleRequest {
    RuntimeRestartScheduleRequest {
        instance_id: candidate.instance_id.clone(),
        instance_name: candidate.instance_name.clone(),
        backoff: Duration::from_millis(candidate.policy.backoff_ms),
        recent_crash_count: candidate.recent_crash_count,
        exit_code: candidate.exit_code,
    }
}

#[tokio::test]
async fn reliability_restart_limit_survives_years_of_virtual_time_and_duplicate_exits() {
    let epoch = Instant::now();
    for limit in [1, 3, 10] {
        let fixture = Fixture::new(limit, true).await;
        set_backoff(&fixture, 300_000).await;
        let mut scheduler = RuntimeRestartScheduler::default();
        let mut restarts = 0;
        for failed_session in 1..=limit + 1 {
            let run = fixture
                .process(&format!("failed-{failed_session}"), "master")
                .await;
            fixture.exit(run, 7, true).await;
            // Each crash is one virtual year apart; elapsed time cannot reset the
            // persisted consecutive-failure limit or make duplicate events count twice.
            let now = epoch + Duration::from_secs(failed_session as u64 * 365 * 24 * 60 * 60);
            let candidate = fixture.candidate(false, 7).await;
            if failed_session > limit {
                assert!(
                    candidate.is_none(),
                    "limit {limit} must block session {failed_session}"
                );
                assert!(fixture.candidate(false, 7).await.is_none());
                assert!(scheduler.take_due_at(now).is_empty());
                continue;
            }
            let candidate = candidate.unwrap();
            assert_eq!(candidate.recent_crash_count, failed_session);
            assert_eq!(candidate.policy.backoff_ms, 300_000);
            let scheduled = scheduler.schedule_at(request(&candidate), now).unwrap();
            let duplicate = fixture.candidate(false, 7).await.unwrap();
            assert_eq!(duplicate.recent_crash_count, failed_session);
            assert!(
                scheduler
                    .schedule_at(request(&duplicate), now + Duration::from_secs(60))
                    .is_none()
            );
            assert!(
                scheduler
                    .take_due_at(scheduled.due_at - Duration::from_nanos(1))
                    .is_empty()
            );
            let due = scheduler.take_due_at(scheduled.due_at);
            assert_eq!(due.len(), 1);
            assert!(scheduler.entry_is_current(&due[0]));
            restarts += due.len();
            scheduler.finish_restart(&due[0]);
            assert!(scheduler.take_due_at(scheduled.due_at).is_empty());
        }
        assert_eq!(restarts, limit);
        assert_eq!(scheduler.pending_count(), 0);
        // A confirmed clean manual session is the existing reset boundary.
        let clean = fixture.process("clean-manual", "master").await;
        fixture.exit(clean, 0, false).await;
        let next = fixture.process("after-clean", "master").await;
        fixture.exit(next, 7, true).await;
        assert_eq!(
            fixture
                .candidate(false, 7)
                .await
                .unwrap()
                .recent_crash_count,
            1
        );
    }
}
