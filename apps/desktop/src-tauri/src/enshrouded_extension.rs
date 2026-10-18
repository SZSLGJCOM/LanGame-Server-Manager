//! Independently signed, exact-build Enshrouded control updates. No arbitrary URLs.
use std::{collections::HashSet, fs, path::Path};

use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::live_players::enshrouded_native::io;

const PUBLIC_KEY: &str =
    include_str!("../../../../modules/enshrouded/control/update-public-key.pub");
pub(crate) const MANIFEST: &str = "extension-manifest.json";
pub(crate) const MANIFEST_SIGNATURE: &str = "extension-manifest.json.minisig";
pub(crate) const DLL_SIGNATURE: &str = "extension-dll.minisig";
const MAX_MANIFEST: usize = 32 * 1024;
pub(crate) const MAX_DLL: usize = 1024 * 1024;
const MAX_SIGNATURE: usize = 2048;
const MAX_CACHED_PACKAGES: usize = 64;
const WATERMARK: &str = "highest-manifest.json";
const WATERMARK_SIGNATURE: &str = "highest-manifest.json.minisig";

#[path = "enshrouded_extension_update.rs"]
mod update;
pub(crate) use update::{RefreshResult, start_refresh};

pub(crate) struct Resolution {
    pub(crate) package: Option<SignedPackage>,
    pub(crate) source: &'static str,
    pub(crate) version: Option<String>,
    pub(crate) update_issue: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    module_id: String,
    protocol: u32,
    target: String,
    version: String,
    revision: u64,
    profiles: Vec<Profile>,
    dll: Artifact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    game_build_id: String,
    executable_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    asset_name: String,
    sha256: String,
    size: usize,
}

#[derive(Clone)]
pub(crate) struct SignedPackage {
    manifest: Manifest,
    manifest_bytes: Vec<u8>,
    manifest_signature: Vec<u8>,
    dll: Vec<u8>,
    dll_signature: Vec<u8>,
}

impl SignedPackage {
    pub(crate) fn payload(&self) -> &[u8] {
        &self.dll
    }

    pub(crate) fn publish_proof(&self, directory: &Path) -> Result<(), String> {
        io::publish(&directory.join(MANIFEST), &self.manifest_bytes)?;
        io::publish(
            &directory.join(MANIFEST_SIGNATURE),
            &self.manifest_signature,
        )?;
        io::publish(&directory.join(DLL_SIGNATURE), &self.dll_signature)
    }
}

