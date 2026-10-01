use super::*;

const MISSING_PREFAB: &str =
    "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0\t";
const RETRY: &str =
    "[00:00:10]: An error occured during world gen we will retry! [was \t1\t of \t5\t]\t";
const COMPLETE: &str = "[00:00:12]: Generation complete, injecting world entities.\t";
const READY: &str = "[00:00:23]: [LGSM-DST-READY:0123456789abcdef0123456789abcdef]\t";

fn classify(module_id: &str, lines: &[&str]) -> Vec<String> {
    runtime_fatal_log_lines(
        module_id,
        &lines
            .iter()
            .map(|line| String::from(*line))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn runtime_fatal_log_recognizes_completed_dst_world_generation_retry() {
    assert!(classify("dontstarve", &[MISSING_PREFAB, RETRY, COMPLETE, READY]).is_empty());
    assert!(
        classify(
            " DontStarve ",
            &[
                "PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0",
                "An error occured during world gen we will retry! [was 1 of 5]",
                "Generation complete, injecting world entities.",
            ],
        )
        .is_empty()
    );
}

#[test]
fn runtime_fatal_log_recovery_preserves_other_fatal_diagnostics_and_order() {
    let first = "[00:00:09]: FATAL: unrelated subsystem failed";
    let last = "[00:00:24]: unhandled exception in server";
    assert_eq!(
        classify(
            "dontstarve",
            &[first, MISSING_PREFAB, RETRY, COMPLETE, READY, last],
        ),
        [first, last]
    );
}

#[test]
fn runtime_fatal_log_requires_retry_then_generation_completion() {
    for lines in [
        vec![MISSING_PREFAB],
        vec![MISSING_PREFAB, RETRY],
        vec![MISSING_PREFAB, COMPLETE],
        vec![MISSING_PREFAB, READY],
        vec![MISSING_PREFAB, RETRY, READY],
        vec![COMPLETE, MISSING_PREFAB, RETRY],
        vec![RETRY, MISSING_PREFAB, COMPLETE],
        vec![MISSING_PREFAB, COMPLETE, RETRY],
    ] {
        assert_eq!(
            classify("dontstarve", &lines),
            [MISSING_PREFAB],
            "{lines:?}"
        );
    }
}

#[test]
fn runtime_fatal_log_world_generation_recovery_is_dst_only() {
    for module_id in ["minecraft", "test", "projectzomboid"] {
        assert_eq!(
            classify(module_id, &[MISSING_PREFAB, RETRY, COMPLETE, READY]),
            [MISSING_PREFAB]
        );
    }
}

#[test]
fn runtime_fatal_log_recovery_rejects_console_echoes_and_non_native_markers() {
    for (retry, complete) in [
        (
            "[00:00:11]: RemoteCommandInput: print('An error occured during world gen we will retry!')",
            COMPLETE,
        ),
        (
            RETRY,
            "[00:00:12]: RemoteCommandInput: print('Generation complete, injecting world entities.')",
        ),
        (
            RETRY,
            "[player]: Generation complete, injecting world entities.",
        ),
        (
            RETRY,
            "[00:00:12]: Generation complete, injecting world entities. failed",
        ),
    ] {
        assert_eq!(
            classify("dontstarve", &[MISSING_PREFAB, retry, complete, READY]),
            [MISSING_PREFAB]
        );
    }
}

#[test]
fn runtime_fatal_log_only_recognizes_the_native_missing_prefab_diagnostic() {
    for panic in [
        "[00:00:10]: PANIC: actual Lua failure",
        "[00:00:10]: PANIC: missing required prefab [bishop]! Expected one, got zero",
        "[00:00:10]: PANIC: missing required prefab [bishop]! Expected 1, got 0 after crash",
        "[00:00:10]: RemoteCommandInput: print('PANIC: missing required prefab [bishop]! Expected 1, got 0')",
    ] {
        assert_eq!(
            classify("dontstarve", &[panic, RETRY, COMPLETE, READY]),
            [panic]
        );
    }
}

#[test]
fn runtime_fatal_log_resolves_retried_attempts_but_preserves_new_failures() {
    let second = "[00:00:11]: PANIC: missing required prefab [sculpture_rook]! Expected 2, got 1";
    let later = "[00:00:25]: PANIC: missing required prefab [sculpture_knight]! Expected 1, got 0";
    assert_eq!(
        classify(
            "dontstarve",
            &[MISSING_PREFAB, RETRY, second, RETRY, COMPLETE, READY, later],
        ),
        [later]
    );
}

#[test]
fn runtime_fatal_log_preserves_existing_generic_matching() {
    let lines = [
        "FATAL: failure",
        "Unhandled Exception",
        "segmentation fault",
        "Access Violation",
        "Assertion Failed",
        "PANIC: failure",
    ];
    assert_eq!(classify("test", &lines), lines);
    assert!(classify("dontstarve", &[RETRY, COMPLETE, READY]).is_empty());
}
