use super::*;
use crate::state::RuntimeLogStreams;
use std::cell::{Cell, RefCell};
use std::fs;

struct Fixture(PathBuf);

impl Fixture {
    fn new(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lg-stream-owner-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("server.log"), text).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("server.log")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn live_idle_partial_is_not_emitted_until_the_producer_is_finished() {
    let fixture = Fixture::new("ready\nlast partial");
    let calls = Cell::new(0);
    let output = RefCell::new(Vec::new());
    run_stream(
        fixture.path(),
        StreamLimits {
            poll_interval: Duration::ZERO,
            ..LIMITS
        },
        || {
            let call = calls.get();
            calls.set(call + 1);
            // Two complete polls include an idle read with an unterminated row.
            if call >= 6 {
                if call == 6 {
                    assert_eq!(*output.borrow(), ["ready"]);
                }
                RuntimeLogStreamStatus::ProducerFinished
            } else {
                RuntimeLogStreamStatus::Active
            }
        },
        |lines, _, stream_error| {
            assert!(stream_error.is_none());
            output.borrow_mut().extend(lines);
        },
    )
    .await;
    assert_eq!(*output.borrow(), ["ready", "last partial"]);
}

#[tokio::test]
async fn producer_completion_drains_multiple_reads_and_flushes_unicode_once() {
    let fixture = Fixture::new("ready\n玩家😀");
    let output = RefCell::new(Vec::new());
    run_stream(
        fixture.path(),
        StreamLimits {
            read_bytes: 2,
            ..LIMITS
        },
        || RuntimeLogStreamStatus::ProducerFinished,
        |lines, _, stream_error| {
            assert!(stream_error.is_none());
            output.borrow_mut().extend(lines);
        },
    )
    .await;
    assert_eq!(*output.borrow(), ["ready", "玩家😀"]);
}

#[tokio::test]
async fn the_last_permitted_read_preserves_a_complete_final_partial_row() {
    let fixture = Fixture::new("final");
    let output = RefCell::new(Vec::new());
    run_stream(
        fixture.path(),
        StreamLimits {
            read_bytes: 128,
            final_reads: 1,
            ..LIMITS
        },
        || RuntimeLogStreamStatus::ProducerFinished,
        |lines, _, stream_error| {
            assert!(stream_error.is_none());
            output.borrow_mut().extend(lines);
        },
    )
    .await;
    assert_eq!(*output.borrow(), ["final"]);
}

#[tokio::test]
async fn replacement_during_file_read_discards_the_old_delta_and_pending_tail() {
    for new_path in ["server.log", "new-run.log"] {
        let fixture = Fixture::new("old complete\nold partial");
        let streams = RefCell::new(RuntimeLogStreams::default());
        let lease = streams.borrow_mut().reserve("world", "server.log").unwrap();
        let calls = Cell::new(0);
        let replacement = Cell::new(None);
        let output = RefCell::new(Vec::new());
        run_stream(
            fixture.path(),
            LIMITS,
            || {
                let call = calls.get();
                calls.set(call + 1);
                if call == 1 {
                    let mut streams = streams.borrow_mut();
                    streams.finish_path("world", "server.log");
                    replacement.set(streams.reserve("world", new_path));
                }
                streams.borrow().status("world", "server.log", lease)
            },
            |lines, _, stream_error| {
                assert!(stream_error.is_none());
                output.borrow_mut().extend(lines);
            },
        )
        .await;
        assert!(output.borrow().is_empty());
        assert!(
            streams
                .borrow()
                .is_active("world", new_path, replacement.get().unwrap())
        );
    }
}

#[tokio::test]
async fn final_overlong_row_terminates_after_empty_read_even_when_limit_is_exhausted() {
    let fixture = Fixture::new("abcdefghijk");
    let output = RefCell::new(Vec::new());
    run_stream(
        fixture.path(),
        StreamLimits {
            read_bytes: 3,
            pending_bytes: 4,
            ..LIMITS
        },
        || RuntimeLogStreamStatus::ProducerFinished,
        |lines, _, stream_error| {
            assert!(
                lines.is_empty(),
                "Reader diagnostics are not native file rows"
            );
            output.borrow_mut().extend(stream_error);
        },
    )
    .await;
    let output = output.borrow();
    assert_eq!(output.len(), 1);
    assert!(
        output[0].contains("final console line exceeded"),
        "{output:?}"
    );
    assert!(!output[0].contains("before all final output"), "{output:?}");
}

#[tokio::test]
async fn final_read_and_time_budgets_report_incomplete_without_flushing_unread_tail() {
    for limits in [
        StreamLimits {
            read_bytes: 3,
            final_reads: 1,
            ..LIMITS
        },
        StreamLimits {
            final_budget: Duration::ZERO,
            ..LIMITS
        },
    ] {
        let fixture = Fixture::new("partial that must not be presented as a complete final row");
        let output = RefCell::new(Vec::new());
        run_stream(
            fixture.path(),
            limits,
            || RuntimeLogStreamStatus::ProducerFinished,
            |lines, _, stream_error| {
                assert!(
                    lines.is_empty(),
                    "Incomplete drain must not invent file rows"
                );
                output.borrow_mut().extend(stream_error);
            },
        )
        .await;
        assert_eq!(*output.borrow(), [INCOMPLETE]);
    }
}

#[tokio::test]
async fn a_read_failure_is_reported_once_and_ends_the_stream() {
    let fixture = Fixture::new("unused");
    let output = RefCell::new(Vec::new());
    run_stream(
        fixture.0.clone(),
        LIMITS,
        || RuntimeLogStreamStatus::Active,
        |lines, _, stream_error| {
            assert!(
                lines.is_empty(),
                "Read failure must use the diagnostic field"
            );
            output.borrow_mut().extend(stream_error);
        },
    )
    .await;
    let output = output.borrow();
    assert_eq!(output.len(), 1);
    assert!(output[0].contains("Log read failed:"));
    assert!(output[0].contains(INCOMPLETE));
}

#[tokio::test]
async fn shutdown_wait_has_a_deadline_and_distinguishes_unfinished_leases() {
    let streams = RefCell::new(RuntimeLogStreams::default());
    let lease = streams.borrow_mut().reserve("world", "server.log").unwrap();
    streams.borrow_mut().finish_path("world", "server.log");
    assert!(!wait_until_empty(|| streams.borrow().is_empty(), Duration::ZERO).await);
    streams.borrow_mut().release("world", "server.log", lease);
    assert!(wait_until_empty(|| streams.borrow().is_empty(), Duration::ZERO).await);
}
