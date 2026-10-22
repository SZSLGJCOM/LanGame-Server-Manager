use super::{DIRECTORY, OWNER, Ownership, Profile, io, redloader};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const PROTOCOL: &str = include_str!("../../../../modules/corekeeper/control/SaveProtocol.cs");
const FILES: &str = include_str!("../../../../modules/corekeeper/control/SaveFiles.cs");
const CORE: &str = include_str!("../../../../modules/corekeeper/control/CoreKeeperSave.cs");
const CORE_EXTRA: &[(&str, &str)] = &[(
    "CoreKeeperSaveNative.cs",
    include_str!("../../../../modules/corekeeper/control/CoreKeeperSaveNative.cs"),
)];
const VRISING: &str = include_str!("../../../../modules/vrising/control/VRisingSave.cs");
const SONS: &str = include_str!("../../../../modules/sonsoftheforest/control/SonsSave.cs");
const SONS_COMPLETION: &str =
    include_str!("../../../../modules/sonsoftheforest/control/SonsSaveCompletion.cs");
const BAROTRAUMA: &str = include_str!("../../../../modules/barotrauma/control/BarotraumaSave.cs");
const CORE_MANIFEST: &str = include_str!("../../../../modules/corekeeper/control/ModManifest.json");
const SONS_MANIFEST: &str =
    include_str!("../../../../modules/sonsoftheforest/control/manifest.json");
fn source(game: Profile) -> &'static str {
    match game.module {
        "corekeeper" => CORE,
        "vrising" => VRISING,
        "sonsoftheforest" => SONS,
        _ => BAROTRAUMA,
    }
}
fn extra_sources(game: Profile) -> &'static [(&'static str, &'static str)] {
    match game.module {
        "corekeeper" => CORE_EXTRA,
        "sonsoftheforest" => &[("SonsSaveCompletion.cs", SONS_COMPLETION)],
        _ => &[],
    }
}
pub(super) fn source_hash(game: Profile) -> String {
    let extra = extra_sources(game)
        .iter()
        .map(|(name, source)| format!("{name}\0{source}\0"))
        .collect::<String>();
    io::digest(
        format!(
            "compiler-v1\0{PROTOCOL}\0{FILES}\0{}\0{CORE_MANIFEST}\0{SONS_MANIFEST}\0{extra}",
            source(game),
        )
        .as_bytes(),
    )
}
fn paths(game: Profile) -> Vec<&'static str> {
    let mut paths = vec![game.plugin];
    if game.module == "corekeeper" {
        paths
            .push("CoreKeeperServer_Data/StreamingAssets/Mods/LanGameSaveControl/ModManifest.json");
    }
    if game.module == "sonsoftheforest" {
        paths.push("Mods/LanGame.Sons.Save/manifest.json");
    }
    paths
}
fn retired_paths(game: Profile) -> &'static [&'static str] {
    if game.module == "vrising" {
        &[
            "BepInEx/plugins/LgsmVRisingSaveControl.dll",
            "BepInEx/plugins/LanGame.VRising.Save.minhook.LICENSE.txt",
        ]
    } else {
        &[]
    }
}
fn owned_paths(game: Profile) -> Vec<&'static str> {
    paths(game)
        .into_iter()
        .chain(retired_paths(game).iter().copied())
        .collect()
}
pub(super) fn inspect_ancestors(root: &Path, parent: &Path) -> Result<Vec<File>, String> {
    let mut pins = io::pin_directories(root)?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| "Save extension destination escaped the instance program directory.")?;
    let mut path = root.to_owned();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Invalid save extension destination.".into());
        }
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => pins.push(io::pin_directory(&path)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(pins)
}
pub(super) fn create_directories(root: &Path, parent: &Path) -> Result<Vec<File>, String> {
    let mut pins = io::pin_directories(root)?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| "Save extension destination escaped the program directory.")?;
    let mut path = root.to_owned();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Invalid save extension destination.".into());
        }
        path.push(component);
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
        pins.push(io::pin_directory(&path)?);
    }
    Ok(pins)
}

