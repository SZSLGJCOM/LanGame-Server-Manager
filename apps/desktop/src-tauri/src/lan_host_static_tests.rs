use super::*;
use std::time::Duration;

#[test]
fn rejects_traversal_alternate_streams_and_invalid_encoding() {
    for path in [
        "/../private",
        "/%2e%2e/private",
        "/index.html:private",
        "/index.html%3Aprivate",
        "/dir%5cprivate",
        "/%00",
        "/%ff",
        "/%",
        "/%GG",
    ] {
        assert!(relative_path(path).is_none(), "must reject {path}");
    }
    assert_eq!(
        relative_path("/assets/a%20b.js?v=1").unwrap(),
        Path::new("assets/a b.js")
    );
    assert_eq!(relative_path("/").unwrap(), Path::new("index.html"));
}

#[test]
fn static_transfer_bounds_reads_and_detects_truncation() {
    struct BoundedReader {
        remaining: usize,
    }
    impl Read for BoundedReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            assert!(output.len() <= COPY_BUFFER_BYTES);
            let count = output.len().min(self.remaining);
            output[..count].fill(b'x');
            self.remaining -= count;
            Ok(count)
        }
    }
    let length = COPY_BUFFER_BYTES * 3 + 17;
    let mut reader = BoundedReader { remaining: length };
    let mut output = Vec::new();
    copy_body(
        &mut reader,
        &mut output,
        length as u64,
        ResponseWritePolicy::bounded(Duration::from_secs(2)),
    )
    .unwrap();
    assert_eq!(output, vec![b'x'; length]);
    assert!(
        copy_body(
            &mut reader,
            &mut output,
            1,
            ResponseWritePolicy::bounded(Duration::from_secs(2))
        )
        .is_err()
    );
}

#[cfg(windows)]
#[test]
fn opened_handle_identifies_links_outside_distribution_root() {
    use std::os::windows::process::CommandExt;

    let root = std::env::temp_dir().join(format!("langame-lan-static-{}", uuid::Uuid::new_v4()));
    let dist = root.join("dist");
    fs::create_dir_all(&dist).unwrap();
    fs::write(root.join("private.txt"), b"private fixture").unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(dist.join("linked"))
        .arg(&root)
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(output.status.success(), "create isolated junction fixture");
    let path = dist.join("linked/private.txt");
    let file = File::open(&path).unwrap();
    assert!(
        !opened_path(&file, &path)
            .unwrap()
            .starts_with(fs::canonicalize(&dist).unwrap())
    );
    drop(file);
    fs::remove_dir(dist.join("linked")).unwrap();
    fs::remove_dir_all(&root).unwrap();
}
