//! Offline delivery and process-owned initialization of the pinned CPU runtime.
//! Runtime code is shipped inside the application, never downloaded with a model.

use std::path::Path;

use crate::{KnowledgeError, Result};

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::*;
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use sha2::{Digest, Sha256};
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileVersionInfoSizeW, GetFileVersionInfoW,
        VS_FIXEDFILEINFO, VerQueryValueW,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    struct EmbeddedFile {
        name: &'static str,
        sha256: &'static str,
        bytes: &'static [u8],
    }

    include!(concat!(env!("OUT_DIR"), "/embedding_runtime_bundle.rs"));

    const LOAD_ORDER: &[&str] = &[
        "vcruntime140.dll",
        "vcruntime140_1.dll",
        "msvcp140.dll",
        "msvcp140_1.dll",
        "onnxruntime_providers_shared.dll",
        "onnxruntime.dll",
    ];

    static OWNER: Mutex<Option<RuntimeOwner>> = Mutex::new(None);

    struct RuntimeOwner {
        // Preserve loader references before releasing the file/directory guards.
        _libraries: Vec<NativeLibrary>,
        _system_locks: Vec<File>,
        _installation: RuntimeInstallation,
        bundle_id: &'static str,
        initialized: std::result::Result<(), String>,
    }

    struct RuntimeInstallation {
        directory: PathBuf,
        _locks: Vec<File>,
    }

    struct NativeLibrary(HMODULE);

    // Windows module reference counting is process-wide and supports releasing
    // a LoadLibraryEx reference on a different thread. All access stays in OWNER.
    unsafe impl Send for NativeLibrary {}

    impl Drop for NativeLibrary {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns exactly one successful loader reference.
            unsafe { FreeLibrary(self.0) };
        }
    }

    fn runtime_error(error: impl std::fmt::Display) -> KnowledgeError {
        KnowledgeError::Model(format!("Bundled ONNX Runtime: {error}"))
    }

    pub(super) fn initialize(model_dir: &Path) -> Result<()> {
        if !model_dir.is_absolute() {
            return Err(runtime_error("model directory must be absolute"));
        }
        let mut owner = OWNER
            .lock()
            .map_err(|_| runtime_error("initialization lock poisoned"))?;
        if let Some(owner) = owner.as_ref() {
            if owner.bundle_id != BUNDLE_ID {
                return Err(runtime_error(
                    "a different native runtime was already initialized",
                ));
            }
            return owner.initialized.clone().map_err(runtime_error);
        }
        let installation = ensure_runtime(model_dir)?;
        let mut libraries = Vec::with_capacity(LOAD_ORDER.len());
        let mut system_locks = Vec::new();
        for name in LOAD_ORDER {
            libraries.push(preload(
                &installation.directory.join(name),
                &mut system_locks,
            )?);
        }
        let api = native_api(
            libraries
                .last()
                .ok_or_else(|| runtime_error("missing ORT module"))?,
        )?;
        // Keep guards even if ORT configuration fails: the crate may already own
        // a global library/API reference, which remains valid until process exit.
        *owner = Some(RuntimeOwner {
            _libraries: libraries,
            _system_locks: system_locks,
            _installation: installation,
            bundle_id: BUNDLE_ID,
            initialized: Err("initialization was interrupted".into()),
        });
        let initialized = (|| {
            // Publish only the API table obtained from our protected module.
            // alternative-backend prevents any implicit DLL/path/env loading.
            if !ort::set_api(api) {
                return Err("ORT API was initialized outside the verified runtime owner".to_owned());
            }
            if !ort::init().with_telemetry(false).commit() {
                return Err("ORT was configured outside the verified runtime owner".to_owned());
            }
            ort::environment::Environment::current().map_err(|error| error.to_string())?;
            Ok(())
        })();
        owner
            .as_mut()
            .expect("runtime owner stored above")
            .initialized = initialized.clone();
        initialized.map_err(runtime_error)
    }

    fn ensure_runtime(model_dir: &Path) -> Result<RuntimeInstallation> {
        if !model_dir.is_absolute() {
            return Err(runtime_error("model directory must be absolute"));
        }
        let mut locks = vec![lock_directory(model_dir)?];
        let root = model_dir.join("native-runtime");
        create_directory(&root)?;
        locks.push(lock_directory(&root)?);
        // Serializes publication across processes until every verified reader
        // handle is held. Rename's transient DELETE access conflicts with those
        // readers even when another publisher subsequently observes the file.
        let _installer = installation_lock(&root, Instant::now() + Duration::from_secs(30))?;
        let directory = root.join(BUNDLE_ID);
        create_directory(&directory)?;
        locks.push(lock_directory(&directory)?);
        for embedded in FILES {
            let path = directory.join(embedded.name);
            match fs::symlink_metadata(&path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    publish_file(&directory, embedded)?;
                }
                Err(error) => return Err(runtime_error(error)),
            }
            locks.push(lock_verified(&path, embedded)?);
        }
        Ok(RuntimeInstallation {
            directory,
            _locks: locks,
        })
    }

    fn installation_lock(root: &Path, deadline: Instant) -> Result<File> {
        // Keep this stable sibling file after release: deleting a lock file can
        // split waiters between old and newly created filesystem objects.
        let path = root.join(format!(".install-{BUNDLE_ID}.lock"));
        loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)
            {
                Ok(file) => {
                    let metadata = file.metadata().map_err(runtime_error)?;
                    if !metadata.is_file()
                        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                    {
                        return Err(runtime_error(
                            "runtime installation lock must be a plain file",
                        ));
                    }
                    return Ok(file);
                }
                Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(runtime_error(
                            "timed out waiting for the runtime installation lock",
                        ));
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                Err(error) => return Err(runtime_error(error)),
            }
        }
    }

    fn create_directory(path: &Path) -> Result<()> {
        match fs::create_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(runtime_error(error)),
        }
    }

    fn lock_directory(path: &Path) -> Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(runtime_error)?;
        let metadata = file.metadata().map_err(runtime_error)?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(runtime_error(
                "runtime directories must be plain directories, not links or junctions",
            ));
        }
        // No FILE_SHARE_DELETE: neither this directory nor a guarded parent can
        // be renamed/replaced while files are verified, loaded, or in use.
        Ok(file)
    }

    fn lock_verified(path: &Path, embedded: &EmbeddedFile) -> Result<File> {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(runtime_error)?;
        let metadata = file.metadata().map_err(runtime_error)?;
        if !metadata.is_file()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata.len() != embedded.bytes.len() as u64
        {
            return Err(runtime_error(format!(
                "invalid bundled file: {}",
                embedded.name
            )));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(runtime_error)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let actual: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != embedded.sha256 {
            return Err(runtime_error(format!(
                "bundled file SHA256 mismatch: {}",
                embedded.name
            )));
        }
        // No FILE_SHARE_WRITE/DELETE. The verified bytes cannot change before
        // LoadLibraryEx or while the native code remains owned by this process.
        Ok(file)
    }

    fn publish_file(root: &Path, embedded: &EmbeddedFile) -> Result<()> {
        let staging = root.join(format!(
            ".{}-{}.partial",
            embedded.name,
            uuid::Uuid::new_v4()
        ));
        let destination = root.join(embedded.name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .open(&staging)
            .map_err(runtime_error)?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(embedded.bytes)?;
            file.sync_all()?;
            drop(file);
            // The installation lock serializes all cooperating publishers.
            // Unexpected destination changes fail rather than hiding IO errors.
            fs::rename(&staging, &destination)
        })();
        if let Err(cleanup) = fs::remove_file(&staging)
            && cleanup.kind() != std::io::ErrorKind::NotFound
        {
            return Err(runtime_error(format!(
                "cannot remove runtime staging file: {cleanup}; publication: {result:?}"
            )));
        }
        result.map_err(runtime_error)
    }

    fn preload(path: &Path, system_locks: &mut Vec<File>) -> Result<NativeLibrary> {
        let path = fs::canonicalize(path).map_err(runtime_error)?;
        let base_name = path
            .file_name()
            .ok_or_else(|| runtime_error("missing library name"))?;
        let base_name_wide: Vec<u16> = base_name.encode_wide().chain(Some(0)).collect();
        let mut existing = std::ptr::null_mut();
        // SAFETY: the closed library name is NUL-terminated. No UNCHANGED_REFCOUNT:
        // a returned module stays live during path/version inspection.
        if unsafe { GetModuleHandleExW(0, base_name_wide.as_ptr(), &mut existing) } != 0 {
            let library = NativeLibrary(existing);
            let actual = module_path(&library)?;
            if actual == path {
                return Ok(library);
            }
            let crt = base_name.to_str().is_some_and(|name| {
                matches!(
                    name,
                    "vcruntime140.dll" | "vcruntime140_1.dll" | "msvcp140.dll" | "msvcp140_1.dll"
                )
            });
            if !crt || actual != system_directory()?.join(base_name) {
                return Err(runtime_error(format!(
                    "conflicting already-loaded native library: {}",
                    base_name.to_string_lossy()
                )));
            }
            let guard = lock_plain_file(&actual)?;
            if file_version(&actual)? < CRT_VERSION {
                return Err(runtime_error(format!(
                    "already-loaded System32 CRT is older than the bundled baseline: {}",
                    base_name.to_string_lossy()
                )));
            }
            system_locks.push(guard);
            return Ok(library);
        }
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: null-terminated absolute path names a locked, hash-verified
        // embedded PE. Private dependencies were explicitly preloaded above;
        // unresolved dependencies search System32 only, never the writable cache.
        let module = unsafe {
            LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return Err(runtime_error(std::io::Error::last_os_error()));
        }
        let library = NativeLibrary(module);
        if module_path(&library)? != path {
            return Err(runtime_error(
                "Windows loaded a different native runtime file",
            ));
        }
        Ok(library)
    }

    fn module_path(library: &NativeLibrary) -> Result<PathBuf> {
        let mut actual = vec![0u16; 32_768];
        // SAFETY: a live module and writable buffer of the declared length.
        let len = unsafe { GetModuleFileNameW(library.0, actual.as_mut_ptr(), actual.len() as u32) }
            as usize;
        if len == 0 || len >= actual.len() {
            return Err(runtime_error("cannot identify the loaded native module"));
        }
        let actual = PathBuf::from(std::ffi::OsString::from_wide(&actual[..len]));
        fs::canonicalize(actual).map_err(runtime_error)
    }

    fn system_directory() -> Result<PathBuf> {
        let mut buffer = vec![0u16; 32_768];
        // SAFETY: writable buffer; the system API supplies the trusted location.
        let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        if len == 0 || len >= buffer.len() {
            return Err(runtime_error("cannot locate the Windows system directory"));
        }
        fs::canonicalize(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
            .map_err(runtime_error)
    }

    fn lock_plain_file(path: &Path) -> Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(runtime_error)?;
        let metadata = file.metadata().map_err(runtime_error)?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(runtime_error("loaded System32 CRT must be a plain file"));
        }
        Ok(file)
    }

    fn file_version(path: &Path) -> Result<[u16; 4]> {
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: path is NUL-terminated and the underlying file is held locked.
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
        if size == 0 || size > 1024 * 1024 {
            return Err(runtime_error("invalid CRT version resource size"));
        }
        let mut bytes = vec![0u8; size as usize];
        // SAFETY: buffer matches the size returned for this locked file.
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, bytes.as_mut_ptr().cast()) } == 0 {
            return Err(runtime_error("cannot read CRT file version"));
        }
        let mut value = std::ptr::null_mut();
        let mut length = 0;
        // SAFETY: version buffer remains live, root sub-block is NUL-terminated.
        if unsafe {
            VerQueryValueW(
                bytes.as_ptr().cast(),
                [b'\\' as u16, 0].as_ptr(),
                &mut value,
                &mut length,
            )
        } == 0
            || value.is_null()
            || (length as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return Err(runtime_error("missing CRT fixed version information"));
        }
        // SAFETY: Windows returned a valid fixed-version block of the checked size.
        let version = unsafe { std::ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>()) };
        if version.dwSignature != 0xFEEF04BD {
            return Err(runtime_error("invalid CRT version signature"));
        }
        Ok([
            (version.dwFileVersionMS >> 16) as u16,
            version.dwFileVersionMS as u16,
            (version.dwFileVersionLS >> 16) as u16,
            version.dwFileVersionLS as u16,
        ])
    }

    fn native_api(library: &NativeLibrary) -> Result<ort::sys::OrtApi> {
        type GetBase = unsafe extern "system" fn() -> *const ort::sys::OrtApiBase;
        // SAFETY: only the verified ONNX Runtime module is inspected.
        let getter = unsafe { GetProcAddress(library.0, c"OrtGetApiBase".as_ptr().cast()) }
            .ok_or_else(|| runtime_error("missing OrtGetApiBase export"))?;
        // SAFETY: the pinned official export has this published C ABI signature.
        let getter: GetBase = unsafe { std::mem::transmute(getter) };
        let base = unsafe { getter() };
        if base.is_null() {
            return Err(runtime_error("missing ONNX Runtime API base"));
        }
        // SAFETY: the base points into the lifetime-owned, pinned official module.
        let api = unsafe { ((*base).GetApi)(ort::sys::ORT_API_VERSION) };
        if api.is_null() {
            return Err(runtime_error(
                "native runtime does not support the selected API version",
            ));
        }
        // SAFETY: GetApi returned the requested ABI; set_api copies its stable table.
        Ok(unsafe { std::ptr::read(api) })
    }

    #[cfg(test)]
    mod tests {
        include!("embedding_runtime_tests.rs");
    }
}

pub(crate) fn initialize(model_dir: &Path) -> Result<()> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        windows::initialize(model_dir)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = model_dir;
        Err(KnowledgeError::Unavailable(
            "The bundled local semantic runtime supports Windows x64".into(),
        ))
    }
}