pub(super) fn native_files(game: Profile) -> Result<Vec<(&'static str, &'static str)>, String> {
    Ok(match game.module {
        "corekeeper" => vec![
            (
                "CoreKeeperServer_Data/Managed/Pug.Other.dll",
                "631b58cfb806e838bc0c770963b3ec13edeba368007e2ad91e280bd7bf8ad2b0",
            ),
            (
                "CoreKeeperServer_Data/Managed/PugMod.SDK.Runtime.dll",
                "ed2157a69174d6e85331d2744e3e3c9c43fa7e1e69be344aac1158c3f711ec5d",
            ),
            (
                "CoreKeeperServer_Data/Managed/UnityEngine.CoreModule.dll",
                "d184eb633e67cd97d5a0eb0faf3a8f05bdfe17e56c3cef5595cf15c685030aa6",
            ),
            (
                "CoreKeeperServer_Data/Managed/netstandard.dll",
                "6ae62e082dc494a2433984177f60ca4db5fae69b1f360a8b33754172b310b8c5",
            ),
            (
                "CoreKeeperServer_Data/Managed/Unity.Entities.dll",
                "e4f03d5dcc4547f2730218fd128e918a26d24590f31b827d334c09a0de2e762a",
            ),
            (
                "CoreKeeperServer_Data/Managed/Pug.ECS.Components.dll",
                "c2822ba48eec76430167963ddfd885eb2d5225cf5193850f1833583a27401d0f",
            ),
        ],
        "vrising" => vec![
            (
                "GameAssembly.dll",
                "ff366577fb0c707c42b8bde242304653aaa37f6ff0665ab9520150ceb692aed1",
            ),
            (
                "VRisingServer_Data/il2cpp_data/Metadata/global-metadata.dat",
                "58d3cea9b9b546ad3d734ba2a7296ac3c9820491eec7bb108a03cfbcb1065f85",
            ),
        ],
        "sonsoftheforest" => vec![
            (
                "GameAssembly.dll",
                "deadbef72344c5d19e2572e607f0c1ecbd0b85200ec9b98d5939753cde5929a2",
            ),
            (
                "SonsOfTheForestDS_Data/il2cpp_data/Metadata/global-metadata.dat",
                "d9df15ce6470003dcffe1149a2d11625fa6b3feb7db509bcd65e3b268bfe448c",
            ),
        ],
        "barotrauma" => vec![
            (
                "DedicatedServer.dll",
                "18278624996b543a2595a0ea9c0c4c14881d511ac307e1a69028d6579e67ad3c",
            ),
            (
                "BarotraumaCore.dll",
                "72acd77784b83a0e09584262ab1eea9ee40395b95f3a2e3ec3974da0bc0151b3",
            ),
        ],
        _ => return Err("This module has no verified managed save build.".into()),
    })
}

pub(super) fn verify_native(root: &Path, game: Profile) -> Result<(), String> {
    pin_native(root, game).map(|_| ())
}

pub(super) fn pin_native(root: &Path, game: Profile) -> Result<Vec<File>, String> {
    pin_supported_native(root, game)?.ok_or_else(|| format!(
        "The {} server build has not been verified for native saving. Existing files were preserved.",
        game.module
    ))
}

/// An ordinary, readable but unknown dependency tuple is optional at startup.
/// Missing, inaccessible or replaced files remain errors at every caller.
pub(super) fn pin_supported_native(
    root: &Path,
    game: Profile,
) -> Result<Option<Vec<File>>, String> {
    let mut pins = io::pin_directories(root)?;
    let mut files = native_files(game)?;
    let mut hashes = Vec::with_capacity(files.len());
    for (relative, _) in &files {
        let path = root.join(relative);
        pins.extend(io::pin_directories(
            path.parent().ok_or("Invalid native path")?,
        )?);
        let (file, bytes) = io::pin_regular(&path, 512 * 1024 * 1024)?;
        hashes.push(io::digest(&bytes));
        pins.push(file);
    }
    if game.module == "corekeeper" && !select_corekeeper_files(&hashes[0], &mut files) {
        return Ok(None);
    }
    Ok(files
        .iter()
        .zip(hashes)
        .all(|((_, expected), actual)| *expected == actual)
        .then_some(pins))
}

fn corekeeper_native_files(
    root: &Path,
    game: Profile,
) -> Result<Vec<(&'static str, &'static str)>, String> {
    let mut files = native_files(game)?;
    let primary = io::read_regular(&root.join(files[0].0), 512 * 1024 * 1024)?;
    if !select_corekeeper_files(&io::digest(&primary), &mut files) {
        return Err("This Core Keeper build has not been verified for native saving.".into());
    }
    Ok(files)
}

