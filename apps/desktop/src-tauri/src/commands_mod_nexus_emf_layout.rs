//! EMF's packaged game/ tree is data to deploy; its installer is never executed.
//! https://github.com/matenm/emf-enshrouded/tree/v0.6.6/package
use super::*;
use crate::commands::commands_mod_capacity::{ModSpaceBudget, require_mod_capacity};
use layout::Route;
use std::io::{Seek, SeekFrom};

const AUXILIARY_ARCHIVE: &str = "emf-package-auxiliary.zip.lgsm";

struct AuxiliaryZipWriter {
    file: fs::File,
    budget: ModSpaceBudget,
}

impl Write for AuxiliaryZipWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.budget
            .before_write(bytes.len())
            .map_err(std::io::Error::other)?;
        self.file.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for AuxiliaryZipWriter {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

pub(super) fn is_payload_descriptor(path: &str) -> bool {
    path == "emf/known_tools.txt" || path.starts_with("emf/vk/") || mod_manifest(path).is_some()
}

fn mod_manifest(path: &str) -> Option<&str> {
    let (name, inner) = path.strip_prefix("emf/mods/")?.split_once('/')?;
    (layout::safe_mod_name(name) && inner == "mod.json").then_some(name)
}

pub(super) fn allowed_runtime(path: &str) -> bool {
    if matches!(
        path,
        "emf/emf_core.dll"
            | "emf/emf_content.dll"
            | "emf/emf_frametime.dll"
            | "emf/known_tools.txt"
            | "emf/vk/VkLayer_EMF_console.json"
            | "emf/vk/VkLayer_EMF_frametime.json"
    ) {
        return true;
    }
    if let Some(font) = path.strip_prefix("emf/fonts/") {
        return !font.contains('/')
            && management::validate_component(font).is_ok()
            && Path::new(font)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ttf"));
    }
    let Some((name, inner)) = path
        .strip_prefix("emf/mods/")
        .and_then(|path| path.split_once('/'))
    else {
        return false;
    };
    layout::safe_mod_name(name)
        && !inner.is_empty()
        && !inner.split('/').any(|part| part.starts_with('.'))
        && !matches!(
            Path::new(inner)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "exe" | "bat" | "cmd" | "ps1" | "vbs" | "msi" | "com" | "py"
        )
}

fn auxiliary_game_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "emf/emf.exe"
        || ["emf/setup/", "emf/tools/", "emf/ui/"]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
}

pub(super) fn plan(payload: &BTreeMap<String, String>) -> Result<Option<Vec<Route>>, String> {
    let mut roots = BTreeMap::new();
    for source in payload.keys() {
        let parts = source.split('/').collect::<Vec<_>>();
        let game_index = if parts
            .first()
            .is_some_and(|name| name.eq_ignore_ascii_case("game"))
        {
            0
        } else if parts
            .get(1)
            .is_some_and(|name| name.eq_ignore_ascii_case("game"))
        {
            1
        } else {
            continue;
        };
        let relative = parts[game_index + 1..].join("/");
        if relative.eq_ignore_ascii_case("dinput8.dll")
            || relative.to_ascii_lowercase().starts_with("emf/")
        {
            let prefix = format!("{}/", parts[..=game_index].join("/"));
            roots.insert(prefix.to_ascii_lowercase(), prefix);
        }
    }
    if roots.is_empty() {
        return Ok(None);
    }
    if roots.len() != 1 {
        return Err("EMF package contains multiple game/ trees; no files were installed".into());
    }
    let prefix = roots
        .into_values()
        .next()
        .expect("one EMF tree was checked");
    for required in ["dinput8.dll", "emf/emf_core.dll"] {
        if !payload
            .keys()
            .any(|source| source.eq_ignore_ascii_case(&format!("{prefix}{required}")))
        {
            return Err(
                "EMF installation requires game/dinput8.dll and game/emf/emf_core.dll together"
                    .into(),
            );
        }
    }
    let mut routes = Vec::new();
    let mut registry = ManualModStagingRegistry::default();
    for source in payload.keys() {
        let normalized = normalize_manual_mod_relative_path(Path::new(source))?;
        if normalized.to_string_lossy().replace('\\', "/") != *source {
            return Err("Invalid EMF package path".into());
        }
        let relative = source
            .get(..prefix.len())
            .filter(|start| start.eq_ignore_ascii_case(&prefix))
            .map(|_| &source[prefix.len()..]);
        let destination = match relative {
            Some(path) if path.eq_ignore_ascii_case("dinput8.dll") => Some("dinput8.dll".into()),
            Some(path) if allowed_runtime(path) => Some(path.to_owned()),
            Some(path) if auxiliary_game_file(path) => None,
            Some(path) => {
                return Err(format!(
                    "EMF package has an unsupported game/ file: {}; existing files were preserved",
                    path.chars().take(160).collect::<String>()
                ));
            }
            None => None,
        };
        if let Some(path) = &destination {
            registry.register_file(Path::new(path))?;
        }
        routes.push(Route {
            source: source.clone(),
            metadata_path: destination.is_none().then(|| source.clone()),
            destination,
        });
    }
    Ok(Some(routes))
}

