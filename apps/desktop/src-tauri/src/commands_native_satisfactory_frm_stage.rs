//! Fixed SMR WindowsServer inputs for this opt-in native acceptance only.
//! Product Mod mutations intentionally reject Satisfactory until its installation
//! workflow is modeled; this fixture must not change that production boundary.
#![cfg(test)]
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use super::Result;

struct Package {
    name: &'static str,
    version: &'static str,
    archive_bytes: u64,
    sha256: &'static str,
    game_feature: bool,
}

// https://api.ficsit.app/v1/version/<id>/WindowsServer/download
// SML 74YqQBq2KJKN77; FRM DaDacXi8Lgf6zY, checked 2026-10-27.
const PACKAGES: [Package; 2] = [
    Package {
        name: "SML",
        version: "3.12.0",
        archive_bytes: 19_045_046,
        sha256: "7479e2702da4b81eb5880a98d264d7bbe1fa2831e247e2cbcfb2b8add87512f1",
        game_feature: false,
    },
    Package {
        name: "FicsitRemoteMonitoring",
        version: "1.5.4",
        archive_bytes: 36_570_254,
        sha256: "d860093063192481bd6836295983b9308c58e57c986f1aab727052de88b3f630",
        game_feature: true,
    },
];

struct Member {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

pub(super) struct VerifiedMod {
    source: PathBuf,
    destination: PathBuf,
    members: Vec<Member>,
}

pub(super) fn plain(path: &Path) -> Result<PathBuf> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Satisfactory fixture paths cannot cross reparse points".into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("Satisfactory fixture paths cannot cross symbolic links".into());
        }
    }
    Ok(path.canonicalize()?)
}

fn hash(reader: &mut impl Read) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn member_path(name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if name.contains(['\\', ':'])
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Satisfactory fixture archive has an unsafe member path".into());
    }
    Ok(path.to_owned())
}

pub(super) fn load(source: &Path, archives: &Path) -> Result<Vec<VerifiedMod>> {
    PACKAGES.iter().map(|package| {
        let archive_path = plain(&archives.join(format!("{}-WindowsServer.zip", package.name)))?;
        let mut archive_file = File::open(&archive_path)?;
        if archive_file.metadata()?.len() != package.archive_bytes || hash(&mut archive_file.by_ref().take(package.archive_bytes + 1))? != package.sha256 {
            return Err("Satisfactory fixture Mod archive differs from its reviewed SMR WindowsServer artifact".into());
        }
        archive_file.seek(SeekFrom::Start(0))?;
        let mut archive = zip::ZipArchive::new(archive_file)?;
        if archive.len() > 4096 { return Err("Satisfactory fixture Mod archive exceeds the member limit".into()); }
        let descriptor_name = format!("{}.uplugin", package.name);
        let descriptor: serde_json::Value = serde_json::from_reader(archive.by_name(&descriptor_name)?.take(64 * 1024))?;
        if descriptor["VersionName"].as_str() != Some(package.version)
            || descriptor["GameFeature"].as_bool().unwrap_or(false) != package.game_feature {
            return Err("Satisfactory fixture Mod descriptor differs from the reviewed version or layout".into());
        }
        // Official ManualInstallDirections: GameFeature=true routes below
        // Mods/GameFeatures; the folder name must equal the .uplugin name.
        let relative = if package.game_feature { Path::new("GameFeatures").join(package.name) } else { PathBuf::from(package.name) };
        let source = plain(&source.join("FactoryGame/Mods").join(&relative))?;
        let mut seen = BTreeSet::new();
        let mut members = Vec::new();
        let mut total = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() { continue; }
            let path = member_path(entry.name()?.as_ref())?;
            if entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
                || !seen.insert(path.to_string_lossy().to_ascii_lowercase()) {
                return Err("Satisfactory fixture Mod archive has a link or duplicate member".into());
            }
            total = total.checked_add(entry.size()).ok_or("Satisfactory fixture Mod size overflowed")?;
            if total > 256 * 1024 * 1024 { return Err("Satisfactory fixture Mod exceeds the byte limit".into()); }
            let sha256 = hash(&mut entry)?;
            let mut input = File::open(plain(&source.join(&path))?)?;
            if input.metadata()?.len() != entry.size() || hash(&mut input.by_ref().take(entry.size() + 1))? != sha256 {
                return Err("Satisfactory native source Mod member differs from the reviewed archive".into());
            }
            members.push(Member { path, bytes: entry.size(), sha256 });
        }
        Ok(VerifiedMod { source, destination: relative, members })
    }).collect()
}