fn select_corekeeper_files(primary: &str, files: &mut [(&'static str, &'static str)]) -> bool {
    match primary {
        "631b58cfb806e838bc0c770963b3ec13edeba368007e2ad91e280bd7bf8ad2b0" => {}
        "042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938" => {
            // Build 25537714 uses the same native save API. Select its whole
            // dependency tuple; independently mixing admitted DLLs is invalid.
            files[0].1 = "042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938";
            files[1].1 = "6f1bd3ea3c46c7e6392c42f27bca6baf1bfb13a4eaa89756941eb51e193e7664";
            files[4].1 = "d73687d44b81ceccb1f0416cd373d75e2dc58d8d998ed26310e31f48650a6c1d";
            files[5].1 = "564660969f1c3630fbe6e133e118ced32906682bc8b53839a139211cc4fd177f";
        }
        _ => return false,
    }
    true
}

/// Remove only this extension's verified scan entries. Keep its receipt and
/// control data so a supported build can use the ordinary publication path again.
pub(super) fn deactivate_owned(root: &Path, game: Profile) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    let record = root.join(OWNER);
    let owner_pin = if let Some(previous) = io::read_optional(&record, 65536)? {
        let (pin, bytes) = io::pin_regular(&record, 65536)?;
        if previous != bytes {
            return Err("The save extension ownership changed; it was preserved.".into());
        }
        Some(pin)
    } else {
        None
    };
    // Inspect every destination before retiring the first one, including an
    // unowned installation and an interrupted owned publication.
    let Some(owner) = inspect(root, game)? else {
        return Ok(());
    };
    if owner_pin.is_none() {
        return Err("The save extension ownership changed; it was preserved.".into());
    }
    for relative in owned_paths(game) {
        let path = root.join(relative);
        if let Some(bytes) = io::read_optional(&path, 4 * 1024 * 1024)? {
            let hash = io::digest(&bytes);
            if owner.files.get(relative) != Some(&hash)
                && owner.pending_files.get(relative) != Some(&hash)
            {
                return Err(
                    "A save extension changed during deactivation; it was preserved.".into(),
                );
            }
            io::remove_owned(&path, &hash)?;
        }
    }
    Ok(())
}

pub(super) fn inspect(root: &Path, game: Profile) -> Result<Option<Ownership>, String> {
    let _root = io::pin_directories(root)?;
    let owner = io::read_optional(&root.join(OWNER), 65536)?
        .map(|b| decode(&b, game))
        .transpose()?;
    for relative in owned_paths(game) {
        let path = root.join(relative);
        let _pins = inspect_ancestors(root, path.parent().ok_or("Invalid extension path")?)?;
        let current = io::read_optional(&path, 4 * 1024 * 1024)?;
        if let Some(bytes) = current {
            let hash = io::digest(&bytes);
            if owner.as_ref().is_none_or(|o| {
                o.files.get(relative) != Some(&hash) && o.pending_files.get(relative) != Some(&hash)
            }) {
                return Err(
                    "An existing save extension file is unowned or changed; it was preserved."
                        .into(),
                );
            }
        }
    }
    let directory = root.join(DIRECTORY);
    if std::fs::symlink_metadata(&directory).is_ok() {
        let _pin = io::pin_directory(&directory)?;
        if owner.is_none() {
            return Err("An unowned save control directory exists; it was preserved.".into());
        }
    }
    Ok(owner)
}
fn decode(bytes: &[u8], game: Profile) -> Result<Ownership, String> {
    let owner: Ownership = serde_json::from_slice(bytes)
        .map_err(|_| "The save extension ownership record is invalid.")?;
    let allowed = owned_paths(game);
    if owner.protocol != 1
        || owner.module_id != game.module
        || owner
            .files
            .keys()
            .chain(owner.pending_files.keys())
            .any(|p| !allowed.contains(&p.as_str()))
    {
        return Err("The save extension ownership does not match this module.".into());
    }
    Ok(owner)
}
pub(super) fn verify_prepared(root: &Path, game: Profile) -> Result<Vec<File>, String> {
    let mut pins = io::pin_directories(root)?;
    pins.push(io::pin_directory(&root.join(DIRECTORY))?);
    let (file, bytes) = io::pin_regular(&root.join(OWNER), 65536)?;
    let owner = decode(&bytes, game)?;
    pins.push(file);
    if owner.source_sha256 != source_hash(game)
        || !owner.pending_files.is_empty()
        || owner.files.len() != paths(game).len()
    {
        return Err(
            "Prepare this save extension while the server is stopped before using it.".into(),
        );
    }
    for path in paths(game) {
        pins.extend(io::pin_directories(
            root.join(path).parent().ok_or("Invalid extension path")?,
        )?);
        let (file, bytes) = io::pin_regular(&root.join(path), 4 * 1024 * 1024)?;
        if owner.files.get(path) != Some(&io::digest(&bytes)) {
            return Err("The prepared save extension was changed; it was preserved.".into());
        }
        pins.push(file);
    }
    Ok(pins)
}

