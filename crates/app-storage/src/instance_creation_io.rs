use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::StorageError;
use sha2::{Digest, Sha256};

pub(crate) fn check_creation_cancelled(
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(StorageError::InstanceCreationCancelled);
    }
    Ok(())
}

pub(crate) fn copy_creation_file(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<String, StorageError> {
    copy_creation_file_with(source, destination, cancellation, false, |input, output| {
        copy_creation_stream(input, output, cancellation)
    })
}

pub(crate) fn copy_verified_creation_file(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<String, StorageError> {
    copy_creation_file_with(source, destination, cancellation, true, |input, output| {
        copy_creation_stream(input, output, cancellation)
    })
}

pub(crate) fn copy_independent_program_file(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    copy_creation_file_with(source, destination, cancellation, false, |input, output| {
        copy_creation_stream_with(input, output, cancellation, |_| {})
    })
}

fn copy_creation_file_with<T>(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
    distinguish_source_errors: bool,
    transfer: impl FnOnce(&mut fs::File, &mut fs::File) -> Result<T, CopyStreamError>,
) -> Result<T, StorageError> {
    check_creation_cancelled(cancellation)?;
    let copy_error = |source_error| StorageError::CopyPath {
        from: source.to_path_buf(),
        to: destination.to_path_buf(),
        source: source_error,
    };
    let read_error = |source_error| {
        if distinguish_source_errors {
            StorageError::ReadPath {
                path: source.to_owned(),
                source: source_error,
            }
        } else {
            copy_error(source_error)
        }
    };
    let mut input = fs::File::open(source).map_err(read_error)?;
    let mut output = fs::File::create_new(destination).map_err(copy_error)?;
    let result = transfer(&mut input, &mut output).map_err(|error| match error {
        CopyStreamError::Cancelled => StorageError::InstanceCreationCancelled,
        CopyStreamError::Read(error) => read_error(error),
        CopyStreamError::Io(error) => copy_error(error),
    })?;
    #[cfg(test)]
    {
        use std::io::Seek;
        crate::instance_archive::read_probe::record(
            source,
            input.stream_position().map_err(read_error)?,
        );
    }
    let permissions = input.metadata().map_err(read_error)?.permissions();
    fs::set_permissions(destination, permissions).map_err(copy_error)?;
    Ok(result)
}

/// The caller owns the pending directory and runs on a blocking worker.
/// Windows scanners may keep a copied executable open after our copy/hash
/// handles close. Directory publication must finish or fail before that owner
/// can release its lease; it must never fall back to a partially copied runtime.
pub(crate) fn publish_creation_directory(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let started = Instant::now();
    publish_directory_with_wait(
        source,
        destination,
        cancellation,
        Duration::from_secs(5),
        || started.elapsed(),
        |delay| {
            let started = Instant::now();
            while started.elapsed() < delay {
                check_creation_cancelled(cancellation)?;
                std::thread::sleep(
                    delay
                        .saturating_sub(started.elapsed())
                        .min(Duration::from_millis(25)),
                );
            }
            Ok(())
        },
    )
}

fn publish_directory_with_wait(
    source: &Path,
    destination: &Path,
    cancellation: Option<&AtomicBool>,
    budget: Duration,
    elapsed: impl Fn() -> Duration,
    mut wait: impl FnMut(Duration) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut delay = Duration::from_millis(25);
    let mut attempts = 0_u32;
    let mut last_error = None;
    let failure = |error, attempts| StorageError::PublishInstanceRuntime {
        from: source.to_owned(),
        to: destination.to_owned(),
        attempts,
        elapsed_ms: elapsed().as_millis(),
        source: error,
    };
    loop {
        check_creation_cancelled(cancellation)?;
        if elapsed() >= budget
            && let Some(error) = last_error.take()
        {
            return Err(failure(error, attempts));
        }
        validate_publication_paths(source, destination)
            .map_err(|error| failure(error, attempts))?;
        attempts += 1;
        match rename_new_runtime(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let remaining = budget.saturating_sub(elapsed());
                if !is_windows_publication_contention(&error) || remaining.is_zero() {
                    return Err(failure(error, attempts));
                }
                last_error = Some(error);
                // Revalidate both paths before every attempt. In particular, a
                // newly appeared destination is a conflict, never a retry target.
                let jitter =
                    Duration::from_millis(u64::from(uuid::Uuid::new_v4().as_bytes()[0] % 26));
                wait((delay + jitter).min(remaining))?;
                delay = (delay * 2).min(Duration::from_millis(200));
            }
        }
    }
}

#[cfg(windows)]
fn rename_new_runtime(source: &Path, destination: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let source = crate::atomic_file::wide_verbatim_path(source)?;
    let destination = crate::atomic_file::wide_verbatim_path(destination)?;
    // std::fs::rename can replace an existing empty directory on Windows.
    // Omit REPLACE_EXISTING so the OS also rejects a target created after our check.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn rename_new_runtime(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn validate_publication_paths(source: &Path, destination: &Path) -> io::Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "runtime destination already exists",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(source)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if !metadata.is_dir() || reparse {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pending runtime must remain a plain directory",
        ));
    }
    Ok(())
}

