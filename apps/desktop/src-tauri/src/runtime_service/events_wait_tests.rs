use super::*;
use std::future::{Future, poll_fn};
use std::task::Poll;

fn initialized_cursor(source: &Events) -> Cursor {
    let mut cursor = Cursor::default();
    cursor.accept(source.after(0, None).unwrap()).unwrap();
    cursor
}

async fn immediately<T>(future: impl Future<Output = T>) -> T {
    tokio::pin!(future);
    match poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await {
        Poll::Ready(result) => result,
        Poll::Pending => {
            panic!("An available event or reset must not wait for the long-poll timer")
        }
    }
}

#[tokio::test]
async fn initial_generation_and_already_queued_events_return_immediately() {
    let source = Events::default();
    let mut cursor = Cursor::default();
    let first = immediately(source.wait_after(0, None, LONG_POLL_TIMEOUT))
        .await
        .unwrap();
    assert_eq!(first["reset"], true);
    assert!(first["events"].as_array().unwrap().is_empty());
    assert!(!cursor.accept(first).unwrap().0);

    source.push("runtime-log-stream", "42");
    let request = cursor.request_args();
    let batch = immediately(source.wait_after(
        request["after"].as_u64().unwrap(),
        request["generation"].as_str(),
        LONG_POLL_TIMEOUT,
    ))
    .await
    .unwrap();
    let (readback, received) = cursor.accept(batch).unwrap();
    assert!(!readback);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].sequence, 1);
    assert_eq!(received[0].payload, 42);
}

#[tokio::test]
async fn an_idle_reader_wakes_on_publication_and_does_not_replay_the_event() {
    let source = Events::default();
    let mut cursor = initialized_cursor(&source);
    let request = cursor.request_args();
    let mut waiting = Box::pin(source.wait_after(
        request["after"].as_u64().unwrap(),
        request["generation"].as_str(),
        LONG_POLL_TIMEOUT,
    ));
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    source.push("runtime-log-stream", "42");
    let batch = immediately(waiting).await.unwrap();
    let (readback, received) = cursor.accept(batch).unwrap();
    assert!(!readback);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].sequence, 1);

    let next = cursor.request_args();
    let mut idle = Box::pin(source.wait_after(
        next["after"].as_u64().unwrap(),
        next["generation"].as_str(),
        LONG_POLL_TIMEOUT,
    ));
    assert!(
        poll_fn(|cx| Poll::Ready(idle.as_mut().poll(cx)))
            .await
            .is_pending(),
        "An acknowledged event must not cause another immediate response"
    );
}

#[tokio::test]
async fn publication_after_the_empty_check_reaches_every_unpolled_subscription() {
    let source = Events::default();
    let mut first_cursor = initialized_cursor(&source);
    let mut second_cursor = initialized_cursor(&source);
    let request = first_cursor.request_args();
    let generation = request["generation"].as_str();
    let (first_batch, first_changed) = source.observe_after(0, generation).unwrap();
    let (second_batch, second_changed) = source.observe_after(0, generation).unwrap();
    assert!(!first_batch.reset && first_batch.events.is_empty());
    assert!(!second_batch.reset && second_batch.events.is_empty());

    // Publish after both empty checks but before either Notified is polled.
    // A single notify_one permit would incorrectly leave one reader asleep.
    source.push("runtime-log-stream", "42");
    immediately(first_changed).await;
    immediately(second_changed).await;
    for cursor in [&mut first_cursor, &mut second_cursor] {
        let batch = immediately(source.wait_after(0, generation, LONG_POLL_TIMEOUT))
            .await
            .unwrap();
        let (readback, received) = cursor.accept(batch).unwrap();
        assert!(!readback);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].sequence, 1);
    }
}

#[tokio::test]
async fn cancelling_one_waiter_does_not_consume_another_readers_notification() {
    let source = Events::default();
    let cursor = initialized_cursor(&source);
    let request = cursor.request_args();
    let generation = request["generation"].as_str();
    let mut cancelled = Box::pin(source.wait_after(0, generation, LONG_POLL_TIMEOUT));
    let mut retained = Box::pin(source.wait_after(0, generation, LONG_POLL_TIMEOUT));
    for waiting in [&mut cancelled, &mut retained] {
        assert!(
            poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
                .await
                .is_pending()
        );
    }
    drop(cancelled);
    source.push("runtime-log-stream", "42");
    let batch = immediately(retained).await.unwrap();
    assert_eq!(batch["events"][0]["sequence"], 1);
}

#[tokio::test]
async fn an_empty_timeout_preserves_the_acknowledged_cursor() {
    let source = Events::default();
    source.push("runtime-log-stream", "42");
    let mut cursor = initialized_cursor(&source);
    let before = cursor.request_args();
    let batch = source
        .wait_after(
            before["after"].as_u64().unwrap(),
            before["generation"].as_str(),
            Duration::ZERO,
        )
        .await
        .unwrap();
    let (readback, received) = cursor.accept(batch).unwrap();
    assert!(!readback);
    assert!(received.is_empty());
    assert_eq!(cursor.request_args(), before);
}

#[tokio::test]
async fn a_final_dropped_event_wakes_an_idle_reader_for_readback() {
    let source = Events::default();
    let mut cursor = initialized_cursor(&source);
    let request = cursor.request_args();
    let mut waiting =
        Box::pin(source.wait_after(0, request["generation"].as_str(), LONG_POLL_TIMEOUT));
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    source.push("runtime-log-stream", "{");
    let (readback, received) = cursor.accept(immediately(waiting).await.unwrap()).unwrap();
    assert!(readback);
    assert!(received.is_empty());
    assert_ne!(cursor.request_args()["generation"], request["generation"]);
}

#[tokio::test]
async fn retained_backlog_drains_immediately_in_bounded_batches_without_replay() {
    let source = Events::default();
    let mut cursor = initialized_cursor(&source);
    let count = BATCH_SIZE * 3 + 1;
    for _ in 0..count {
        source.push("runtime-log-stream", "42");
    }
    let mut sequences = Vec::new();
    while sequences.len() < count {
        let request = cursor.request_args();
        let batch = immediately(source.wait_after(
            request["after"].as_u64().unwrap(),
            request["generation"].as_str(),
            LONG_POLL_TIMEOUT,
        ))
        .await
        .unwrap();
        let (readback, received) = cursor.accept(batch).unwrap();
        assert!(!readback);
        assert_eq!(received.len(), BATCH_SIZE.min(count - sequences.len()));
        sequences.extend(received.into_iter().map(|event| event.sequence));
    }
    assert_eq!(sequences, (1..=count as u64).collect::<Vec<_>>());
    let request = cursor.request_args();
    let mut idle = Box::pin(source.wait_after(
        request["after"].as_u64().unwrap(),
        request["generation"].as_str(),
        LONG_POLL_TIMEOUT,
    ));
    assert!(
        poll_fn(|cx| Poll::Ready(idle.as_mut().poll(cx)))
            .await
            .is_pending()
    );
}

#[tokio::test]
async fn a_spurious_notification_does_not_complete_an_empty_long_poll() {
    let source = Events::default();
    let cursor = initialized_cursor(&source);
    let request = cursor.request_args();
    let mut waiting =
        Box::pin(source.wait_after(0, request["generation"].as_str(), LONG_POLL_TIMEOUT));
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    source.changed.notify_waiters();
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    source.push("runtime-log-stream", "42");
    assert_eq!(
        immediately(waiting).await.unwrap()["events"][0]["sequence"],
        1
    );
}
