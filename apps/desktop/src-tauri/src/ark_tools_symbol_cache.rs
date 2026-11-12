//! AsaApi 2.11 executable-bound cache format and publication. Cache maps are
//! streamed and validated before the small selection metadata is replaced.
use super::*;
use std::collections::HashSet;
use std::io::{BufReader, Seek, SeekFrom};

pub(super) const MAX_ARCHIVE: u64 = 768 * 1024 * 1024;
const MAX_MAP: u64 = 512 * 1024 * 1024;
const KEY: &str = "ArkApi/Cache/cached_key.cache";
const MAPS: [(&str, usize); 2] = [("cached_offsets.cache", 8), ("cached_bitfields.cache", 32)];

fn executable_path_supported(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    };
    #[cfg(not(windows))]
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
    let error = "The ASA extension requires a server executable path that fits its upstream 260-character Windows buffers. Use a shorter, locally representable server program path before preparing the extension.";
    if wide.len() >= 260 {
        return Err(error.into());
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Globalization::{
            GetACP, WC_NO_BEST_FIT_CHARS, WideCharToMultiByte,
        };
        let code_page = unsafe { GetACP() };
        let mut used_default = 0;
        let mut ansi = [0_u8; 260];
        let count = unsafe {
            WideCharToMultiByte(
                code_page,
                if code_page == 65001 {
                    0
                } else {
                    WC_NO_BEST_FIT_CHARS
                },
                wide.as_ptr(),
                wide.len() as i32,
                ansi.as_mut_ptr(),
                ansi.len() as i32,
                std::ptr::null(),
                if code_page == 65001 {
                    std::ptr::null_mut()
                } else {
                    &mut used_default
                },
            )
        };
        if count <= 0 || count >= 260 || used_default != 0 {
            return Err(error.into());
        }
    }
    Ok(())
}

fn cache_total_supported(sizes: impl IntoIterator<Item = u64>) -> bool {
    let mut total = 0;
    for size in sizes {
        if size > MAX_MAP || size > MAX_ARCHIVE - total {
            return false;
        }
        total += size;
    }
    true
}

#[derive(Deserialize, Serialize)]
struct Metadata {
    version: u32,
    executable_hash: String,
    #[serde(default)]
    last_modified: String,
    #[serde(default)]
    cache_directory: String,
}

