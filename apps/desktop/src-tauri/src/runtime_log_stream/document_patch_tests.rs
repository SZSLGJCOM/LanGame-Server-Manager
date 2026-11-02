use super::*;

fn document(lines: &[&str], revision: u64) -> NativeLogDocument {
    NativeLogDocument {
        snapshot: LogTailSnapshot {
            source_path: Some("native.log".into()),
            lines: lines.iter().map(|line| (*line).into()).collect(),
            total_lines: lines.len(),
            truncated: false,
            read_error: None,
        },
        revision,
        run_id: 1,
    }
}

fn assert_applies(old: &NativeLogDocument, new: &NativeLogDocument, reset: bool) {
    let patch = document_patch(old, new, reset);
    assert_eq!(patch.previous_revision, old.revision);
    assert_eq!(patch.revision, new.revision);
    assert!(patch.start <= old.snapshot.lines.len());
    assert!(patch.delete_count <= old.snapshot.lines.len() - patch.start);
    let mut actual = old.snapshot.clone();
    actual
        .lines
        .splice(patch.start..patch.start + patch.delete_count, patch.lines);
    actual.total_lines = patch.total_lines;
    actual.truncated = patch.truncated;
    actual.read_error = patch.read_error;
    assert!(same_document(&actual, &new.snapshot));
}

#[test]
fn unicode_repetitions_empty_documents_and_partial_rows_splice_exactly() {
    let alphabet = ["same", "汉字🙂", ""];
    let mut documents = vec![Vec::<&str>::new()];
    let mut level = vec![Vec::<&str>::new()];
    for _ in 0..4 {
        level = level
            .iter()
            .flat_map(|previous| {
                alphabet.iter().map(move |line| {
                    let mut next = previous.clone();
                    next.push(*line);
                    next
                })
            })
            .collect();
        documents.extend(level.clone());
    }
    for old in &documents {
        for new in &documents {
            assert_applies(&document(old, 7), &document(new, 19), false);
        }
    }
    let old = document(&["header", "下载中", "same"], 7);
    let new = document(&["header", "下载中🙂完成", "same"], 19);
    let patch = document_patch(&old, &new, false);
    assert_eq!((patch.start, patch.delete_count), (1, 1));
    assert_eq!(patch.lines, ["下载中🙂完成"]);
    assert_applies(&old, &new, false);

    let repeated = document_patch(
        &document(&["same", "same"], 1),
        &document(&["same", "same", "same"], 2),
        false,
    );
    assert_eq!((repeated.start, repeated.delete_count), (2, 0));
    assert_eq!(repeated.lines, ["same"]);
}

#[test]
fn a_four_hundred_row_window_slide_is_an_exact_bounded_document_replacement() {
    let mut old = document(&[], 1);
    old.snapshot.lines = (0..400).map(|index| format!("row {index}")).collect();
    old.snapshot.total_lines = 400;
    let mut new = document(&[], 9);
    new.snapshot.lines = (1..401).map(|index| format!("row {index}")).collect();
    new.snapshot.total_lines = 401;
    new.snapshot.truncated = true;
    assert_applies(&old, &new, false);

    old.snapshot.lines.fill("same".into());
    new.snapshot.lines.fill("same".into());
    let patch = document_patch(&old, &new, false);
    assert!(patch.lines.is_empty());
    assert_eq!(patch.delete_count, 0);
    assert_eq!(patch.total_lines, 401);
    assert!(patch.truncated);
    assert_applies(&old, &new, false);
}

#[test]
fn metadata_only_changes_and_recovery_keep_the_exact_rows() {
    let old = document(&["same", "same"], 1);
    let mut error = document(&["same", "same"], 2);
    error.snapshot.read_error = Some("native read failed".into());
    let patch = document_patch(&old, &error, false);
    assert!(patch.lines.is_empty());
    assert_eq!(patch.delete_count, 0);
    assert_eq!(patch.read_error.as_deref(), Some("native read failed"));
    assert_applies(&old, &error, false);
    assert_applies(&error, &document(&["same", "same"], 3), false);
}

#[test]
fn a_source_generation_replacement_splices_the_entire_document_even_when_text_matches() {
    let old = document(&["same", "same"], 1);
    let new = document(&["same", "same"], 8);
    let patch = document_patch(&old, &new, true);
    assert_eq!((patch.start, patch.delete_count), (0, 2));
    assert_eq!(patch.lines, ["same", "same"]);
    assert_applies(&old, &new, true);
    assert_applies(&old, &document(&[], 9), true);
}
