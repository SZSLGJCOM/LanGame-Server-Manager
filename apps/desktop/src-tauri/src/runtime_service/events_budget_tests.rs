use super::*;

fn pull(source: &Events, cursor: &mut Cursor) -> (bool, Vec<Event>) {
    let request = cursor.request_args();
    cursor
        .accept(
            source
                .after(
                    request["after"].as_u64().unwrap(),
                    request["generation"].as_str(),
                )
                .unwrap(),
        )
        .unwrap()
}

#[test]
fn a_bounded_log_read_preserves_large_json_escaped_rows() {
    let source = Events::default();
    let mut cursor = Cursor::default();
    pull(&source, &mut cursor);
    // The producer can combine 256 KiB of new bytes with 64 KiB pending.
    // Control characters expand to six JSON bytes each; every row remains
    // below its existing 64 KiB pending-row limit.
    let rows = vec!["\u{0001}".repeat(64 * 1024 - 1); 5];
    let payload = json!({
        "instance_id": "escaped", "log_path": "server.log",
        "run_id": 1, "lines": rows, "byte_offset": 320 * 1024,
    });
    let encoded = serde_json::to_string(&payload).unwrap();
    assert!(encoded.len() > 128 * 1024);
    assert!(encoded.len() < MAX_EVENT_BYTES);
    source.push("runtime-log-stream", &encoded);
    let (reset, events) = pull(&source, &mut cursor);
    assert!(!reset);
    assert_eq!(events.len(), 1);
    assert!(
        events[0].payload == payload,
        "large escaped rows must arrive unchanged"
    );
}

#[test]
fn byte_retention_evicts_before_the_count_limit_and_reports_one_gap() {
    let source = Events::default();
    let mut cursor = Cursor::default();
    pull(&source, &mut cursor);
    let payload = serde_json::to_string(&"x".repeat(1024 * 1024)).unwrap();
    for _ in 0..35 {
        source.push("runtime-log-stream", &payload);
    }
    let (reset, first) = pull(&source, &mut cursor);
    assert!(reset, "byte eviction must invalidate a lagging cursor");
    assert!(!first.is_empty());
    let first_sequence = first[0].sequence;
    assert!(first_sequence > 1);
    let mut events = first;
    loop {
        let (reset, next) = pull(&source, &mut cursor);
        assert!(!reset, "the retained contiguous suffix must not replay");
        if next.is_empty() {
            break;
        }
        events.extend(next);
    }
    let bytes: usize = events.iter().map(|event| json_size(event).unwrap()).sum();
    assert!(bytes <= MAX_RETAINED_BYTES);
    assert!(events.len() < 35 && events.len() < CAPACITY);
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        (first_sequence..=35).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn large_event_batches_fit_the_wire_and_deliver_the_remaining_events() {
    use super::super::wire;

    let source = Events::default();
    let mut cursor = Cursor::default();
    pull(&source, &mut cursor);
    let payload = serde_json::to_string(&"x".repeat(1_500_000)).unwrap();
    for _ in 0..4 {
        source.push("runtime-log-stream", &payload);
    }
    let mut sequences = Vec::new();
    for _ in 0..2 {
        let request = cursor.request_args();
        let batch = source
            .after(
                request["after"].as_u64().unwrap(),
                request["generation"].as_str(),
            )
            .unwrap();
        assert!(serde_json::to_vec(&batch).unwrap().len() <= MAX_BATCH_BYTES);
        assert_eq!(batch["events"].as_array().unwrap().len(), 2);
        let response = wire::Response { result: Ok(batch) };
        assert!(serde_json::to_vec(&response).unwrap().len() < wire::MAX_FRAME);
        let (mut sender, mut receiver) = tokio::io::duplex(64 * 1024);
        let (sent, received) = tokio::join!(
            wire::write_response(&mut sender, &response),
            wire::read_frame::<wire::Response>(&mut receiver),
        );
        sent.unwrap();
        let (reset, events) = cursor.accept(received.unwrap().result.unwrap()).unwrap();
        assert!(!reset);
        assert!(
            events
                .iter()
                .all(|event| event.payload.as_str().unwrap().len() == 1_500_000)
        );
        sequences.extend(events.iter().map(|event| event.sequence));
    }
    assert_eq!(sequences, [1, 2, 3, 4]);
    assert!(pull(&source, &mut cursor).1.is_empty());
}

#[test]
fn an_oversized_final_event_requests_readback_without_a_later_event() {
    let source = Events::default();
    let mut cursor = Cursor::default();
    source.push("runtime-log-stream", "\"before\"");
    pull(&source, &mut cursor);
    let oversized = serde_json::to_string(&"x".repeat(MAX_EVENT_BYTES)).unwrap();
    source.push("runtime-log-stream", &oversized);
    let (reset, events) = pull(&source, &mut cursor);
    assert!(
        reset,
        "a dropped tail must be observable even without a successor"
    );
    assert!(events.is_empty());
    assert!(
        !pull(&source, &mut cursor).0,
        "readback must not repeat forever"
    );
    source.push("runtime-log-stream", "\"after\"");
    let (reset, events) = pull(&source, &mut cursor);
    assert!(!reset);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload, "after");
}

#[test]
fn an_oversized_middle_event_resets_before_delivering_its_successor() {
    let source = Events::default();
    let mut cursor = Cursor::default();
    pull(&source, &mut cursor);
    source.push("runtime-log-stream", "\"before\"");
    // The payload alone fits exactly, but its Event envelope does not.
    let oversized = serde_json::to_string(&"x".repeat(MAX_EVENT_BYTES - 2)).unwrap();
    assert_eq!(oversized.len(), MAX_EVENT_BYTES);
    source.push("runtime-log-stream", &oversized);
    source.push("runtime-log-stream", "\"after\"");
    let (reset, events) = pull(&source, &mut cursor);
    assert!(reset);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload, "after");
    assert!(!pull(&source, &mut cursor).0);
}
