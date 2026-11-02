use super::*;

#[test]
fn public_preview_commit_failure_and_invalid_size_preserve_previous_rows() {
    let path =
        std::env::temp_dir().join(format!("langame-preview-commit-{}.log", std::process::id()));
    let preview = LiveConsolePreview::register(&path).unwrap();
    preview.replace(vec!["first".into()], || Ok(())).unwrap();
    let error = preview
        .replace(vec!["second".into()], || {
            Err::<(), _>(io::Error::other("disk failure"))
        })
        .unwrap_err();
    assert!(error.to_string().contains("disk failure"));
    assert_eq!(
        with_live_console_preview(&path, <[String]>::to_vec).unwrap(),
        ["first"]
    );
    assert!(
        preview
            .replace::<()>(vec!["x".repeat(64 * 1024 + 1)], || panic!(
                "oversized preview must not commit"
            ))
            .is_err()
    );
    assert_eq!(
        with_live_console_preview(&path, <[String]>::to_vec).unwrap(),
        ["first"]
    );
}
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

fn log_path() -> PathBuf {
    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "langame-console-preview-{}-{}.log",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn absent_and_finished_consoles_have_no_preview_even_before_log_creation() {
    let path = log_path();
    assert!(with_live_console_preview(&path, |rows| rows.is_empty()).unwrap());
    let preview = LiveConsolePreview::register(&path).unwrap();
    preview
        .update(|rows| {
            rows.push("loading".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        with_live_console_preview(&path, <[String]>::to_vec).unwrap(),
        ["loading"]
    );
    drop(preview);
    assert!(with_live_console_preview(&path, |rows| rows.is_empty()).unwrap());
    assert!(!registry().lock().unwrap().contains_key(&path));
}

#[test]
fn relative_and_absolute_log_names_find_the_same_owner() {
    let relative = PathBuf::from(log_path().file_name().unwrap());
    let absolute = std::path::absolute(&relative).unwrap();
    let preview = LiveConsolePreview::register(&relative).unwrap();
    preview
        .update(|rows| {
            rows.push("ready soon".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        with_live_console_preview(&absolute, <[String]>::to_vec).unwrap(),
        ["ready soon"]
    );
}

#[test]
fn a_replaced_owner_cannot_publish_to_or_remove_the_new_preview() {
    let path = log_path();
    let older = LiveConsolePreview::register(&path).unwrap();
    let newer = LiveConsolePreview::register(&path).unwrap();
    newer
        .update(|rows| {
            rows.push("new session".into());
            Ok(())
        })
        .unwrap();
    older
        .update(|rows| {
            rows.push("late old output".into());
            Ok(())
        })
        .unwrap();
    drop(older);
    assert_eq!(
        with_live_console_preview(&path, <[String]>::to_vec).unwrap(),
        ["new session"]
    );
}

#[test]
fn durable_row_and_preview_replacement_share_the_readers_lock() {
    let path = log_path();
    let file = std::fs::File::create(&path).unwrap();
    let preview = LiveConsolePreview::register(&path).unwrap();
    preview
        .update(|rows| {
            rows.push("complete soon".into());
            Ok(())
        })
        .unwrap();
    let (blocked, observed_block) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        with_live_console_preview(&path, |rows| {
            scope.spawn(|| {
                assert!(matches!(
                    preview.rows.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                blocked.send(()).unwrap();
                let mut writer = std::io::BufWriter::new(file);
                preview
                    .update(|rows| {
                        writer.write_all(b"complete soon\n")?;
                        writer.flush()?;
                        rows.clear();
                        Ok(())
                    })
                    .unwrap();
            });
            observed_block
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert_eq!(rows, ["complete soon"]);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        })
        .unwrap();
    });
    let (durable, pending) = with_live_console_preview(&path, |rows| {
        (std::fs::read_to_string(&path).unwrap(), rows.to_vec())
    })
    .unwrap();
    assert_eq!(durable, "complete soon\n");
    assert!(pending.is_empty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn poisoned_preview_reports_an_error_instead_of_pretending_output_is_empty() {
    let path = log_path();
    let preview = LiveConsolePreview::register(&path).unwrap();
    let panic = std::panic::catch_unwind(|| {
        let _: io::Result<()> = preview.update(|_| panic!("synthetic writer panic"));
    });
    assert!(panic.is_err());
    let error = with_live_console_preview(&path, |_| {
        panic!("a poisoned preview must not invoke the reader")
    })
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("live console preview lock failed")
    );
}
