//! Opt-in export for real Windows Sandbox installs. The host test only writes
//! production scripts; the separate runner guards every installer to the guest.
use super::*;
use std::io::Write;

const GUEST_ROOT: &str = "C:/LanGame Sandbox O'Brien/运行库/runtimes/windows-packages";

fn sandbox_case(
    package: catalog::RuntimePackage,
    requirements: Vec<WindowsRuntimeRequirement>,
) -> serde_json::Value {
    let root = Path::new(GUEST_ROOT);
    let path = package_path(root, package);
    let probes = if requirements.is_empty() {
        vec![serde_json::json!({
            "architecture": "x86",
            "dlls": [],
            "script": process::probe_script(None),
        })]
    } else {
        requirements
            .iter()
            .map(|requirement| {
                serde_json::json!({
                    "architecture": match requirement.architecture {
                        Architecture::X86 => "x86",
                        Architecture::X64 => "x64",
                    },
                    "dlls": requirement.imported_dlls,
                    "script": process::probe_script(Some(requirement)),
                })
            })
            .collect()
    };
    serde_json::json!({
        "id": package.id,
        "name": package.name,
        "url": package.url,
        "expected_sha256": package.sha256,
        "package_path": path,
        "installer_script": process::installer_script(package, &path, root),
        "probes": probes,
    })
}

fn sandbox_requirement(
    family: Family,
    architecture: Architecture,
    dlls: &[&str],
) -> WindowsRuntimeRequirement {
    WindowsRuntimeRequirement {
        family,
        architecture,
        imported_dlls: dlls.iter().map(|dll| (*dll).to_owned()).collect(),
        search_directories: vec![PathBuf::from(GUEST_ROOT).join("empty-consumer-directory")],
    }
}

#[test]
#[ignore = "exports production scripts only; launch the explicit Windows Sandbox runner separately"]
fn export_windows_runtime_sandbox_manifest() {
    let directory = PathBuf::from(
        std::env::var_os("LGSM_RUNTIME_SANDBOX_EXPORT_DIR")
            .expect("LGSM_RUNTIME_SANDBOX_EXPORT_DIR must select disposable task output"),
    );
    assert!(directory.is_absolute(), "export directory must be absolute");
    validate_cache_path(&directory).expect("export path must not traverse reparse points");
    fs::create_dir_all(&directory).expect("create export directory");
    let mut cases = Vec::new();
    // v14 x64 comes first so an optional Rust API worker can run in this guest.
    for (family, architecture, dlls) in [
        (
            Family::VisualCpp2015,
            Architecture::X64,
            &["vcruntime140.dll", "msvcp140.dll"][..],
        ),
        (
            Family::VisualCpp2015,
            Architecture::X86,
            &["vcruntime140.dll", "msvcp140.dll"][..],
        ),
        (
            Family::VisualCpp2010,
            Architecture::X86,
            &["msvcr100.dll", "msvcp100.dll"][..],
        ),
        (
            Family::VisualCpp2010,
            Architecture::X64,
            &["msvcr100.dll", "msvcp100.dll"][..],
        ),
        (
            Family::VisualCpp2012,
            Architecture::X86,
            &["msvcr110.dll", "msvcp110.dll"][..],
        ),
        (
            Family::VisualCpp2012,
            Architecture::X64,
            &["msvcr110.dll", "msvcp110.dll"][..],
        ),
        (
            Family::VisualCpp2013,
            Architecture::X86,
            &["msvcr120.dll", "msvcp120.dll"][..],
        ),
        (
            Family::VisualCpp2013,
            Architecture::X64,
            &["msvcr120.dll", "msvcp120.dll"][..],
        ),
    ] {
        cases.push(sandbox_case(
            catalog::package(family, architecture),
            vec![sandbox_requirement(family, architecture, dlls)],
        ));
    }
    let directx_dlls = [
        "d3dx9_43.dll",
        "d3dcompiler_43.dll",
        "xinput1_3.dll",
        "xaudio2_7.dll",
        "xactengine3_7.dll",
        "x3daudio1_7.dll",
    ];
    cases.push(sandbox_case(
        catalog::package(Family::DirectXJune2010, Architecture::X64),
        [Architecture::X86, Architecture::X64]
            .into_iter()
            .map(|architecture| {
                sandbox_requirement(Family::DirectXJune2010, architecture, &directx_dlls)
            })
            .collect(),
    ));
    cases.push(sandbox_case(catalog::XNA, Vec::new()));
    let manifest = serde_json::json!({
        "schema_version": 1,
        "scope": "Production installer_script and independent probe_script processes; host Rust orchestration is not exercised by this manifest.",
        "guest_package_root": GUEST_ROOT,
        "signature_function": process::SIGNATURE_FUNCTION,
        "cases": cases,
    });
    let path = directory.join("manifest.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create a fresh sandbox manifest without overwriting prior evidence");
    file.write_all(&serde_json::to_vec_pretty(&manifest).expect("serialize manifest"))
        .expect("write manifest");
    file.sync_all().expect("flush manifest");
    eprintln!("Windows runtime sandbox manifest: {}", path.display());
}
