use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn job(id: &str, use_query_capacity: bool) -> CountJob {
    CountJob {
        source: CountSource::PlayerList {
            instance_id: id.into(),
        },
        use_query_capacity,
    }
}

#[tokio::test]
async fn successful_zero_is_counted_but_failed_query_is_not_a_zero_result() {
    let mut snapshot = GlobalPlayerCountSnapshot {
        total_player_capacity: 80,
        queryable_instances: 3,
        ..Default::default()
    };
    collect_jobs(
        &mut snapshot,
        VecDeque::from([
            job("zero", false),
            job("occupied", true),
            job("unavailable", false),
        ]),
        |job| async move {
            let CountSource::PlayerList { instance_id } = job.source else {
                unreachable!()
            };
            let current_players = match instance_id.as_str() {
                "zero" => 0,
                "occupied" => 3,
                _ => return None,
            };
            Some(CountResult {
                current_players,
                max_players: Some(10),
                use_query_capacity: job.use_query_capacity,
            })
        },
    )
    .await;
    assert_eq!(snapshot.total_online_players, 3);
    assert_eq!(snapshot.queried_instances, 2);
    assert_eq!(snapshot.queryable_instances, 3);
    assert_eq!(snapshot.total_player_capacity, 90);
}

#[tokio::test]
async fn failed_collection_does_not_count_as_a_success_or_invent_capacity() {
    let mut snapshot = GlobalPlayerCountSnapshot {
        queryable_instances: 2,
        ..Default::default()
    };
    collect_jobs(
        &mut snapshot,
        VecDeque::from([job("failed", true), job("no_maximum", true)]),
        |job| async move {
            let CountSource::PlayerList { instance_id } = job.source else {
                unreachable!()
            };
            (instance_id == "no_maximum").then_some(CountResult {
                current_players: 2,
                max_players: None,
                use_query_capacity: job.use_query_capacity,
            })
        },
    )
    .await;
    assert_eq!(snapshot.total_online_players, 2);
    assert_eq!(snapshot.queried_instances, 1);
    assert_eq!(snapshot.total_player_capacity, 0);
}

#[tokio::test]
async fn combined_query_sources_share_the_same_four_request_limit() {
    let started = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let (sender, mut receiver) = tokio::sync::mpsc::channel(12);
    let mut jobs = VecDeque::new();
    for index in 0..12 {
        jobs.push_back(CountJob {
            source: if index % 2 == 0 {
                CountSource::PlayerList {
                    instance_id: index.to_string(),
                }
            } else {
                CountSource::PlayerQuery {
                    protocol: "a2s_info".into(),
                    host: "127.0.0.1".into(),
                    port: 27015,
                }
            },
            use_query_capacity: false,
        });
    }
    let worker = tokio::spawn({
        let active = active.clone();
        let peak = peak.clone();
        let release = release.clone();
        let started = started.clone();
        async move {
            let mut snapshot = GlobalPlayerCountSnapshot::default();
            collect_jobs(&mut snapshot, jobs, move |_| {
                let active = active.clone();
                let peak = peak.clone();
                let release = release.clone();
                let sender = sender.clone();
                let started = started.clone();
                async move {
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    started.fetch_add(1, Ordering::SeqCst);
                    sender.send(()).await.unwrap();
                    release.acquire_owned().await.unwrap().forget();
                    active.fetch_sub(1, Ordering::SeqCst);
                    Some(CountResult {
                        current_players: 1,
                        max_players: None,
                        use_query_capacity: false,
                    })
                }
            })
            .await;
            snapshot
        }
    });
    for _ in 0..QUERY_CONCURRENCY {
        tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(started.load(Ordering::SeqCst), QUERY_CONCURRENCY);
    assert_eq!(active.load(Ordering::SeqCst), QUERY_CONCURRENCY);
    release.add_permits(12);
    let snapshot = tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), QUERY_CONCURRENCY);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot.total_online_players, 12);
    assert_eq!(snapshot.queried_instances, 12);
}