pub(super) fn install(root: &Path, game: Profile, refresh: bool) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    super::process::ensure_scope(root, game, None)?;
    verify_native(root, game)?;
    let previous = inspect(root, game)?;
    if refresh && previous.is_none() {
        return Err("Save extension refresh cannot perform its first installation.".into());
    }
    if game.module == "vrising"
        && crate::commands::commands_mod_staging::runtime_loader_requires_archive("vrising", root)?
    {
        return Err(
            "Prepare the reviewed V Rising loader while the server is stopped first.".into(),
        );
    }
    if game.module == "sonsoftheforest" {
        redloader::verify(root)?;
    }
    if previous.as_ref().is_some_and(|owner| {
        owner.source_sha256 == source_hash(game)
            && owner.pending_files.is_empty()
            && owner.files.len() == paths(game).len()
    }) && paths(game).into_iter().all(|p| root.join(p).is_file())
    {
        return verify_prepared(root, game).map(|_| ());
    }
    let plugin = compile(root, game)?;
    publish_payload(root, game, plugin)
}

pub(super) fn publish_payload(root: &Path, game: Profile, plugin: Vec<u8>) -> Result<(), String> {
    publish_payload_with(root, game, plugin, io::replace_owned)
}

fn publish_payload_with(
    root: &Path,
    game: Profile,
    plugin: Vec<u8>,
    mut publish: impl FnMut(&Path, Option<&[u8]>, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    let mut files = BTreeMap::from([(game.plugin.to_owned(), plugin)]);
    if game.module == "corekeeper" {
        files.insert(paths(game)[1].into(), CORE_MANIFEST.as_bytes().to_vec());
    }
    if game.module == "sonsoftheforest" {
        files.insert(paths(game)[1].into(), SONS_MANIFEST.as_bytes().to_vec());
    }
    // Revalidate after compilation; an external replacement during that work
    // must never become a newly adopted rollback identity.
    super::process::ensure_scope(root, game, None)?;
    let previous = inspect(root, game)?;
    let previous_owner = io::read_optional(&root.join(OWNER), 65536)?;
    let mut owner = previous.unwrap_or(Ownership {
        protocol: 1,
        module_id: game.module.into(),
        source_sha256: String::new(),
        files: BTreeMap::new(),
        pending_files: BTreeMap::new(),
    });
    // Preserve the *actual* known previous/pending bytes for crash recovery.
    let mut originals = BTreeMap::new();
    for path in owned_paths(game) {
        let original = io::read_optional(&root.join(path), 4 * 1024 * 1024)?;
        if let Some(bytes) = &original {
            let hash = io::digest(bytes);
            if owner.files.get(path) != Some(&hash) && owner.pending_files.get(path) != Some(&hash)
            {
                return Err(
                    "A save extension changed during compilation; it was preserved.".into(),
                );
            }
            owner.files.insert(path.into(), hash);
        }
        originals.insert(path.to_owned(), original);
    }
    owner.pending_files = files
        .iter()
        .map(|(p, b)| (p.clone(), io::digest(b)))
        .collect();
    let intent = serde_json::to_vec(&owner).map_err(|e| e.to_string())?;
    io::replace_owned(&root.join(OWNER), previous_owner.as_deref(), &intent)?;
    for (path, bytes) in files {
        let original = originals
            .get(&path)
            .ok_or("Missing extension publication identity")?;
        let path = root.join(path);
        let _pins = create_directories(root, path.parent().ok_or("Invalid extension path")?)?;
        publish(&path, original.as_deref(), &bytes)?;
    }
    // The new managed entry no longer loads the observer. Retire its exact
    // owned sidecars only after publication; keep the intent until cleanup ends.
    for path in retired_paths(game) {
        if let Some(Some(bytes)) = originals.get(*path) {
            io::remove_owned(&root.join(path), &io::digest(bytes))?;
        }
    }
    let _control = create_directories(root, &root.join(DIRECTORY))?;
    owner.files = std::mem::take(&mut owner.pending_files);
    owner.source_sha256 = source_hash(game);
    io::replace_owned(
        &root.join(OWNER),
        Some(&intent),
        &serde_json::to_vec(&owner).map_err(|e| e.to_string())?,
    )?;
    verify_prepared(root, game).map(|_| ())
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "langame-native-save-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&path).map_err(|_| "Cannot create the save compiler workspace.")?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        for name in [
            "SaveProtocol.cs",
            "SaveFiles.cs",
            "NativeSave.cs",
            "SonsSaveCompletion.cs",
            "CoreKeeperSaveNative.cs",
            "0Harmony.dll",
            "manifest.json",
            "save.dll",
            "compiler.txt",
            "compiler.rsp",
        ] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn compile(root: &Path, game: Profile) -> Result<Vec<u8>, String> {
    let scratch = Scratch::new()?;
    for (name, text) in [
        ("SaveProtocol.cs", PROTOCOL),
        ("SaveFiles.cs", FILES),
        ("NativeSave.cs", source(game)),
        ("manifest.json", SONS_MANIFEST),
    ] {
        std::fs::write(scratch.0.join(name), text)
            .map_err(|_| "Cannot stage the fixed save extension source.")?;
    }
    for (name, text) in extra_sources(game) {
        std::fs::write(scratch.0.join(name), text)
            .map_err(|_| "Cannot stage the native save completion source.")?;
    }
    let references = references(root, game)?;
    let mut pins = Vec::new();
    for (reference, expected) in &references {
        pins.extend(io::pin_directories(
            reference.parent().ok_or("Invalid compiler reference")?,
        )?);
        let (pin, bytes) = io::pin_regular(reference, 64 * 1024 * 1024)?;
        if io::digest(&bytes) != *expected {
            return Err(
                "The fixed compiler reference was changed; no extension was compiled.".into(),
            );
        }
        pins.push(pin);
    }
    let compiler = PathBuf::from(
        std::env::var_os("WINDIR")
            .ok_or("Native save preparation requires Windows Framework csc.")?,
    )
    .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let output = File::create(scratch.0.join("compiler.txt"))
        .map_err(|_| "Cannot create the save compiler diagnostic file.")?;
    let response = compiler_response(&compiler, &scratch.0, game, &references)?;
    let response_file = scratch.0.join("compiler.rsp");
    std::fs::write(&response_file, response)
        .map_err(|error| format!("Cannot stage the fixed save compiler response file: {error}"))?;
    let mut command = Command::new(&compiler);
    // IL2CPP SDKs have hundreds of references. Keeping them in the response
    // file avoids Windows' 32,767-character process command-line limit. This
    // option itself must stay on the command line (Framework warning CS2023).
    command
        .arg("/noconfig")
        .arg(format!("@{}", response_file.display()));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(output));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|error| {
        format!("Cannot start Windows Framework csc for the fixed save extension: {error}")
    })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The save extension compiler exceeded 30 seconds.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let diagnostics = io::read_regular(&scratch.0.join("compiler.txt"), 65536)?;
    if !diagnostics.is_empty() {
        eprintln!(
            "native-save compiler: {}",
            String::from_utf8_lossy(&diagnostics)
        );
    }
    if !status.success() {
        return Err("The fixed save extension did not compile against this loader SDK; no plugin was installed. See the application compiler diagnostics.".into());
    }
    io::read_regular(&scratch.0.join("save.dll"), 1024 * 1024)
}