fn sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn version(text: &str) -> bool {
    text.len() <= 48
        && text.split('.').count() == 3
        && text.split('.').all(|part| {
            !part.is_empty()
                && (part == "0" || !part.starts_with('0'))
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}

fn signature(key: &PublicKey, bytes: &[u8], proof: &[u8], purpose: &str) -> Result<(), String> {
    let proof = std::str::from_utf8(proof).map_err(|_| "Invalid extension signature encoding.")?;
    if proof.len() > MAX_SIGNATURE || proof.lines().count() != 4 {
        return Err("Invalid extension signature size or framing.".into());
    }
    let signature = Signature::decode(proof).map_err(|_| "Invalid extension signature.")?;
    key.verify(bytes, &signature, false)
        .map_err(|_| "Extension signature verification failed.")?;
    if signature.trusted_comment() != purpose {
        return Err("The extension signature has the wrong purpose or version.".into());
    }
    Ok(())
}

fn manifest(bytes: &[u8], proof: &[u8], key: &PublicKey) -> Result<Manifest, String> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST {
        return Err("Extension manifest exceeds its size limit.".into());
    }
    signature(key, bytes, proof, "lgsm-enshrouded-control-manifest:1")?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid extension manifest.")?;
    let mut hashes = HashSet::new();
    let mut builds = HashSet::new();
    if manifest.schema != 1
        || manifest.module_id != "enshrouded"
        || manifest.protocol != 1
        || manifest.target != "windows-x86_64"
        || !version(&manifest.version)
        || manifest.revision == 0
        || manifest.profiles.is_empty()
        || manifest.profiles.len() > 64
        || manifest.profiles.iter().any(|profile| {
            !sha256(&profile.executable_sha256)
                || !hashes.insert(&profile.executable_sha256)
                || profile.game_build_id.is_empty()
                || profile.game_build_id.len() > 20
                || !profile.game_build_id.bytes().all(|b| b.is_ascii_digit())
                || !builds.insert(&profile.game_build_id)
        })
        || !sha256(&manifest.dll.sha256)
        || manifest.dll.size == 0
        || manifest.dll.size > MAX_DLL
        || manifest.dll.asset_name
            != format!(
                "Lgsm.Enshrouded.Control_{}_windows-x86_64.dll",
                manifest.version
            )
    {
        return Err("The signed extension manifest is incompatible or invalid.".into());
    }
    Ok(manifest)
}

fn admit_manifest(
    bytes: &[u8],
    proof: &[u8],
    key: &PublicKey,
    minimum_revision: u64,
) -> Result<Manifest, String> {
    let manifest = manifest(bytes, proof, key)?;
    if manifest.revision < minimum_revision {
        return Err("The extension update manifest would roll back a trusted revision.".into());
    }
    Ok(manifest)
}

fn verify_package(
    manifest_bytes: Vec<u8>,
    manifest_signature: Vec<u8>,
    dll: Vec<u8>,
    dll_signature: Vec<u8>,
    executable: &str,
    key: &PublicKey,
) -> Result<SignedPackage, String> {
    let manifest = manifest(&manifest_bytes, &manifest_signature, key)?;
    if !manifest
        .profiles
        .iter()
        .any(|p| p.executable_sha256 == executable)
        || dll.len() != manifest.dll.size
        || io::digest(&dll) != manifest.dll.sha256
    {
        return Err("The extension payload does not match this exact server executable.".into());
    }
    signature(
        key,
        &dll,
        &dll_signature,
        &format!("lgsm-enshrouded-control-dll:{}", manifest.version),
    )?;
    Ok(SignedPackage {
        manifest,
        manifest_bytes,
        manifest_signature,
        dll,
        dll_signature,
    })
}

fn key() -> Result<PublicKey, String> {
    PublicKey::decode(PUBLIC_KEY)
        .map_err(|_| "The embedded extension signing key is invalid.".into())
}

fn read_package(
    directory: &Path,
    dll: &Path,
    executable: &str,
    key: &PublicKey,
) -> Result<SignedPackage, String> {
    let _directory = io::pin_directory(directory)?;
    verify_package(
        io::read_regular(&directory.join(MANIFEST), MAX_MANIFEST)?,
        io::read_regular(&directory.join(MANIFEST_SIGNATURE), MAX_SIGNATURE)?,
        io::read_regular(dll, MAX_DLL)?,
        io::read_regular(&directory.join(DLL_SIGNATURE), MAX_SIGNATURE)?,
        executable,
        key,
    )
}

/// Revalidates signatures from the installed evidence, never trusting an owner hash alone.
pub(crate) fn verify_installed(root: &Path, executable: &str, dll: &[u8]) -> Result<(), String> {
    if io::BUNDLED_EXE_SHA256.contains(&executable)
        && !io::bundled_payload().is_empty()
        && io::digest(dll) == io::digest(io::bundled_payload())
    {
        return Ok(());
    }
    let directory = root.join(io::DIRECTORY);
    verify_package(
        io::read_regular(&directory.join(MANIFEST), MAX_MANIFEST)?,
        io::read_regular(&directory.join(MANIFEST_SIGNATURE), MAX_SIGNATURE)?,
        dll.to_owned(),
        io::read_regular(&directory.join(DLL_SIGNATURE), MAX_SIGNATURE)?,
        executable,
        &key()?,
    )
    .map(|_| ())
}

fn ensure_cache(directory: &Path) -> Result<(), String> {
    // Pin every existing ancestor before creating a descendant; no cache path may traverse a junction.
    let mut ancestors = directory.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    let mut guards = Vec::new();
    for path in ancestors {
        if !path.exists() {
            fs::create_dir(path).map_err(|_| "Cannot create the extension cache.")?;
        }
        guards.push(io::pin_directory(path)?);
    }
    Ok(())
}

fn cached(
    directory: &Path,
    executable: &str,
    key: &PublicKey,
) -> Result<(Option<SignedPackage>, u64), String> {
    ensure_cache(directory)?;
    let _guard = io::pin_directory(directory)?;
    let mut newest: Option<SignedPackage> = None;
    let mut revision = 0;
    if let (Ok(bytes), Ok(proof)) = (
        io::read_regular(&directory.join(WATERMARK), MAX_MANIFEST),
        io::read_regular(&directory.join(WATERMARK_SIGNATURE), MAX_SIGNATURE),
    ) && let Ok(manifest) = manifest(&bytes, &proof, key)
    {
        revision = manifest.revision;
    }
    for (index, entry) in fs::read_dir(directory)
        .map_err(|_| "Cannot enumerate the extension cache.")?
        .enumerate()
    {
        if index >= MAX_CACHED_PACKAGES {
            return Err("Extension cache exceeded its package limit.".into());
        }
        let entry = entry.map_err(|_| "Cannot inspect an extension cache entry.")?;
        let name = entry.file_name();
        if !name.to_str().is_some_and(sha256) {
            continue;
        }
        let path = entry.path();
        let Ok(_guard) = io::pin_directory(&path) else {
            continue;
        };
        let Ok(bytes) = io::read_regular(&path.join(MANIFEST), MAX_MANIFEST) else {
            continue;
        };
        let Ok(proof) = io::read_regular(&path.join(MANIFEST_SIGNATURE), MAX_SIGNATURE) else {
            continue;
        };
        if io::digest(&bytes) != name.to_string_lossy() {
            continue;
        }
        let Ok(manifest) = manifest(&bytes, &proof, key) else {
            continue;
        };
        // A verified manifest records the high-water mark even if a cached DLL was lost.
        revision = revision.max(manifest.revision);
        if let Ok(package) = read_package(&path, &path.join("dbghelp.dll"), executable, key)
            && newest
                .as_ref()
                .is_none_or(|old| package.manifest.revision > old.manifest.revision)
        {
            newest = Some(package);
        }
    }
    Ok((newest, revision))
}

fn cache(directory: &Path, package: &SignedPackage) -> Result<(), String> {
    let _guard = io::pin_directory(directory)?;
    let destination = directory.join(io::digest(&package.manifest_bytes));
    match fs::create_dir(&destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("Cannot create an extension cache package.".into()),
    }
    let _guard = io::pin_directory(&destination)?;
    package.publish_proof(&destination)?;
    io::publish(&destination.join("dbghelp.dll"), &package.dll)?;
    prune_cache(directory, &key()?)
}

fn remember_manifest(directory: &Path, bytes: &[u8], proof: &[u8]) -> Result<(), String> {
    let _guard = io::pin_directory(directory)?;
    // Both files are signed and reverified on every read. An interrupted pair can
    // never authorize code; complete package manifests retain the watermark too.
    io::publish(&directory.join(WATERMARK), bytes)?;
    io::publish(&directory.join(WATERMARK_SIGNATURE), proof)
}

fn prune_cache(directory: &Path, key: &PublicKey) -> Result<(), String> {
    let mut packages = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| "Cannot inspect the extension cache.")? {
        let entry = entry.map_err(|_| "Cannot inspect an extension cache package.")?;
        if !entry.file_name().to_str().is_some_and(sha256) {
            continue;
        }
        let path = entry.path();
        let Ok(_guard) = io::pin_directory(&path) else {
            continue;
        };
        let Ok(bytes) = io::read_regular(&path.join(MANIFEST), MAX_MANIFEST) else {
            continue;
        };
        let Ok(proof) = io::read_regular(&path.join(MANIFEST_SIGNATURE), MAX_SIGNATURE) else {
            continue;
        };
        if io::digest(&bytes) != entry.file_name().to_string_lossy() {
            continue;
        }
        if let Ok(manifest) = manifest(&bytes, &proof, key) {
            packages.push((manifest.revision, path));
        }
    }
    packages.sort_by_key(|package| std::cmp::Reverse(package.0));
    for (_, path) in packages.into_iter().skip(8) {
        let _guard = io::pin_directory(&path)?;
        // Remove only known bounded plain files under a verified content-addressed directory.
        for name in [MANIFEST, MANIFEST_SIGNATURE, DLL_SIGNATURE, "dbghelp.dll"] {
            let file = path.join(name);
            if io::read_regular(&file, MAX_DLL).is_ok() {
                let _ = fs::remove_file(file);
            }
        }
        drop(_guard);
        let _ = fs::remove_dir(path);
    }
    Ok(())
}

/// Local selection runs in the stopped-instance blocking worker. No network work
/// participates in preparing an instance; refreshed packages apply on its next start.
pub(crate) fn resolve_local(cache_root: &Path, executable: &str) -> Resolution {
    let fallback = |package: Option<SignedPackage>, update_issue| Resolution {
        version: package.as_ref().map(|p| p.manifest.version.clone()),
        source: if package.is_some() {
            "signed_cache"
        } else if io::BUNDLED_EXE_SHA256.contains(&executable) {
            "bundled"
        } else {
            "unsupported"
        },
        package,
        update_issue,
    };
    match key().and_then(|key| {
        cached(
            &cache_root.join("cache/enshrouded-control"),
            executable,
            &key,
        )
    }) {
        Ok((package, _)) => fallback(package, None),
        Err(error) => fallback(None, Some(error)),
    }
}
#[cfg(test)]
#[path = "enshrouded_extension_tests.rs"]
mod tests;
