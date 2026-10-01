use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn native_guide_and_mod_action_types_are_distinct() {
    assert_eq!(
        parse_file_type(
            "<button onClick=\"PublishedFileAward( '441378551', 9 )\">",
            "441378551"
        )
        .expect("guide type"),
        9
    );
    assert_eq!(
        parse_file_type(
            "<button onClick=\"PublishedFileAward( '2964299587', 0, 29 )\">",
            "2964299587"
        )
        .expect("mod type"),
        0
    );
    assert_eq!(item_kind(Some(9)), "guide");
    assert_eq!(item_kind(Some(0)), "item");
    assert_eq!(item_kind(Some(15)), "item");
    assert_eq!(item_kind(Some(2)), "collection");
    assert_eq!(item_kind(None), "unknown");
    assert_eq!(item_kind(Some(3)), "unsupported");
}

#[test]
fn description_text_and_another_items_action_do_not_authorize_download() {
    assert!(parse_file_type("<p>PublishedFileAward('441378551', 0)</p>", "441378551").is_err());
    assert!(
        parse_file_type(
            "<button onClick=\"PublishedFileAward('999999', 0)\">",
            "441378551"
        )
        .is_err()
    );
}

#[tokio::test]
async fn supplied_item_types_and_collection_children_do_not_require_community_html() {
    let current_id = "880000001";
    let child_id = "880000002";
    let collection_id = "880000003";
    remember_file_type(current_id, 0).expect("old mod type");
    // GetCollectionDetails returns filetype for each child; that evidence can
    // verify the child even if its separate public details omit file_type.
    remember_file_type(child_id, 15).expect("collection child type");
    let mut details = HashMap::from([
        (
            current_id.to_string(),
            serde_json::json!({"result": 1, "file_type": 9}),
        ),
        (child_id.to_string(), serde_json::json!({"result": 1})),
        (collection_id.to_string(), serde_json::json!({"result": 1})),
    ]);
    let collections = HashMap::from([(collection_id.to_string(), vec![child_id.to_string()])]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture proxy");
    let proxy = format!("http://{}", listener.local_addr().expect("proxy address"));
    let client = reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(proxy).expect("proxy"))
        .timeout(Duration::from_millis(100))
        .build()
        .expect("client");
    let errors = populate_file_types(
        &client,
        &mut details,
        &collections,
        app_network::SourcePreference::ChinaFirst,
        None,
    )
    .await
    .expect("metadata needs no network");
    assert!(errors.is_empty());
    let listener = listener.into_std().expect("fixture proxy socket");
    assert_eq!(
        listener
            .accept()
            .expect_err("no HTTP request needed")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(details[current_id]["file_type"], 9);
    assert_eq!(
        cached_file_type(current_id).expect("current cached type"),
        Some(9)
    );
    assert_eq!(details[child_id]["file_type"], 15);
    assert_eq!(details[collection_id]["file_type"], 2);
}

#[tokio::test]
async fn concurrent_duplicate_ids_share_the_same_failed_request() {
    let coordinator = TypeLookupCoordinator::new();
    let calls = AtomicUsize::new(0);
    let fetch = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        Err(String::from("Steam returned HTTP 429"))
    };
    let (first, second) = tokio::join!(
        coordinator.resolve("880000101", fetch),
        coordinator.resolve("880000101", fetch)
    );
    assert_eq!(first, Err(String::from("Steam returned HTTP 429")));
    assert_eq!(second, first);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // A failure must not become proof that an item is installable.
    assert_eq!(cached_file_type("880000101").expect("cache"), None);
}

