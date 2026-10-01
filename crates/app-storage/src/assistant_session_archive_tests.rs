use super::*;

#[tokio::test]
async fn assistant_session_archive_timeout_retains_worker_slot_until_real_completion() {
    let archive = AssistantSessionArchive {
        root: std::env::temp_dir().join("unused-assistant-archive-timeout-fixture"),
    };
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (expire_tx, expire_rx) = tokio::sync::oneshot::channel();
    let worker_archive = archive.clone();
    let worker_slots = slots.clone();
    let waiter = tokio::spawn(async move {
        worker_archive
            .run_with_deadline(
                worker_slots,
                async {
                    expire_rx
                        .await
                        .expect("controlled deadline must be signalled")
                },
                move |_| {
                    let _ = started_tx.send(());
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| "test worker was not released".to_owned())?;
                    Ok(())
                },
            )
            .await
    });
    // Expire only after the worker has definitely started. No scheduler speed
    // assumption or sleep decides whether the timeout exercises in-flight I/O.
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .expect("archive worker must start")
        .expect("archive worker must signal admission");
    expire_tx.send(()).unwrap();
    let error = waiter.await.unwrap().unwrap_err();
    assert!(error.contains("outcome is unknown"));
    assert!(slots.try_acquire().is_err());

    let (queue_expire_tx, queue_expire_rx) = tokio::sync::oneshot::channel();
    let queued = archive.run_with_deadline(
        slots.clone(),
        async {
            queue_expire_rx
                .await
                .expect("queue deadline must be signalled")
        },
        |_| -> Result<(), String> { panic!("timed-out work must retain the only I/O slot") },
    );
    tokio::pin!(queued);
    tokio::select! {
        biased;
        result = &mut queued => panic!("queued work completed before its deadline: {result:?}"),
        () = std::future::ready(()) => {}
    }
    queue_expire_tx.send(()).unwrap();
    assert!(queued.await.unwrap_err().contains("was not started"));
    assert!(slots.try_acquire().is_err());

    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_secs(5), slots.acquire())
        .await
        .expect("finished I/O must release admission")
        .unwrap();
    drop(permit);
    assert_eq!(
        archive
            .run_with_deadline(slots, tokio::time::sleep(Duration::from_secs(1)), |_| Ok(
                42
            ))
            .await
            .unwrap(),
        42
    );
}