fn compiler_path(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("The save compiler path is not valid Unicode.")?;
    if text
        .chars()
        .any(|character| character == '"' || character.is_control())
    {
        return Err("The save compiler path contains an invalid response-file character.".into());
    }
    Ok(format!("\"{text}\""))
}

fn compiler_response(
    compiler: &Path,
    scratch: &Path,
    game: Profile,
    references: &[(PathBuf, String)],
) -> Result<Vec<u8>, String> {
    let mut arguments = [
        "/nologo",
        "/target:library",
        "/langversion:5",
        "/warnaserror+",
    ]
    .map(str::to_owned)
    .to_vec();
    if matches!(game.module, "vrising" | "sonsoftheforest") {
        arguments.push("/nostdlib+".into());
        arguments.push("/warnaserror-:1701".into());
    } else {
        for name in ["System.dll", "System.Core.dll"] {
            arguments.push(format!(
                "/reference:{}",
                compiler_path(
                    &compiler
                        .parent()
                        .ok_or("Invalid Framework compiler path")?
                        .join(name)
                )?
            ));
        }
    }
    arguments.push(format!(
        "/out:{}",
        compiler_path(&scratch.join("save.dll"))?
    ));
    for (reference, _) in references {
        arguments.push(format!("/reference:{}", compiler_path(reference)?));
    }
    if game.module == "sonsoftheforest" {
        arguments.push(format!(
            "/resource:{},LanGame.Sons.manifest.json",
            compiler_path(&scratch.join("manifest.json"))?
        ));
    }
    for name in ["SaveProtocol.cs", "SaveFiles.cs", "NativeSave.cs"] {
        arguments.push(compiler_path(&scratch.join(name))?);
    }
    for (name, _) in extra_sources(game) {
        arguments.push(compiler_path(&scratch.join(name))?);
    }
    Ok(format!("\u{feff}{}\r\n", arguments.join("\r\n")).into_bytes())
}