fn is_windows_publication_contention(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

#[cfg(all(test, windows))]
#[path = "instance_creation_publication_tests.rs"]
mod publication_tests;

enum CopyStreamError {
    Cancelled,
    Read(std::io::Error),
    Io(std::io::Error),
}

fn copy_creation_stream(
    input: &mut impl Read,
    output: &mut impl Write,
    cancellation: Option<&AtomicBool>,
) -> Result<String, CopyStreamError> {
    let mut hasher = Sha256::new();
    copy_creation_stream_with(input, output, cancellation, |bytes| {
        // Digest only the bytes already accepted by the private copy.
        hasher.update(bytes);
        #[cfg(test)]
        if let Some(cancellation) = cancellation {
            test_gate::pause_if_registered(cancellation, test_gate::PausePoint::Hash);
        }
    })?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn copy_creation_stream_with(
    input: &mut impl Read,
    output: &mut impl Write,
    cancellation: Option<&AtomicBool>,
    mut observe: impl FnMut(&[u8]),
) -> Result<(), CopyStreamError> {
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(CopyStreamError::Cancelled);
        }
        let count = input.read(&mut buffer).map_err(CopyStreamError::Read)?;
        if count == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..count])
            .map_err(CopyStreamError::Io)?;
        #[cfg(test)]
        if let Some(cancellation) = cancellation {
            test_gate::pause_if_registered(cancellation, test_gate::PausePoint::Copy);
        }
        observe(&buffer[..count]);
    }
}