fn hash(value: &str) -> Option<String> {
    let value = value.trim_matches(|character: char| character.is_ascii_whitespace());
    (value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn generation(value: &str) -> bool {
    let Some(name) = value.strip_prefix("generations/") else {
        return false;
    };
    let parts: Vec<_> = name.split('-').collect();
    parts.len() == 4
        && parts[0].len() == 64
        && hash(parts[0]).is_some()
        && parts[1..]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn metadata(bytes: &[u8]) -> Option<Metadata> {
    let text = std::str::from_utf8(bytes).ok()?;
    if let Ok(mut value) = serde_json::from_str::<Metadata>(text) {
        value.executable_hash = hash(&value.executable_hash)?;
        value.cache_directory = value.cache_directory.replace('\\', "/");
        return (value.version == 1
            && (value.cache_directory.is_empty() || generation(&value.cache_directory)))
        .then_some(value);
    }
    Some(Metadata {
        version: 1,
        executable_hash: hash(text)?,
        last_modified: String::new(),
        cache_directory: String::new(),
    })
}

fn open_regular(path: &Path, limit: u64) -> Result<Option<File>, String> {
    let stat = match fs::symlink_metadata(path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    fn regular(stat: &fs::Metadata, limit: u64) -> bool {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if stat.file_attributes() & 0x400 != 0 {
                return false;
            }
        }
        stat.is_file() && !stat.file_type().is_symlink() && stat.len() <= limit
    }
    if !regular(&stat, limit) {
        return Err(format!(
            "Unsafe or oversized ASA symbol file: {}",
            path.display()
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    if !regular(&file.metadata().map_err(|error| error.to_string())?, limit) {
        return Err("ASA symbol file changed during inspection".into());
    }
    Ok(Some(file))
}

fn executable_hash(file: &mut File) -> Result<String, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_map(file: &mut File, value_size: usize) -> Result<(), String> {
    let mut remaining = file.metadata().map_err(|error| error.to_string())?.len();
    if remaining == 0 || remaining > MAX_MAP {
        return Err("ASA symbol map is empty or oversized".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut input = BufReader::new(file);
    let mut keys = HashSet::new();
    while remaining > 0 {
        if remaining < 8 {
            return Err("ASA symbol map has a truncated key length".into());
        }
        let mut length = [0; 8];
        input
            .read_exact(&mut length)
            .map_err(|error| error.to_string())?;
        remaining -= 8;
        let size = u64::from_le_bytes(length);
        if size == 0
            || size > 1024 * 1024
            || size + value_size as u64 > remaining
            || keys.len() == 5_000_000
        {
            return Err("ASA symbol map has an invalid entry".into());
        }
        let mut key = vec![0; size as usize];
        input
            .read_exact(&mut key)
            .map_err(|error| error.to_string())?;
        if !keys.insert(key) {
            return Err("ASA symbol map has duplicate keys".into());
        }
        let mut value = [0; 32];
        input
            .read_exact(&mut value[..value_size])
            .map_err(|error| error.to_string())?;
        remaining -= size + value_size as u64;
    }
    Ok(())
}

struct Staging {
    path: PathBuf,
    pin: Option<File>,
    retained: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.retained {
            return;
        }
        for name in [
            "symbols.zip",
            "cached_offsets.cache",
            "cached_bitfields.cache",
        ] {
            if let Err(error) = fs::remove_file(self.path.join(name))
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "ASA symbol staging file retained at {}: {error}",
                    self.path.display()
                );
            }
        }
        self.pin.take();
        if let Err(error) = fs::remove_dir(&self.path) {
            eprintln!(
                "ASA symbol staging directory retained at {}: {error}",
                self.path.display()
            );
        }
    }
}

pub(crate) struct Prepared {
    bin: PathBuf,
    executable: File,
    pub(super) executable_hash: String,
    key_before: Option<Vec<u8>>,
    selected: Vec<File>,
    staging: Option<Staging>,
    _pins: Vec<File>,
}

impl Prepared {
    pub(super) fn inspect(root: &Path) -> Result<Self, String> {
        let (bin, mut pins) = files::runtime(root, ASA.executable)?;
        // Upstream resolves the executable through both A/W MAX_PATH buffers.
        // Map paths use filesystem-aware streams and have no blanket 260 limit.
        executable_path_supported(&bin.join(ASA.executable))?;
        let mut executable = open_regular(&bin.join(ASA.executable), u64::MAX)?
            .ok_or("ASA executable is missing")?;
        let executable_hash = executable_hash(&mut executable)?;
        let key_before = if files::pin_parent(&bin, KEY, &mut pins)? {
            open_regular(&bin.join(KEY), 16 * 1024)?
                .map(|mut file| {
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes)
                        .map(|_| bytes)
                        .map_err(|error| error.to_string())
                })
                .transpose()?
        } else {
            None
        };
        let mut selected = Vec::new();
        if let Some(value) = key_before.as_deref().and_then(metadata)
            && value.executable_hash == executable_hash
        {
            let mut candidates = Vec::new();
            for (name, size) in MAPS {
                let relative = if value.cache_directory.is_empty() {
                    format!("ArkApi/Cache/{name}")
                } else {
                    format!("ArkApi/Cache/{}/{name}", value.cache_directory)
                };
                if !files::pin_parent(&bin, &relative, &mut pins)? {
                    break;
                }
                let Some(file) = open_regular(&bin.join(relative), MAX_MAP)? else {
                    break;
                };
                candidates.push((file, size));
            }
            let sizes = candidates
                .iter()
                .map(|(file, _)| file.metadata().map(|metadata| metadata.len()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            if candidates.len() == MAPS.len() && cache_total_supported(sizes) {
                for (mut file, size) in candidates {
                    if validate_map(&mut file, size).is_err() {
                        break;
                    }
                    selected.push(file);
                }
            }
        }
        if selected.len() != MAPS.len() {
            selected.clear();
        }
        Ok(Self {
            bin,
            _pins: pins,
            executable,
            executable_hash,
            key_before,
            selected,
            staging: None,
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.selected.len() == MAPS.len()
    }

    pub(super) fn discard_download(&mut self) {
        self.selected.clear();
        self.staging.take();
    }

    pub(super) fn archive_path(&mut self) -> Result<PathBuf, String> {
        if self.staging.is_none() {
            // This sibling does not create an unmanaged ArkApi directory before
            // the framework transaction performs its ownership preflight.
            let path = self.bin.join(format!(
                ".langame-asa-symbols-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            let pin = files::pin(&path)?;
            self.staging = Some(Staging {
                path,
                pin: Some(pin),
                retained: false,
            });
        }
        Ok(self
            .staging
            .as_ref()
            .ok_or("ASA symbol staging is missing")?
            .path
            .join("symbols.zip"))
    }

    pub(super) fn extract(&mut self) -> Result<(), String> {
        let stage = self
            .staging
            .as_ref()
            .ok_or("ASA symbol staging is missing")?;
        let archive = open_regular(&stage.path.join("symbols.zip"), MAX_ARCHIVE)?
            .ok_or("Downloaded ASA symbol archive is missing")?;
        let mut archive = zip::ZipArchive::new(archive).map_err(|error| error.to_string())?;
        if !(2..=4).contains(&archive.len()) {
            return Err("ASA symbol archive has an unexpected entry count".into());
        }
        let mut names = HashSet::new();
        let mut total = 0;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let name = crate::archive_entry::name(&entry)
                .map_err(|error| error.to_string())?
                .into_owned();
            if !matches!(
                name.as_str(),
                "cached_offsets.cache"
                    | "cached_bitfields.cache"
                    | "cached_key.cache"
                    | "cached_offsets.txt"
            ) || !names.insert(name.clone())
                || entry.is_dir()
                || entry.is_symlink()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
                || entry.size() == 0
                || entry.size() > MAX_MAP
                || total > MAX_ARCHIVE - entry.size()
            {
                return Err(
                    "ASA symbol archive contains an unsafe, duplicate or oversized entry".into(),
                );
            }
            let expected = entry.size();
            total += expected;
            let written = if MAPS.iter().any(|(path, _)| *path == name) {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(stage.path.join(&name))
                    .map_err(|error| error.to_string())?;
                let written =
                    std::io::copy(&mut Read::by_ref(&mut entry).take(MAX_MAP + 1), &mut file)
                        .map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                written
            } else {
                // Archive-provided metadata never chooses the executable hash or path.
                std::io::copy(
                    &mut Read::by_ref(&mut entry).take(MAX_MAP + 1),
                    &mut std::io::sink(),
                )
                .map_err(|error| error.to_string())?
            };
            if written != expected {
                return Err("ASA symbol archive entry length differs".into());
            }
        }
        for (name, size) in MAPS {
            let mut file = open_regular(&stage.path.join(name), MAX_MAP)?
                .ok_or("ASA symbol archive is missing a required map")?;
            validate_map(&mut file, size)?;
            self.selected.push(file);
        }
        Ok(())
    }

    pub(crate) fn publish(mut self) -> Result<(), String> {
        if !self.ready() {
            return Err("ASA symbols were not fully validated".into());
        }
        if executable_hash(&mut self.executable)? != self.executable_hash {
            return Err("ASA executable changed during symbol preparation".into());
        }
        let Some(stage) = self.staging.as_ref() else {
            if files::read(&self.bin, KEY)? != self.key_before {
                return Err("ASA symbol selection changed during preparation".into());
            }
            return Ok(());
        };
        let mut pins = Vec::new();
        let mut created = Vec::new();
        files::prepare_parent(
            &self.bin,
            "ArkApi/Cache/generations/entry",
            &mut pins,
            &mut created,
        )?;
        let directory = format!(
            "generations/{}-{}-0-{}",
            self.executable_hash,
            std::process::id(),
            uuid::Uuid::new_v4().as_fields().0
        );
        let destination = self.bin.join("ArkApi/Cache").join(&directory);
        fs::create_dir(&destination).map_err(|error| error.to_string())?;
        let mut generation = Staging {
            path: destination.clone(),
            pin: Some(files::pin(&destination)?),
            retained: false,
        };
        let result = (|| {
            for (name, _) in MAPS {
                fs::hard_link(stage.path.join(name), destination.join(name))
                    .map_err(|error| error.to_string())?;
            }
            let selection = serde_json::to_vec(&Metadata {
                version: 1,
                executable_hash: self.executable_hash.clone(),
                last_modified: String::new(),
                cache_directory: directory,
            })
            .map_err(|error| error.to_string())?;
            let key = self.bin.join(KEY);
            if files::read(&self.bin, KEY)? != self.key_before {
                return Err(
                    "ASA symbol selection changed concurrently; the existing cache was preserved"
                        .into(),
                );
            }
            let published = match &self.key_before {
                Some(before) => {
                    app_storage::compare_and_swap_file_atomically(&key, before, &selection)
                        .map_err(|error| error.to_string())?
                }
                None => {
                    app_storage::create_file_atomically(&key, &selection)
                        .map_err(|error| error.to_string())?;
                    true
                }
            };
            if !published {
                return Err(
                    "ASA symbol selection changed concurrently; the existing cache was preserved"
                        .into(),
                );
            }
            generation.retained = true;
            Ok(())
        })();
        // Published hard links retain the selected maps when the private staging
        // links are removed. Never clean previous user/cache generations here.
        self.selected.clear();
        result
    }
}

#[cfg(test)]
#[path = "ark_tools_symbol_cache_tests.rs"]
mod tests;
