use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const ARCHIVE_URL: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip";
const ARCHIVE_BYTES: u64 = 82_645_522;
const ARCHIVE_SHA256: &str = "c6ba983baf5681af108599675d2a89c2d145512d02de28aed0bff177cd0ba949";
const PREFIX: &str = "onnxruntime-win-x64-1.30.0/";
const ORT_FILES: &[(&str, &str, u64)] = &[
    ("lib/onnxruntime.dll", "onnxruntime.dll", 16_462_648),
    (
        "lib/onnxruntime_providers_shared.dll",
        "onnxruntime_providers_shared.dll",
        21_816,
    ),
    ("LICENSE", "ONNX-RUNTIME-LICENSE.txt", 1_094),
    (
        "ThirdPartyNotices.txt",
        "ONNX-RUNTIME-THIRD-PARTY-NOTICES.txt",
        344_457,
    ),
];
const CRT_NAMES: &[&str] = &[
    "vcruntime140.dll",
    "vcruntime140_1.dll",
    "msvcp140.dll",
    "msvcp140_1.dll",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Redist {
    version_file: PathBuf,
    version: String,
    files: Vec<RedistFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedistFile {
    name: String,
    path: PathBuf,
    version: String,
    sha256: String,
}

#[derive(Serialize)]
struct Manifest {
    onnx_runtime: &'static str,
    target: &'static str,
    crt_version: String,
    files: Vec<ManifestFile>,
}

#[derive(Serialize)]
struct ManifestFile {
    name: String,
    bytes: usize,
    sha256: String,
}

pub fn prepare() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=runtime_build.rs");
    println!("cargo:rerun-if-changed=runtime_build.ps1");
    println!("cargo:rerun-if-env-changed=LANGAME_ORT_ARCHIVE");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("Missing OUT_DIR")?);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64")
    {
        fs::write(
            out.join("embedding_runtime_bundle.rs"),
            "const BUNDLE_ID: &str = \"unsupported\";\nstatic FILES: &[EmbeddedFile] = &[];\n",
        )?;
        return Ok(());
    }
    if !cfg!(windows) {
        return Err(
            "Windows x64 packaging requires a Windows host with stable MSVC Build Tools".into(),
        );
    }
    let directory = out.join("embedding-runtime");
    fs::create_dir_all(&directory)?;
    let archive = verified_archive(&directory)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(archive))?;
    let mut entries = Vec::new();
    for (input, name, expected) in ORT_FILES {
        let mut entry = archive.by_name(&format!("{PREFIX}{input}"))?;
        if entry.size() != *expected || entry.is_dir() {
            return Err(format!("Unexpected official archive entry: {input}").into());
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if bytes.len() as u64 != *expected {
            return Err(format!("Truncated official archive entry: {input}").into());
        }
        if name.ends_with(".dll") {
            validate_pe(&bytes)?;
        }
        entries.push(write_entry(&directory, name, &bytes)?);
    }
    let redist = inspect_redist()?;
    println!("cargo:rerun-if-changed={}", redist.version_file.display());
    if redist.files.len() != CRT_NAMES.len() {
        return Err("Unexpected CRT file set".into());
    }
    let mut versions = std::collections::HashSet::new();
    for name in CRT_NAMES {
        let source = redist
            .files
            .iter()
            .find(|file| file.name == *name)
            .ok_or("Missing required CRT file")?;
        println!("cargo:rerun-if-changed={}", source.path.display());
        let bytes = bounded_read(&source.path, 4 * 1024 * 1024)?;
        if digest(&bytes) != source.sha256 {
            return Err(format!("CRT file changed after signature verification: {name}").into());
        }
        validate_pe(&bytes)?;
        validate_crt_imports(&bytes)?;
        versions.insert(source.version.as_str());
        entries.push(write_entry(&directory, name, &bytes)?);
    }
    if versions.len() != 1 {
        return Err("The CRT files have mixed versions".into());
    }
    let crt_file_version = versions.into_iter().next().ok_or("Missing CRT version")?;
    let crt_parts: Vec<u16> = crt_file_version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    let crt_parts: [u16; 4] = crt_parts
        .try_into()
        .map_err(|_| "Expected four CRT version components")?;
    let notice = format!(
        "Microsoft Visual C++ Runtime\nRedistributable toolset: {}\nFile version: {}\nUnmodified retail x64 redistributables from stable Visual Studio Build Tools.\nRedistribution is subject to the Microsoft Software License Terms for Visual Studio Build Tools and its Distributable Code list:\nhttps://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution\nhttps://visualstudio.microsoft.com/license-terms/\nThese libraries are maintained and updated together with LanGame Server Manager.\n",
        redist.version, crt_file_version
    );
    entries.push(write_entry(
        &directory,
        "MICROSOFT-CRT-NOTICE.txt",
        notice.as_bytes(),
    )?);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let manifest = Manifest {
        onnx_runtime: "1.30.0",
        target: "x86_64-pc-windows-msvc",
        crt_version: redist.version,
        files: entries,
    };
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let bundle_id = digest(&manifest_bytes);
    let manifest_entry = write_entry(&directory, "runtime-manifest.json", &manifest_bytes)?;
    let mut code = format!(
        "const BUNDLE_ID: &str = {bundle_id:?};\nconst CRT_VERSION: [u16; 4] = {crt_parts:?};\nstatic FILES: &[EmbeddedFile] = &[\n"
    );
    for file in manifest
        .files
        .iter()
        .chain(std::iter::once(&manifest_entry))
    {
        code.push_str(&format!("EmbeddedFile {{ name: {:?}, sha256: {:?}, bytes: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/embedding-runtime/{}\")) }},\n", file.name, file.sha256, file.name));
    }
    code.push_str("];\n");
    fs::write(out.join("embedding_runtime_bundle.rs"), code)?;
    Ok(())
}