pub(super) fn stage(mods: &[VerifiedMod], runtime: &Path) -> Result<()> {
    let target = runtime.join("FactoryGame/Mods");
    if target.exists() {
        return Err(
            "Satisfactory FRM acceptance requires an unmodified owned runtime Mod directory".into(),
        );
    }
    fs::create_dir(&target)?;
    for package in mods {
        let root = target.join(&package.destination);
        for member in &package.members {
            let destination = root.join(&member.path);
            fs::create_dir_all(
                destination
                    .parent()
                    .ok_or("Satisfactory fixture Mod member has no parent")?,
            )?;
            let mut input = File::open(plain(&package.source.join(&member.path))?)?;
            let mut output = File::options()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let copied = std::io::copy(&mut input.by_ref().take(member.bytes + 1), &mut output)?;
            output.sync_all()?;
            drop(output);
            if copied != member.bytes
                || hash(&mut File::open(plain(&destination)?)?)? != member.sha256
            {
                return Err("Satisfactory fixture Mod changed during staging".into());
            }
        }
    }
    println!(
        "NATIVE_SATISFACTORY_FRM phase=staged fixture_only=true sml=3.12.0 frm=1.5.4 target=owned_runtime source_members=verified_smr_windows_server production_installer=not_claimed"
    );
    Ok(())
}

pub(super) fn settings(existing: &str, port: u16) -> Result<String> {
    if existing.len() > 256 * 1024 || existing.contains("FicsitRemoteMonitoring.") {
        return Err("Satisfactory FRM fixture requires fresh bounded native settings".into());
    }
    let mut lines = existing.lines().map(String::from).collect::<Vec<_>>();
    let sections = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "[/Script/FactoryGame.FGGameUserSettings]")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if sections.len() > 1 {
        return Err("Satisfactory FRM fixture native settings section is ambiguous".into());
    }
    let start = if let Some(&start) = sections.first() {
        start
    } else {
        lines.push("[/Script/FactoryGame.FGGameUserSettings]".into());
        lines.len() - 1
    };
    let end = (start + 1..lines.len())
        .find(|&index| lines[index].trim().starts_with('['))
        .unwrap_or(lines.len());
    let mut insert = Vec::new();
    for (key, additions) in [
        (
            "mIntValues",
            format!(
                "(\"FicsitRemoteMonitoring.Server.uWS.Autostart\",1),(\"FicsitRemoteMonitoring.Server.uWS.Port\",{port})"
            ),
        ),
        (
            "mStringValues",
            "(\"FicsitRemoteMonitoring.Server.uWS.AuthenticationToken\",\"synthetic-test-only\")"
                .into(),
        ),
    ] {
        let indices = (start + 1..end)
            .filter(|&index| {
                lines[index]
                    .trim()
                    .split_once('=')
                    .is_some_and(|(name, _)| {
                        name.trim().trim_start_matches(['+', '-', '.', '!']) == key
                    })
            })
            .collect::<Vec<_>>();
        if indices.len() > 1 {
            return Err("Satisfactory FRM fixture native option map is ambiguous".into());
        }
        if let Some(&index) = indices.first() {
            let (name, value) = lines[index]
                .trim()
                .split_once('=')
                .ok_or("Satisfactory fixture native map is invalid")?;
            let inside = value
                .trim()
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
                .ok_or("Satisfactory fixture native map is invalid")?;
            if name != key {
                return Err("Satisfactory fixture native map uses an ambiguous operator".into());
            }
            lines[index] = format!(
                "{key}=({inside}{}{additions})",
                if inside.trim().is_empty() { "" } else { "," }
            );
        } else {
            insert.push(format!("{key}=({additions})"));
        }
    }
    lines.splice(end..end, insert);
    Ok(format!("{}\n", lines.join("\n")))
}
