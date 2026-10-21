use super::*;
use std::fs;
use std::io::Cursor;

const PE: usize = 0x80;
const OPTIONAL: usize = PE + 24;
const RAW: usize = 0x200;
const RVA: u32 = 0x1000;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(
    architecture: WindowsRuntimeArchitecture,
    imports: &[&str],
    delayed: &[&str],
) -> Vec<u8> {
    let mut bytes = vec![0; 0x2200];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, PE as u32);
    bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
    let (machine, magic, optional_size, directory_start) = match architecture {
        WindowsRuntimeArchitecture::X86 => (0x14c, 0x10b, 0xe0, 96),
        WindowsRuntimeArchitecture::X64 => (0x8664, 0x20b, 0xf0, 112),
    };
    put16(&mut bytes, PE + 4, machine);
    put16(&mut bytes, PE + 6, 1);
    put16(&mut bytes, PE + 20, optional_size);
    put16(&mut bytes, OPTIONAL, magic);
    if architecture == WindowsRuntimeArchitecture::X86 {
        put32(&mut bytes, OPTIONAL + 28, 0x400000);
    } else {
        bytes[OPTIONAL + 24..OPTIONAL + 32].copy_from_slice(&0x140000000u64.to_le_bytes());
    }
    put32(&mut bytes, OPTIONAL + 60, RAW as u32);
    put32(&mut bytes, OPTIONAL + directory_start - 4, 16);
    let section = OPTIONAL + optional_size as usize;
    put32(&mut bytes, section + 8, 0x2000);
    put32(&mut bytes, section + 12, RVA);
    put32(&mut bytes, section + 16, 0x2000);
    put32(&mut bytes, section + 20, RAW as u32);
    let mut next_name = RAW + 0x800;
    for (names, index, relative, width) in [(imports, 1, 0, 20), (delayed, 13, 0x400, 32)] {
        if names.is_empty() {
            continue;
        }
        let directory = OPTIONAL + directory_start + index * 8;
        put32(&mut bytes, directory, RVA + relative as u32);
        put32(
            &mut bytes,
            directory + 4,
            ((names.len() + 1) * width) as u32,
        );
        for (index, name) in names.iter().enumerate() {
            let descriptor = RAW + relative + index * width;
            if width == 32 {
                put32(&mut bytes, descriptor, 1);
            } else {
                put32(&mut bytes, descriptor, RVA + 0x1800);
            }
            put32(
                &mut bytes,
                descriptor + if width == 32 { 4 } else { 12 },
                RVA + (next_name - RAW) as u32,
            );
            bytes[next_name..next_name + name.len()].copy_from_slice(name.as_bytes());
            next_name += name.len() + 1;
        }
    }
    bytes
}

fn inspect(bytes: Vec<u8>) -> Result<Option<Imports>, String> {
    let length = bytes.len() as u64;
    inspect_reader(
        Reader {
            file: Cursor::new(bytes),
            length,
        },
        &mut 0,
    )
}

fn requirement(
    family: WindowsRuntimeFamily,
    architecture: WindowsRuntimeArchitecture,
    dlls: &[&str],
    root: &Path,
) -> WindowsRuntimeRequirement {
    WindowsRuntimeRequirement {
        family,
        architecture,
        imported_dlls: dlls.iter().map(|name| String::from(*name)).collect(),
        search_directories: vec![root.canonicalize().unwrap()],
    }
}

struct Files {
    root: PathBuf,
}