fn write_entry(root: &Path, name: &str, bytes: &[u8]) -> Result<ManifestFile> {
    fs::write(root.join(name), bytes)?;
    Ok(ManifestFile {
        name: name.to_owned(),
        bytes: bytes.len(),
        sha256: digest(bytes),
    })
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn bounded_read(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > max {
        return Err("Runtime input exceeds its size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err("Runtime input grew beyond its size limit".into());
    }
    Ok(bytes)
}

fn verified_archive(out: &Path) -> Result<Vec<u8>> {
    if let Some(path) = std::env::var_os("LANGAME_ORT_ARCHIVE") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("LANGAME_ORT_ARCHIVE must be absolute".into());
        }
        println!("cargo:rerun-if-changed={}", path.display());
        return check_archive(bounded_read(&path, ARCHIVE_BYTES)?);
    }
    let path = out.join("onnxruntime-win-x64-1.30.0.zip");
    if path.exists() {
        return check_archive(bounded_read(&path, ARCHIVE_BYTES)?);
    }
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let allowed = attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                );
            if !allowed || attempt.previous().len() >= 5 {
                attempt.error("Runtime download redirect is not an approved official host")
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let response = client.get(ARCHIVE_URL).send()?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size != ARCHIVE_BYTES)
    {
        return Err("Runtime archive has an unexpected content length".into());
    }
    let mut bytes = Vec::new();
    response.take(ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
    let bytes = check_archive(bytes)?;
    // OUT_DIR is Cargo-owned. Only verified bytes are published in this cache.
    let staging = out.join(format!("archive-{}.partial", std::process::id()));
    let mut file = File::create_new(&staging)?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&staging, path)
    })();
    if let Err(error) = fs::remove_file(&staging)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!(
            "Cannot clean runtime archive staging: {error}; publication: {result:?}"
        )
        .into());
    }
    result?;
    Ok(bytes)
}

fn check_archive(bytes: Vec<u8>) -> Result<Vec<u8>> {
    if bytes.len() as u64 != ARCHIVE_BYTES || digest(&bytes) != ARCHIVE_SHA256 {
        return Err("Official ONNX Runtime archive size/SHA256 verification failed".into());
    }
    Ok(bytes)
}

fn inspect_redist() -> Result<Redist> {
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows SystemRoot")?);
    let mut command = Command::new(system.join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(include_str!("runtime_build.ps1"))
        // Cargo can inherit PowerShell 7's module paths. Let Windows PowerShell
        // construct its own paths so its signature cmdlet uses the matching host.
        .env_remove("PSModulePath")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("MSVC redistributable signature inspection timed out".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "Stable MSVC retail CRT inspection failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    if output.stdout.len() > 64 * 1024 {
        return Err("MSVC inspection exceeded its output limit".into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn validate_pe(bytes: &[u8]) -> Result<()> {
    let offset = bytes
        .get(0x3c..0x40)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or("Invalid PE header")? as usize;
    if bytes.get(..2) != Some(b"MZ")
        || bytes.get(offset..offset.saturating_add(6)) != Some(b"PE\0\0\x64\x86")
    {
        return Err("Native runtime file is not a Windows x64 PE image".into());
    }
    Ok(())
}

fn validate_crt_imports(bytes: &[u8]) -> Result<()> {
    fn number(bytes: &[u8], offset: usize, size: usize) -> Result<u32> {
        let value = bytes
            .get(offset..offset + size)
            .ok_or("Truncated CRT PE header")?;
        match size {
            2 => Ok(u16::from_le_bytes(value.try_into()?) as u32),
            4 => Ok(u32::from_le_bytes(value.try_into()?)),
            _ => Err("Invalid PE number size".into()),
        }
    }
    let pe = number(bytes, 0x3c, 4)? as usize;
    let optional = pe + 24;
    if number(bytes, optional, 2)? != 0x20b {
        return Err("CRT must use the PE32+ format".into());
    }
    let section_count = number(bytes, pe + 6, 2)? as usize;
    if section_count > 32 {
        return Err("Unexpected CRT section count".into());
    }
    let section_base = optional + number(bytes, pe + 20, 2)? as usize;
    let resolve = |rva: u32| -> Result<usize> {
        for section in 0..section_count {
            let start = section_base + section * 40;
            let address = number(bytes, start + 12, 4)?;
            let size = number(bytes, start + 16, 4)?;
            if let Some(delta) = rva.checked_sub(address)
                && delta < size
            {
                let offset = number(bytes, start + 20, 4)? as usize + delta as usize;
                if offset < bytes.len() {
                    return Ok(offset);
                }
            }
        }
        Err("CRT import points outside its image".into())
    };
    let imports = number(bytes, optional + 120, 4)?;
    if imports == 0 {
        return Ok(());
    }
    let base = resolve(imports)?;
    for index in 0..256 {
        let entry = base + index * 20;
        let descriptor = bytes
            .get(entry..entry + 20)
            .ok_or("Truncated CRT import table")?;
        if descriptor.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        let name = resolve(number(bytes, entry + 12, 4)?)?;
        let end = bytes[name..]
            .iter()
            .take(256)
            .position(|byte| *byte == 0)
            .ok_or("Invalid CRT import name")?;
        let name = std::str::from_utf8(&bytes[name..name + end])?.to_ascii_lowercase();
        let system = name == "kernel32.dll"
            || (name.starts_with("api-ms-win-")
                && name.ends_with(".dll")
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.')));
        if !system && !CRT_NAMES.contains(&name.as_str()) {
            return Err(format!("The selected CRT adds an unbundled dependency ({name}); review and update the runtime whitelist").into());
        }
    }
    Err("CRT import table exceeds its entry budget".into())
}