fn references(root: &Path, game: Profile) -> Result<Vec<(PathBuf, String)>, String> {
    if game.module == "barotrauma" {
        return Ok(Vec::new());
    }
    if game.module == "corekeeper" {
        let files = corekeeper_native_files(root, game)?;
        return Ok([
            ("PugMod.SDK.Runtime.dll", files[1].1),
            (
                "UnityEngine.CoreModule.dll",
                "d184eb633e67cd97d5a0eb0faf3a8f05bdfe17e56c3cef5595cf15c685030aa6",
            ),
            (
                "netstandard.dll",
                "6ae62e082dc494a2433984177f60ca4db5fae69b1f360a8b33754172b310b8c5",
            ),
        ]
        .into_iter()
        .map(|(name, hash)| {
            (
                root.join("CoreKeeperServer_Data/Managed").join(name),
                hash.into(),
            )
        })
        .collect());
    }
    let (runtime, sdk) = if game.module == "vrising" {
        ("dotnet", "BepInEx/core")
    } else {
        ("_Redloader/dotnet", "_Redloader/net6")
    };
    let mut references = Vec::new();
    let files: BTreeMap<String, String> = if game.module == "vrising" {
        #[derive(serde::Deserialize)]
        struct Loader {
            files: BTreeMap<String, String>,
        }
        serde_json::from_str::<Loader>(include_str!("../assets/mod-loaders/vrising.json"))
            .map_err(|_| "Invalid reviewed V Rising loader manifest.")?
            .files
    } else {
        redloader::files()?
    };
    for (path, hash) in &files {
        let relative = Path::new(path);
        let Some(name) = relative.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if relative.parent() == Some(Path::new(runtime))
            && name.ends_with(".dll")
            && !name.ends_with(".Native.dll")
            && (name.starts_with("System.")
                || ["mscorlib.dll", "netstandard.dll", "Microsoft.CSharp.dll"].contains(&name))
        {
            references.push((root.join(path), hash.clone()));
        }
    }
    let libraries: &[&str] = if game.module == "vrising" {
        &[
            "BepInEx.Core.dll",
            "BepInEx.Unity.IL2CPP.dll",
            "0Harmony.dll",
            "Il2CppInterop.Runtime.dll",
            "Il2CppInterop.Common.dll",
        ]
    } else {
        &["RedLoader.dll", "SonsSdk.dll", "0Harmony.dll"]
    };
    for name in libraries {
        let relative = format!("{sdk}/{name}");
        references.push((
            root.join(&relative),
            files
                .get(&relative)
                .ok_or("The fixed loader SDK is incomplete.")?
                .clone(),
        ));
    }
    references.sort();
    Ok(references)
}

#[cfg(all(test, windows))]
#[path = "commands_managed_save_installation_tests.rs"]
mod installation_tests;
