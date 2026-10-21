use super::*;
use crate::managed_console_log::ManagedConsoleLog;
use std::{fs, io::Write, path::PathBuf};

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    log: PathBuf,
}
impl Fixture {
    async fn new(managed: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-player-history-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("app"),
            settings_path: root.join("app/settings.json"),
            database_path: root.join("app/db/lgs.db"),
            logs_root: root.join("app/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        fs::create_dir_all(root.join("logs/managed-console")).unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let log = root.join(if managed {
            "logs/managed-console/run-master.log"
        } else {
            "logs/master.log"
        });
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO modules(id,name,version) VALUES('dontstarve','DST','1')")
            .execute(&pool)
            .await
            .unwrap();
        for instance in ["a", "b"] {
            sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES(?,?,'dontstarve','data','config',?,'saves')")
                .bind(instance).bind(instance).bind(root.join("logs").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO instance_runs(id,instance_id,status,process_key,started_at,log_path) VALUES(1,'a','running','master','2026-10-18 00:00:00',?)")
            .bind(log.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        pool.close().await;
        Self { root, paths, log }
    }
    fn append(&self, text: &str) {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    async fn page(
        &self,
        instance: &str,
        kind: Option<&str>,
        before: Option<i64>,
        limit: u32,
    ) -> PlayerHistoryPage {
        query_player_history(
            &self.paths,
            PlayerHistoryQuery {
                instance_id: instance.into(),
                kind: kind.map(str::to_owned),
                search: None,
                before_id: before,
                since_unix_ms: None,
                until_unix_ms: None,
                limit: Some(limit),
            },
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[tokio::test]
async fn durable_collection_handles_partial_lines_replay_paging_filters_and_instance_isolation() {
    let fixture = Fixture::new(false).await;
    fixture
        .append("[00:00:10]: [Join Announcement] 玩家 A\n[00:00:20]: [Say] (KU_test) 玩家 A: 你");
    collect_player_history(&fixture.paths).await.unwrap();
    assert_eq!(fixture.page("a", None, None, 50).await.events.len(), 1);
    fixture.append("好\n[00:01:10]: [Leave Announcement] 玩家 A\n");
    collect_player_history(&fixture.paths).await.unwrap();
    collect_player_history(&fixture.paths).await.unwrap();
    // Reopen pools from persisted paths; no in-memory registry supplies history.
    let page = fixture.page("a", None, None, 2).await;
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].kind, "leave");
    assert_eq!(page.events[0].duration_ms, Some(60_000));
    assert_eq!(page.events[1].message.as_deref(), Some("你好"));
    assert_eq!(page.events[1].account_id.as_deref(), Some("KU_test"));
    let second = fixture.page("a", None, page.next_before_id, 2).await;
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].kind, "join");
    assert_eq!(second.events[0].duration_ms, Some(60_000));
    assert!(second.events[0].account_id.is_none());
    assert!(second.next_before_id.is_none());
    assert!(fixture.page("b", None, None, 50).await.events.is_empty());
    let filtered = query_player_history(
        &fixture.paths,
        PlayerHistoryQuery {
            instance_id: "a".into(),
            kind: Some("chat".into()),
            search: Some("KU_test".into()),
            before_id: None,
            since_unix_ms: Some(1_792_281_600_000),
            until_unix_ms: None,
            limit: Some(50),
        },
    )
    .await
    .unwrap();
    assert_eq!(filtered.events.len(), 1);
    assert_eq!(
        fixture
            .page("a", Some("leave"), None, 50)
            .await
            .events
            .len(),
        1
    );
}

#[tokio::test]
async fn ambiguous_names_unmatched_departures_and_clock_regressions_never_invent_durations() {
    let fixture = Fixture::new(false).await;
    fixture.append("[00:00:10]: [Join Announcement] Same\n[00:00:11]: [Join Announcement] Same\n[00:00:20]: [Leave Announcement] Same\n[00:00:30]: [Leave Announcement] Unknown\n[00:00:02]: [Join Announcement] Reset\n[00:00:05]: [Leave Announcement] Reset\n");
    collect_player_history(&fixture.paths).await.unwrap();
    let page = fixture.page("a", None, None, 50).await;
    assert_eq!(page.events.len(), 6);
    assert!(page.events.iter().all(|event| event.duration_ms.is_none()));
    assert!(page.events[0].estimated_at_unix_ms.is_none());
    assert_eq!(
        page.collection_issue.as_deref(),
        Some("clock_discontinuity")
    );
}

#[tokio::test]
async fn truncation_keeps_saved_history_and_does_not_pair_across_generations() {
    let fixture = Fixture::new(false).await;
    fixture.append(
        "[00:00:10]: [Join Announcement] A\n[00:00:11]: [Say] (KU_a) A: long old message\n",
    );
    collect_player_history(&fixture.paths).await.unwrap();
    fs::write(&fixture.log, "[00:00:30]: [Leave Announcement] A\n").unwrap();
    collect_player_history(&fixture.paths).await.unwrap();
    collect_player_history(&fixture.paths).await.unwrap();
    let page = fixture.page("a", None, None, 50).await;
    assert_eq!(page.events.len(), 3);
    assert!(page.events[0].duration_ms.is_none());
    assert_eq!(page.collection_issue.as_deref(), Some("log_gap"));
}

#[tokio::test]
async fn managed_rotation_reads_retained_segments_once_and_preserves_split_lines() {
    let fixture = Fixture::new(true).await;
    let log = ManagedConsoleLog::open(&fixture.log).unwrap();
    let join = "[00:00:10]: [Join Announcement] A\n";
    log.write_all(join.as_bytes()).unwrap();
    let padding = "ignored\n".repeat(1024);
    // Cross the real 8 MiB segment boundary while retaining the earlier join.
    for _ in 0..1024 {
        log.write_all(padding.as_bytes()).unwrap();
    }
    log.write_all(b"[00:01:10]: [Leave Announcement] A\n")
        .unwrap();
    for _ in 0..34 {
        collect_player_history(&fixture.paths).await.unwrap();
    }
    let page = fixture.page("a", None, None, 50).await;
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].duration_ms, Some(60_000));
    assert!(page.collection_issue.is_none());
}