#[cfg(test)]
pub(crate) mod test_gate {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock, mpsc};
    use std::time::Duration;

    #[derive(Clone, Copy, Eq, Hash, PartialEq)]
    pub(crate) enum PausePoint {
        Lock,
        Inspect,
        Copy,
        Hash,
    }

    type GateKey = (usize, PausePoint);
    struct Gate {
        reached: tokio::sync::oneshot::Sender<()>,
        resume: mpsc::Receiver<()>,
    }
    static GATES: OnceLock<Mutex<HashMap<GateKey, Gate>>> = OnceLock::new();
    static HASH_READS: OnceLock<Mutex<HashMap<usize, Arc<AtomicUsize>>>> = OnceLock::new();

    pub(crate) struct HashReadCounter {
        cancellation: Arc<AtomicBool>,
        chunks: Arc<AtomicUsize>,
    }

    impl HashReadCounter {
        pub(crate) fn chunks(&self) -> usize {
            self.chunks.load(Ordering::SeqCst)
        }
    }

    impl Drop for HashReadCounter {
        fn drop(&mut self) {
            HASH_READS
                .get()
                .unwrap()
                .lock()
                .unwrap()
                .remove(&(Arc::as_ptr(&self.cancellation) as usize));
        }
    }

    pub(crate) fn count_hash_reads(cancellation: &Arc<AtomicBool>) -> HashReadCounter {
        let chunks = Arc::new(AtomicUsize::new(0));
        assert!(
            HASH_READS
                .get_or_init(Mutex::default)
                .lock()
                .unwrap()
                .insert(Arc::as_ptr(cancellation) as usize, Arc::clone(&chunks))
                .is_none()
        );
        HashReadCounter {
            cancellation: Arc::clone(cancellation),
            chunks,
        }
    }

    pub(crate) struct CreationPause {
        key: GateKey,
        _cancellation: Arc<AtomicBool>,
        reached: tokio::sync::oneshot::Receiver<()>,
        resume: mpsc::Sender<()>,
    }

    impl CreationPause {
        pub(crate) async fn reached(&mut self) {
            tokio::time::timeout(Duration::from_secs(10), &mut self.reached)
                .await
                .expect("instance creation did not reach its I/O boundary")
                .expect("instance creation boundary sender was lost");
        }
    }

    impl Drop for CreationPause {
        fn drop(&mut self) {
            GATES.get().unwrap().lock().unwrap().remove(&self.key);
            let _ = self.resume.send(());
        }
    }

    pub(crate) fn pause_at(cancellation: &Arc<AtomicBool>, point: PausePoint) -> CreationPause {
        let key = (Arc::as_ptr(cancellation) as usize, point);
        let (reached_sender, reached) = tokio::sync::oneshot::channel();
        let (resume, resume_receiver) = mpsc::channel();
        assert!(
            GATES
                .get_or_init(Mutex::default)
                .lock()
                .unwrap()
                .insert(
                    key,
                    Gate {
                        reached: reached_sender,
                        resume: resume_receiver,
                    }
                )
                .is_none()
        );
        CreationPause {
            key,
            _cancellation: Arc::clone(cancellation),
            reached,
            resume,
        }
    }

    pub(crate) fn pause_if_registered(cancellation: &AtomicBool, point: PausePoint) {
        if point == PausePoint::Hash
            && let Some(reads) = HASH_READS.get()
            && let Some(chunks) = reads
                .lock()
                .unwrap()
                .get(&(cancellation as *const AtomicBool as usize))
        {
            chunks.fetch_add(1, Ordering::SeqCst);
        }
        let Some(gates) = GATES.get() else {
            return;
        };
        let gate = gates
            .lock()
            .unwrap()
            .remove(&(cancellation as *const AtomicBool as usize, point));
        if let Some(gate) = gate {
            let _ = gate.reached.send(());
            gate.resume
                .recv_timeout(Duration::from_secs(10))
                .expect("instance creation test did not release its I/O boundary");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_cancellation_stops_between_copy_chunks() {
        struct CancelAfterRead<'a> {
            cancellation: &'a AtomicBool,
            reads: usize,
        }
        impl Read for CancelAfterRead<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.reads += 1;
                assert_eq!(self.reads, 1, "cancelled copy read another chunk");
                buffer.fill(7);
                self.cancellation.store(true, Ordering::Release);
                Ok(buffer.len())
            }
        }
        let cancellation = AtomicBool::new(false);
        let mut input = CancelAfterRead {
            cancellation: &cancellation,
            reads: 0,
        };
        let mut output = Vec::new();
        assert!(matches!(
            copy_creation_stream(&mut input, &mut output, Some(&cancellation)),
            Err(CopyStreamError::Cancelled)
        ));
        assert_eq!(output.len(), 256 * 1024);
    }

    #[test]
    fn streamed_copy_hash_matches_known_digest_across_short_reads() {
        struct OneByteReader(std::io::Cursor<Vec<u8>>);
        impl Read for OneByteReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.0.read(&mut buffer[..1])
            }
        }
        let mut input = OneByteReader(std::io::Cursor::new(b"abc".to_vec()));
        let mut output = Vec::new();
        let digest = copy_creation_stream(&mut input, &mut output, None)
            .unwrap_or_else(|_| panic!("copy failed"));
        assert_eq!(output, b"abc");
        assert_eq!(input.0.position(), 3);
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let digest = copy_creation_stream(&mut &b""[..], &mut Vec::new(), None)
            .unwrap_or_else(|_| panic!("empty copy failed"));
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn copy_failure_never_returns_a_baseline_digest() {
        struct FailedWriter;
        impl Write for FailedWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "fixture write denied",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(matches!(
            copy_creation_stream(&mut &b"abc"[..], &mut FailedWriter, None),
            Err(CopyStreamError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn copy_source_read_failure_retains_its_input_error_boundary() {
        struct FailedReader;
        impl Read for FailedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "fixture read denied",
                ))
            }
        }
        let mut output = Vec::new();
        assert!(matches!(
            copy_creation_stream(&mut FailedReader, &mut output, None),
            Err(CopyStreamError::Read(error)) if error.kind() == io::ErrorKind::PermissionDenied
        ));
        assert!(output.is_empty());
    }
}