impl Files {
    fn new() -> Self {
        let root = crate::test_support::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn normal_and_delayed_imports_preserve_x86_and_x64_architecture() {
    for architecture in [
        WindowsRuntimeArchitecture::X86,
        WindowsRuntimeArchitecture::X64,
    ] {
        let parsed = inspect(fixture(
            architecture,
            &["MSVCR100.dll"],
            &["VCRUNTIME140_1.dll"],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(parsed.architecture, architecture);
        assert_eq!(
            parsed.names,
            BTreeSet::from([
                String::from("MSVCR100.dll"),
                String::from("VCRUNTIME140_1.dll")
            ])
        );
    }
}

#[test]
fn legacy_x86_delayed_import_uses_image_base() {
    let mut bytes = fixture(WindowsRuntimeArchitecture::X86, &[], &["msvcr120.dll"]);
    put32(&mut bytes, RAW + 0x400, 0);
    put32(&mut bytes, RAW + 0x404, 0x400000 + RVA + 0x800);
    assert_eq!(
        inspect(bytes).unwrap().unwrap().names,
        BTreeSet::from([String::from("msvcr120.dll")])
    );
}

#[test]
fn runtime_families_cover_supported_versions_without_windows_system_dlls() {
    for (name, expected) in [
        ("msvcr100.dll", WindowsRuntimeFamily::VisualCpp2010),
        ("msvcp100.dll", WindowsRuntimeFamily::VisualCpp2010),
        ("msvcr110.dll", WindowsRuntimeFamily::VisualCpp2012),
        ("msvcp110.dll", WindowsRuntimeFamily::VisualCpp2012),
        ("msvcr120.dll", WindowsRuntimeFamily::VisualCpp2013),
        ("msvcp120.dll", WindowsRuntimeFamily::VisualCpp2013),
        ("vcruntime140_1.dll", WindowsRuntimeFamily::VisualCpp2015),
        (
            "msvcp140_atomic_wait.dll",
            WindowsRuntimeFamily::VisualCpp2015,
        ),
        ("concrt140.dll", WindowsRuntimeFamily::VisualCpp2015),
        ("d3dx9_24.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("d3dx9_43.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("d3dx10_43.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("d3dx11_43.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("d3dcompiler_43.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("xinput1_3.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("xaudio2_7.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("x3daudio1_7.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("xapofx1_5.dll", WindowsRuntimeFamily::DirectXJune2010),
        ("xactengine3_7.dll", WindowsRuntimeFamily::DirectXJune2010),
    ] {
        assert_eq!(runtime_family(name), Some(expected), "{name}");
    }
    for name in [
        "ucrtbase.dll",
        "msvcrt.dll",
        "kernel32.dll",
        "d3dcompiler_47.dll",
        "xinput9_1_0.dll",
        "xaudio2_8.dll",
        "d3dx9_44.dll",
        "d3dx9_foo.dll",
    ] {
        assert_eq!(runtime_family(name), None, "{name}");
    }
}

#[test]
fn inspection_follows_only_reachable_local_dlls_and_deduplicates_cycles() {
    let files = Files::new();
    let architecture = WindowsRuntimeArchitecture::X64;
    let executable = files.write(
        "server.exe",
        &fixture(
            architecture,
            &["engine.dll", "MSVCP140.dll"],
            &["msvcp140.dll"],
        ),
    );
    files.write(
        "engine.dll",
        &fixture(
            architecture,
            &["engine.dll", "audio.dll", "msvcr120.dll"],
            &[],
        ),
    );
    files.write(
        "audio.dll",
        &fixture(architecture, &["engine.dll", "x3daudio1_7.dll"], &[]),
    );
    files.write(
        "unrelated.dll",
        &fixture(architecture, &["msvcr100.dll"], &[]),
    );
    let nested = files.root.join("plugins");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("unrelated.dll"), b"MZ broken PE").unwrap();
    assert_eq!(
        inspect_windows_runtime_requirements(&executable).unwrap(),
        vec![
            requirement(
                WindowsRuntimeFamily::VisualCpp2013,
                architecture,
                &["msvcr120.dll"],
                &files.root
            ),
            requirement(
                WindowsRuntimeFamily::VisualCpp2015,
                architecture,
                &["msvcp140.dll"],
                &files.root
            ),
            requirement(
                WindowsRuntimeFamily::DirectXJune2010,
                architecture,
                &["x3daudio1_7.dll"],
                &files.root
            ),
        ]
    );
}

#[test]
fn missing_executables_scripts_and_non_pe_dependencies_do_not_execute_or_fail() {
    let files = Files::new();
    assert!(
        inspect_windows_runtime_requirements(&files.root.join("absent.exe"))
            .unwrap()
            .is_empty()
    );
    let script = files.write("server.bat", b"@echo off\r\nexit /b 99\r\n");
    assert!(
        inspect_windows_runtime_requirements(&script)
            .unwrap()
            .is_empty()
    );
    let executable = files.write(
        "server.exe",
        &fixture(
            WindowsRuntimeArchitecture::X86,
            &["non-pe.dll", "msvcr100.dll"],
            &[],
        ),
    );
    files.write("non-pe.dll", b"external boundary fixture");
    assert_eq!(
        inspect_windows_runtime_requirements(&executable).unwrap(),
        vec![requirement(
            WindowsRuntimeFamily::VisualCpp2010,
            WindowsRuntimeArchitecture::X86,
            &["msvcr100.dll"],
            &files.root,
        )]
    );
}

#[test]
fn corrupted_pe_reports_the_file_and_failure_location() {
    let files = Files::new();
    let executable = files.write(
        "server.exe",
        &fixture(WindowsRuntimeArchitecture::X64, &["engine.dll"], &[]),
    );
    files.write("engine.dll", b"MZ truncated");
    let error = inspect_windows_runtime_requirements(&executable).unwrap_err();
    assert!(error.contains("engine.dll"), "{error}");
    assert!(error.contains("0x3c"), "{error}");
}

#[test]
fn rva_mapping_accepts_header_data_and_rejects_ambiguous_or_unbacked_sections() {
    let mut header = fixture(WindowsRuntimeArchitecture::X86, &["msvcr110.dll"], &[]);
    let descriptor = header[RAW..RAW + 40].to_vec();
    header[0x1c0..0x1e8].copy_from_slice(&descriptor);
    put32(&mut header, OPTIONAL + 96 + 8, 0x1c0);
    assert!(
        inspect(header)
            .unwrap()
            .unwrap()
            .names
            .contains("msvcr110.dll")
    );
    let mut unbacked = fixture(WindowsRuntimeArchitecture::X86, &["msvcr110.dll"], &[]);
    put32(&mut unbacked, OPTIONAL + 0xe0 + 16, 0x200);
    assert!(inspect(unbacked).err().unwrap().contains("unbacked"));
    let mut ambiguous = fixture(WindowsRuntimeArchitecture::X86, &["msvcr110.dll"], &[]);
    put32(&mut ambiguous, OPTIONAL + 0xe0 + 12, 0x100);
    put32(&mut ambiguous, OPTIONAL + 96 + 8, 0x1c0);
    assert!(inspect(ambiguous).err().unwrap().contains("ambiguous"));
}

#[test]
fn zero_filled_virtual_import_descriptors_follow_pe_loader_semantics() {
    for architecture in [
        WindowsRuntimeArchitecture::X86,
        WindowsRuntimeArchitecture::X64,
    ] {
        let mut bytes = fixture(architecture, &["vcruntime140.dll"], &[]);
        let (optional_size, directories) = match architecture {
            WindowsRuntimeArchitecture::X86 => (0xe0, 96),
            WindowsRuntimeArchitecture::X64 => (0xf0, 112),
        };
        // A protected image may retain a delay directory in virtual-only data.
        // The Windows loader initially supplies zeros there, including its null
        // descriptor. Normal, file-backed imports must still be inspected.
        put32(&mut bytes, OPTIONAL + optional_size + 8, 0x3000);
        put32(&mut bytes, OPTIONAL + directories + 13 * 8, RVA + 0x2100);
        put32(&mut bytes, OPTIONAL + directories + 13 * 8 + 4, 480);
        assert_eq!(
            inspect(bytes).unwrap().unwrap().names,
            BTreeSet::from([String::from("vcruntime140.dll")])
        );
    }
}

#[test]
fn virtual_directory_terminator_can_straddle_the_raw_boundary_but_not_the_image() {
    let mut bytes = fixture(WindowsRuntimeArchitecture::X64, &["vcruntime140.dll"], &[]);
    let descriptor = bytes[RAW..RAW + 20].to_vec();
    bytes[RAW + 0x1fe0..RAW + 0x1ff4].copy_from_slice(&descriptor);
    put32(&mut bytes, OPTIONAL + 0xf0 + 8, 0x3000);
    put32(&mut bytes, OPTIONAL + 112 + 8, RVA + 0x1fe0);
    put32(&mut bytes, OPTIONAL + 112 + 12, 40);
    assert!(
        inspect(bytes.clone())
            .unwrap()
            .unwrap()
            .names
            .contains("vcruntime140.dll")
    );
    put32(&mut bytes, OPTIONAL + 112 + 8, RVA + 0x2ff0);
    assert!(
        inspect(bytes).is_err(),
        "unmapped data is never synthesized"
    );
}

#[test]
fn malformed_headers_directories_and_names_are_rejected_with_bounds() {
    let original = fixture(WindowsRuntimeArchitecture::X86, &["msvcr110.dll"], &[]);
    let mut cases = Vec::new();
    let mut outside_header = original.clone();
    put32(&mut outside_header, 60, MAX_HEADER_BYTES as u32);
    cases.push(outside_header);
    let mut section_count = original.clone();
    put16(&mut section_count, PE + 6, 97);
    cases.push(section_count);
    let mut directory_count = original.clone();
    put32(&mut directory_count, OPTIONAL + 92, 17);
    cases.push(directory_count);
    let mut unmapped = original.clone();
    put32(&mut unmapped, OPTIONAL + 96 + 8, 0x9000);
    cases.push(unmapped);
    let mut no_terminator = original.clone();
    put32(&mut no_terminator, OPTIONAL + 96 + 12, 20);
    cases.push(no_terminator);
    let mut file_range = original.clone();
    put32(&mut file_range, OPTIONAL + 0xe0 + 20, u32::MAX);
    cases.push(file_range);
    let mut rva_overflow = original.clone();
    put32(&mut rva_overflow, OPTIONAL + 0xe0 + 12, u32::MAX);
    cases.push(rva_overflow);
    let mut bad_name = fixture(WindowsRuntimeArchitecture::X86, &["../msvcr110.dll"], &[]);
    cases.push(bad_name.clone());
    bad_name[RAW + 0x800..RAW + 0x800 + MAX_DLL_NAME_BYTES].fill(b'a');
    cases.push(bad_name);
    for (index, bytes) in cases.into_iter().enumerate() {
        assert!(inspect(bytes).is_err(), "malformed fixture {index}");
    }
}

#[test]
fn unsupported_delay_attributes_and_underflowing_legacy_va_fail() {
    for (attributes, name) in [(8, RVA + 0x800), (0, 1)] {
        let mut bytes = fixture(WindowsRuntimeArchitecture::X86, &[], &["msvcr120.dll"]);
        put32(&mut bytes, RAW + 0x400, attributes);
        put32(&mut bytes, RAW + 0x404, name);
        assert!(inspect(bytes).is_err());
    }
}

#[test]
fn import_descriptor_and_reachable_file_budgets_fail_closed() {
    let mut bytes = fixture(WindowsRuntimeArchitecture::X86, &["msvcr110.dll"], &[]);
    let raw_size = ((MAX_IMPORTS + 1) * 20 + 0x1000) as u32;
    bytes.resize(RAW + raw_size as usize, 0);
    put32(&mut bytes, OPTIONAL + 0xe0 + 8, raw_size);
    put32(&mut bytes, OPTIONAL + 0xe0 + 16, raw_size);
    put32(
        &mut bytes,
        OPTIONAL + 96 + 12,
        ((MAX_IMPORTS + 1) * 20) as u32,
    );
    let name_offset = RAW + MAX_IMPORTS * 20 + 0x100;
    bytes[name_offset..name_offset + 12].copy_from_slice(b"msvcr110.dll");
    for index in 0..MAX_IMPORTS {
        put32(&mut bytes, RAW + index * 20, 1);
        put32(
            &mut bytes,
            RAW + index * 20 + 12,
            RVA + (name_offset - RAW) as u32,
        );
    }
    assert!(inspect(bytes).err().unwrap().contains("4096-entry"));
    let files = Files::new();
    for index in 0..=MAX_FILES {
        files.write(
            &format!("dependency{index}.dll"),
            &fixture(
                WindowsRuntimeArchitecture::X64,
                &[&format!("dependency{}.dll", index + 1)],
                &[],
            ),
        );
    }
    let error =
        inspect_windows_runtime_requirements(&files.root.join("dependency0.dll")).unwrap_err();
    assert!(error.contains("128-file"), "{error}");
}