pub(super) fn prepare(
    runtime: &Path,
    previous: Option<&InstalledPackage>,
    unpacked: &Path,
    payload: &mut BTreeMap<String, String>,
    routes: &mut Vec<Route>,
) -> Result<(), String> {
    if !routes
        .iter()
        .any(|route| route.destination.as_deref() == Some("emf/emf_core.dll"))
    {
        return Ok(());
    }
    validate_existing(runtime, previous)?;
    validate_manifests(unpacked, routes)?;
    let auxiliary = routes
        .iter()
        .filter(|route| route.destination.is_none())
        .map(|route| {
            fs::metadata(unpacked.join(&route.source))
                .map(|metadata| (route, metadata.len()))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if auxiliary.is_empty() {
        return Ok(());
    }
    // EML recognises .zip as a Mod. Keep an opaque suffix and no loose DLL,
    // script or mod.json in the inventory anchor, even for recursive loaders.
    let source = format!(
        ".langame-generated-emf-auxiliary-{}",
        uuid::Uuid::new_v4().simple()
    );
    let path = unpacked.join(&source);
    let expected_bytes = auxiliary.iter().try_fold(0u64, |total, (_, bytes)| {
        total
            .checked_add(*bytes)
            .ok_or("EMF auxiliary archive size exceeds capacity accounting")
    })?;
    require_mod_capacity(&path, expected_bytes)?;
    let budget = ModSpaceBudget::new(&path)?;
    let file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    // The capacity wrapper covers payload, ZIP headers, rewrites and the final
    // directory; no ZipWriter output bypasses the shared disk-space budget.
    let mut archive = zip::ZipWriter::new(AuxiliaryZipWriter { file, budget });
    for (route, bytes) in auxiliary {
        archive
            .start_file(
                &route.source,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .large_file(bytes >= u64::from(u32::MAX)),
            )
            .map_err(|error| error.to_string())?;
        let mut input =
            fs::File::open(unpacked.join(&route.source)).map_err(|error| error.to_string())?;
        std::io::copy(&mut input, &mut archive).map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    let hash = fingerprint_tree(&path)?
        .remove("")
        .ok_or("Missing EMF auxiliary fingerprint")?;
    payload.insert(source.clone(), hash);
    routes.retain(|route| route.destination.is_some());
    routes.push(Route {
        source,
        destination: None,
        metadata_path: Some(AUXILIARY_ARCHIVE.into()),
    });
    Ok(())
}

fn validate_existing(runtime: &Path, previous: Option<&InstalledPackage>) -> Result<(), String> {
    let chain = runtime.join("dinput8-chain.ini");
    validate_manual_mod_existing_path_components(&chain, false)?;
    if chain.exists() {
        return Err(
            "EMF chain-loader migration is not supported; existing dinput8-chain.ini was preserved"
                .into(),
        );
    }
    let base = runtime.join("emf");
    validate_manual_mod_existing_path_components(&base, false)?;
    for relative in fingerprint_tree(&base)?.keys() {
        let lower = relative.to_ascii_lowercase();
        if matches!(lower.as_str(), "installed.txt" | "moved.txt") {
            return Err("Existing manual EMF installation records cannot be adopted; existing files were preserved".into());
        }
        let owned = previous.is_some_and(|package| {
            package
                .routed_files
                .keys()
                .any(|path| path.eq_ignore_ascii_case(&format!("emf/{relative}")))
        });
        let retained = lower == "emf_core.ini"
            || [
                "config/",
                "profiles/",
                "mods/",
                "cache/",
                "logs/",
                "content/",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
            || matches!(
                lower.as_str(),
                "emf_core.log"
                    | "emf_loader.log"
                    | "content.log"
                    | "chain_status.txt"
                    | "commands.txt"
            );
        if !owned && !retained {
            return Err(format!(
                "Unknown existing EMF file {} cannot be adopted; existing files were preserved",
                relative.chars().take(160).collect::<String>()
            ));
        }
    }
    Ok(())
}

fn validate_manifests(unpacked: &Path, routes: &[Route]) -> Result<(), String> {
    let destinations = routes
        .iter()
        .filter_map(|route| route.destination.as_deref())
        .collect::<HashSet<_>>();
    let mut ids = HashSet::new();
    let mut needs_lua = false;
    for route in routes {
        let Some(path) = route.destination.as_deref() else {
            continue;
        };
        if path.starts_with("emf/mods/") {
            let (name, _) = path["emf/mods/".len()..]
                .split_once('/')
                .ok_or("Invalid EMF Mod path")?;
            if !destinations.contains(format!("emf/mods/{name}/mod.json").as_str()) {
                return Err(format!(
                    "EMF Mod {name} has no mod.json; no files were installed"
                ));
            }
        }
        let Some(name) = mod_manifest(path) else {
            continue;
        };
        let mut bytes = Vec::new();
        fs::File::open(unpacked.join(&route.source))
            .and_then(|file| file.take(65537).read_to_end(&mut bytes))
            .map_err(|error| error.to_string())?;
        if bytes.len() > 65536 {
            return Err("EMF mod.json exceeds its size limit".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "EMF mod.json is not valid JSON")?;
        let id = value
            .get("id")
            .and_then(|id| id.as_str())
            .filter(|id| layout::safe_mod_name(id))
            .ok_or("EMF mod.json needs a valid id")?;
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err("EMF package contains duplicate Mod ids".into());
        }
        let dll = value.get("dll").filter(|value| !value.is_null());
        let main = value.get("main").filter(|value| !value.is_null());
        if dll.is_some() && main.is_some() {
            return Err("EMF mod.json cannot declare both dll and main".into());
        }
        let default = format!("{id}.dll");
        let entry = main
            .or(dll)
            .map_or(Some(default.as_str()), |entry| entry.as_str())
            .ok_or("Invalid EMF Mod entry")?;
        let normalized = normalize_manual_mod_relative_path(Path::new(entry))?;
        if normalized.to_string_lossy().replace('\\', "/") != entry
            || !destinations.contains(format!("emf/mods/{name}/{entry}").as_str())
        {
            return Err(format!(
                "EMF Mod {name} is missing its declared entry; no files were installed"
            ));
        }
        if main.is_some() {
            if !entry.to_ascii_lowercase().ends_with(".lua") {
                return Err("This EMF package declares an unsupported script entry".into());
            }
            needs_lua = true;
        }
    }
    if needs_lua && !(ids.contains("lua") && destinations.contains("emf/mods/lua/lua.dll")) {
        return Err("EMF Lua Mods require the bundled lua host; no files were installed".into());
    }
    Ok(())
}