#[tokio::test]
async fn independent_batches_share_one_html_request_and_reuse_verified_types() {
    let coordinator = TypeLookupCoordinator::new();
    let active = AtomicUsize::new(0);
    let calls = AtomicUsize::new(0);
    let fetch = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
        tokio::task::yield_now().await;
        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
        Ok(0)
    };
    let (first, second) = tokio::join!(
        coordinator.resolve("880000102", fetch),
        coordinator.resolve("880000103", fetch)
    );
    assert_eq!(first, Ok(0));
    assert_eq!(second, Ok(0));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(coordinator.resolve("880000102", fetch).await, Ok(0));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancelling_the_owner_releases_the_request_and_resumes_an_existing_waiter() {
    let coordinator = TypeLookupCoordinator::new();
    let started = tokio::sync::Notify::new();
    let mut owner = Box::pin(coordinator.resolve("880000104", || async {
        started.notify_one();
        std::future::pending::<Result<u32, String>>().await
    }));
    tokio::select! {
        result = &mut owner => panic!("owner unexpectedly finished: {result:?}"),
        () = started.notified() => {}
    }
    let mut waiter = Box::pin(coordinator.resolve("880000104", || async { Ok(15) }));
    std::future::poll_fn(|context| {
        assert!(waiter.as_mut().poll(context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(owner);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("cancelled owner cannot strand a waiter"),
        Ok(15)
    );
    assert_eq!(coordinator.permit.available_permits(), 1);
}

#[test]
fn pending_registry_is_bounded_and_reclaims_released_requests() {
    let coordinator = TypeLookupCoordinator::new();
    let requests = (0..TYPE_LOOKUP_QUEUE_LIMIT)
        .map(|index| {
            coordinator
                .pending_lookup(&format!("bounded-{index}"))
                .expect("bounded pending request")
        })
        .collect::<Vec<_>>();
    assert!(coordinator.pending_lookup("overflow").is_err());
    assert!(coordinator.pending_lookup("bounded-0").is_ok());
    drop(requests);
    let _request = coordinator
        .pending_lookup("next-request")
        .expect("released requests reclaim capacity");
    assert_eq!(coordinator.pending.lock().expect("queue").len(), 1);
}

#[test]
fn failed_type_lookup_preserves_metadata_and_other_verified_items() {
    let mut details = HashMap::from([
        (
            "880000105".to_string(),
            serde_json::json!({"result": 1, "title": "Available Mod"}),
        ),
        (
            "880000106".to_string(),
            serde_json::json!({"result": 1, "title": "Temporarily unverified"}),
        ),
    ]);
    let mut errors = HashMap::new();
    apply_type_result(&mut details, &mut errors, "880000105".to_string(), Ok(0));
    apply_type_result(
        &mut details,
        &mut errors,
        "880000106".to_string(),
        Err(String::from("Steam returned HTTP 429")),
    );
    assert_eq!(details["880000105"]["file_type"], 0);
    assert_eq!(details["880000106"]["title"], "Temporarily unverified");
    assert!(details["880000106"].get("file_type").is_none());
    assert_eq!(errors.len(), 1);
    assert_eq!(errors["880000106"], "Steam returned HTTP 429");
}

#[tokio::test]
async fn the_type_budget_keeps_completed_metadata_and_cancels_queued_work() {
    struct PendingRequest(Arc<AtomicUsize>);
    impl Drop for PendingRequest {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let ids = (880000201..880000209)
        .map(|id| id.to_string())
        .collect::<Vec<_>>();
    let mut details = ids
        .iter()
        .map(|id| (id.clone(), serde_json::json!({"result": 1, "title": id})))
        .collect::<HashMap<_, _>>();
    let completed = Arc::new(tokio::sync::Notify::new());
    let pending_started = Arc::new(tokio::sync::Notify::new());
    let started = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicUsize::new(0));
    let deadline = async {
        completed.notified().await;
        pending_started.notified().await;
    };
    let errors = populate_missing_types(&mut details, ids.clone(), deadline, |id| {
        let completed = completed.clone();
        let pending_started = pending_started.clone();
        let started = started.clone();
        let cancelled = cancelled.clone();
        async move {
            if id == "880000201" {
                completed.notify_one();
                return Ok(0);
            }
            let _request = PendingRequest(cancelled);
            started.fetch_add(1, Ordering::SeqCst);
            pending_started.notify_one();
            std::future::pending().await
        }
    })
    .await
    .expect("the budget returns partial metadata");
    assert_eq!(details[&ids[0]]["file_type"], 0);
    assert_eq!(errors.len(), ids.len() - 1);
    assert!(!errors.contains_key(&ids[0]));
    for id in &ids[1..] {
        assert_eq!(details[id]["title"], *id);
        assert!(details[id].get("file_type").is_none());
        let error: Value = serde_json::from_str(&errors[id]).expect("structured timeout");
        assert_eq!(error["code"], "steam_workshop_network_failed");
        assert_eq!(error["stage"], "item_type");
        assert_eq!(error["reason"], "timeout");
    }
    assert!(started.load(Ordering::SeqCst) > 0);
    assert!(started.load(Ordering::SeqCst) < ids.len() - 1);
    assert_eq!(
        cancelled.load(Ordering::SeqCst),
        started.load(Ordering::SeqCst),
        "all owned requests have been dropped before the partial response returns"
    );
}
