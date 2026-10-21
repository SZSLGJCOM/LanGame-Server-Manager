use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 128;
const MAX_HEADER_BYTES: u64 = 1024 * 1024;
const MAX_IMPORTS: usize = 4096;
const MAX_DLL_NAME_BYTES: usize = 261;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowsRuntimeFamily {
    VisualCpp2010,
    VisualCpp2012,
    VisualCpp2013,
    VisualCpp2015,
    DirectXJune2010,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowsRuntimeArchitecture {
    X86,
    X64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowsRuntimeRequirement {
    pub family: WindowsRuntimeFamily,
    pub architecture: WindowsRuntimeArchitecture,
    pub imported_dlls: Vec<String>,
    pub search_directories: Vec<PathBuf>,
}

/// Inspect imports without loading a binary or executing an upstream launcher.
/// Only imported DLLs beside this executable are followed; unrelated files and
/// subdirectories are never enumerated. Missing dependencies remain requirements
/// for the caller to verify, rather than making the inspection itself fail.
pub fn inspect_windows_runtime_requirements(
    executable: &Path,
) -> Result<Vec<WindowsRuntimeRequirement>, String> {
    let Some(executable) = canonical_file(executable)? else {
        return Ok(Vec::new());
    };
    let directory = executable.parent().ok_or_else(|| {
        format!(
            "{}: executable has no parent directory",
            executable.display()
        )
    })?;
    let mut pending = vec![executable.clone()];
    let mut inspected = BTreeSet::new();
    let mut requirements: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    let mut descriptors = 0;
    while let Some(path) = pending.pop() {
        let Some(path) = canonical_file(&path)? else {
            continue;
        };
        if path.parent() != Some(directory) {
            return Err(format!(
                "{}: imported DLL resolves outside the executable directory",
                path.display()
            ));
        }
        if !inspected.insert(path.clone()) {
            continue;
        }
        if inspected.len() > MAX_FILES {
            return Err(format!(
                "{}: dependency inspection exceeds the {MAX_FILES}-file limit",
                path.display()
            ));
        }
        let Some(imports) = inspect_file(&path, &mut descriptors)
            .map_err(|error| format!("{}: {error}", path.display()))?
        else {
            continue;
        };
        for name in imports.names {
            let normalized = name.to_ascii_lowercase();
            if let Some(family) = runtime_family(&normalized) {
                requirements
                    .entry((family, imports.architecture))
                    .or_default()
                    .insert(normalized.clone());
            }
            if normalized.ends_with(".dll") {
                // On Windows filesystem lookup is case insensitive. Retaining
                // the import spelling also permits deterministic non-Windows
                // fixtures without a directory scan or guessed search path.
                pending.push(directory.join(name));
            }
        }
    }
    Ok(requirements
        .into_iter()
        .map(
            |((family, architecture), names)| WindowsRuntimeRequirement {
                family,
                architecture,
                imported_dlls: names.into_iter().collect(),
                search_directories: vec![directory.to_path_buf()],
            },
        )
        .collect())
}

fn canonical_file(path: &Path) -> Result<Option<PathBuf>, String> {
    match path.canonicalize() {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "{}: cannot resolve dependency: {error}",
            path.display()
        )),
    }
}

fn runtime_family(name: &str) -> Option<WindowsRuntimeFamily> {
    use WindowsRuntimeFamily::*;
    match name {
        "msvcr100.dll" | "msvcp100.dll" => Some(VisualCpp2010),
        "msvcr110.dll" | "msvcp110.dll" => Some(VisualCpp2012),
        "msvcr120.dll" | "msvcp120.dll" => Some(VisualCpp2013),
        "concrt140.dll" => Some(VisualCpp2015),
        name if name.ends_with(".dll")
            && (name.starts_with("vcruntime140") || name.starts_with("msvcp140")) =>
        {
            Some(VisualCpp2015)
        }
        name if legacy_directx(name) => Some(DirectXJune2010),
        // ucrtbase, newer DirectX and msvcrt are supplied by supported Windows.
        _ => None,
    }
}

fn legacy_directx(name: &str) -> bool {
    [
        ("d3dx9_", 24, 43),
        ("d3dx10_", 33, 43),
        ("d3dx11_", 42, 43),
        ("d3dcompiler_", 33, 43),
        ("d3dcsx_", 42, 43),
        ("xinput1_", 1, 3),
        ("xaudio2_", 0, 7),
        ("x3daudio1_", 0, 7),
        ("xapofx1_", 0, 5),
        ("xactengine3_", 0, 7),
    ]
    .iter()
    .any(|(prefix, minimum, maximum)| {
        name.strip_prefix(*prefix)
            .and_then(|suffix| suffix.strip_suffix(".dll"))
            .filter(|version| {
                !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
            })
            .and_then(|version| version.parse::<u8>().ok())
            .is_some_and(|version| (*minimum..=*maximum).contains(&version))
    })
}

struct Imports {
    architecture: WindowsRuntimeArchitecture,
    names: BTreeSet<String>,
}

struct Reader<R> {
    file: R,
    length: u64,
}

impl<R: Read + Seek> Reader<R> {
    fn bytes<const N: usize>(&mut self, offset: u64) -> Result<[u8; N], String> {
        let mut bytes = [0; N];
        self.read(offset, &mut bytes)?;
        Ok(bytes)
    }

    fn read(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), String> {
        self.span(offset, bytes.len() as u64)?;
        self.file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.read_exact(bytes))
            .map_err(|error| format!("cannot read PE offset 0x{offset:x}: {error}"))
    }

    fn span(&self, offset: u64, size: u64) -> Result<(), String> {
        if offset.checked_add(size).is_none_or(|end| end > self.length) {
            Err(format!("PE range 0x{offset:x}+0x{size:x} exceeds the file"))
        } else {
            Ok(())
        }
    }

    fn u16(&mut self, offset: u64) -> Result<u16, String> {
        self.bytes(offset).map(u16::from_le_bytes)
    }

    fn u32(&mut self, offset: u64) -> Result<u32, String> {
        self.bytes(offset).map(u32::from_le_bytes)
    }

    fn u64(&mut self, offset: u64) -> Result<u64, String> {
        self.bytes(offset).map(u64::from_le_bytes)
    }
}

struct Section {
    rva: u64,
    virtual_size: u64,
    raw: u64,
    raw_size: u64,
}

struct PeLayout {
    architecture: WindowsRuntimeArchitecture,
    image_base: u64,
    header_size: u64,
    directories: Vec<(u32, u32)>,
    sections: Vec<Section>,
}

impl PeLayout {
    fn locate(&self, rva: u32, size: u64) -> Result<(u64, u64), String> {
        let (offset, available) = self.locate_image(rva, size)?;
        if available < size {
            return Err(format!("RVA 0x{rva:x} points to unbacked section data"));
        }
        Ok((offset, available))
    }

    fn locate_image(&self, rva: u32, size: u64) -> Result<(u64, u64), String> {
        let rva = u64::from(rva);
        let mut located = None;
        let mut matches = 0;
        if rva < self.header_size && rva + size <= self.header_size {
            located = Some((rva, self.header_size - rva));
            matches += 1;
        }
        for section in &self.sections {
            if rva < section.rva || rva >= section.rva + section.virtual_size.max(section.raw_size)
            {
                continue;
            }
            let delta = rva - section.rva;
            if delta
                .checked_add(size)
                .is_none_or(|end| end > section.virtual_size.max(section.raw_size))
            {
                return Err(format!("RVA 0x{rva:x} exceeds its mapped section"));
            }
            located = Some((section.raw + delta, section.raw_size.saturating_sub(delta)));
            matches += 1;
        }
        if matches != 1 {
            return Err(format!("RVA 0x{rva:x} is unmapped or ambiguous"));
        }
        located.ok_or_else(|| format!("RVA 0x{rva:x} has no file mapping"))
    }
}

fn inspect_file(path: &Path, descriptors: &mut usize) -> Result<Option<Imports>, String> {
    let file = File::open(path).map_err(|error| format!("cannot open PE file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect PE file: {error}"))?;
    if !metadata.is_file() {
        return Err(String::from("dependency is not a regular file"));
    }
    inspect_reader(
        Reader {
            file,
            length: metadata.len(),
        },
        descriptors,
    )
}

fn inspect_reader<R: Read + Seek>(
    mut reader: Reader<R>,
    descriptors: &mut usize,
) -> Result<Option<Imports>, String> {
    if reader.length < 2 || reader.bytes::<2>(0)? != *b"MZ" {
        return Ok(None);
    }
    let pe = u64::from(reader.u32(60)?);
    if pe < 64 || pe + 24 > MAX_HEADER_BYTES {
        return Err(format!(
            "PE header offset 0x{pe:x} is outside the header budget"
        ));
    }
    if reader.bytes::<4>(pe)? != *b"PE\0\0" {
        return Err(format!("invalid PE signature at offset 0x{pe:x}"));
    }
    let machine = reader.u16(pe + 4)?;
    let section_count = u64::from(reader.u16(pe + 6)?);
    if !(1..=96).contains(&section_count) {
        return Err(format!("invalid PE section count {section_count}"));
    }
    let optional_size = u64::from(reader.u16(pe + 20)?);
    let optional = pe + 24;
    let table = optional + optional_size;
    let table_end = table + section_count * 40;
    if table_end > MAX_HEADER_BYTES {
        return Err(String::from("PE headers exceed the 1 MiB header limit"));
    }
    reader.span(optional, optional_size + section_count * 40)?;
    let (architecture, directory_start, image_base) = match (machine, reader.u16(optional)?) {
        (0x014c, 0x010b) if optional_size >= 96 => (
            WindowsRuntimeArchitecture::X86,
            96,
            u64::from(reader.u32(optional + 28)?),
        ),
        (0x8664, 0x020b) if optional_size >= 112 => (
            WindowsRuntimeArchitecture::X64,
            112,
            reader.u64(optional + 24)?,
        ),
        _ => {
            return Err(format!(
                "unsupported or inconsistent PE machine/header 0x{machine:x}"
            ));
        }
    };
    let count = u64::from(reader.u32(optional + directory_start - 4)?);
    if count > 16 || directory_start + count * 8 > optional_size {
        return Err(String::from("invalid PE data-directory table"));
    }
    let header_size = u64::from(reader.u32(optional + 60)?);
    if header_size < table_end || header_size > MAX_HEADER_BYTES || header_size > reader.length {
        return Err(String::from("invalid or oversized PE SizeOfHeaders"));
    }
    let mut directories = Vec::new();
    for index in 0..count {
        let offset = optional + directory_start + index * 8;
        directories.push((reader.u32(offset)?, reader.u32(offset + 4)?));
    }
    let mut sections = Vec::new();
    for index in 0..section_count {
        let offset = table + index * 40;
        let section = Section {
            virtual_size: u64::from(reader.u32(offset + 8)?),
            rva: u64::from(reader.u32(offset + 12)?),
            raw_size: u64::from(reader.u32(offset + 16)?),
            raw: u64::from(reader.u32(offset + 20)?),
        };
        reader.span(section.raw, section.raw_size)?;
        if section.rva + section.virtual_size.max(section.raw_size) > u64::from(u32::MAX) + 1 {
            return Err(format!("section {index} overflows the RVA address space"));
        }
        sections.push(section);
    }
    let layout = PeLayout {
        architecture,
        image_base,
        header_size,
        directories,
        sections,
    };
    let mut names = BTreeSet::new();
    read_import_directory(&mut reader, &layout, 1, false, descriptors, &mut names)?;
    read_import_directory(&mut reader, &layout, 13, true, descriptors, &mut names)?;
    Ok(Some(Imports {
        architecture: layout.architecture,
        names,
    }))
}

fn read_import_directory<R: Read + Seek>(
    reader: &mut Reader<R>,
    layout: &PeLayout,
    index: usize,
    delay: bool,
    descriptors: &mut usize,
    names: &mut BTreeSet<String>,
) -> Result<(), String> {
    let Some(&(rva, size)) = layout.directories.get(index) else {
        return Ok(());
    };
    if rva == 0 && size == 0 {
        return Ok(());
    }
    let width = if delay { 32 } else { 20 };
    if rva == 0 || size < width {
        return Err(format!("invalid import directory {index}"));
    }
    layout.locate_image(rva, u64::from(size))?;
    for entry in 0..size / width {
        if *descriptors >= MAX_IMPORTS {
            return Err(format!(
                "import descriptors exceed the {MAX_IMPORTS}-entry limit"
            ));
        }
        *descriptors += 1;
        let entry_rva = rva
            .checked_add(entry * width)
            .ok_or("import descriptor overflows the RVA address space")?;
        let (entry_offset, available) = layout.locate_image(entry_rva, u64::from(width))?;
        let mut bytes = [0; 32];
        // PE sections are zero-filled past SizeOfRawData up to VirtualSize.
        // In protected executables a delay directory can initially contain only
        // that null descriptor. Inspect the loader-visible bytes without loading
        // or unpacking code; DLL names still require nonempty file-backed data.
        // https://learn.microsoft.com/windows/win32/debug/pe-format#section-table-section-headers
        let backed = available.min(u64::from(width)) as usize;
        if backed != 0 {
            reader.read(entry_offset, &mut bytes[..backed])?;
        }
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        let name = if delay {
            let attributes =
                u32::from_le_bytes(bytes[..4].try_into().map_err(|_| "invalid attributes")?);
            let name = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| "invalid name")?);
            match attributes {
                1 => name,
                0 => u64::from(name)
                    .checked_sub(layout.image_base)
                    .and_then(|rva| u32::try_from(rva).ok())
                    .ok_or_else(|| format!("invalid delay-import VA at 0x{entry_offset:x}"))?,
                _ => {
                    return Err(format!(
                        "unsupported delay-import attributes at 0x{entry_offset:x}"
                    ));
                }
            }
        } else {
            u32::from_le_bytes(
                bytes[12..16]
                    .try_into()
                    .map_err(|_| "invalid import name")?,
            )
        };
        names.insert(read_dll_name(reader, layout, name)?);
    }
    Err(format!(
        "import directory {index} has no bounded null terminator"
    ))
}

fn read_dll_name<R: Read + Seek>(
    reader: &mut Reader<R>,
    layout: &PeLayout,
    rva: u32,
) -> Result<String, String> {
    if rva == 0 {
        return Err(String::from("DLL name has a zero RVA"));
    }
    let (offset, available) = layout.locate(rva, 1)?;
    let count = available.min(MAX_DLL_NAME_BYTES as u64) as usize;
    let mut bytes = [0; MAX_DLL_NAME_BYTES];
    reader.read(offset, &mut bytes[..count])?;
    let end = bytes[..count]
        .iter()
        .position(|byte| *byte == 0)
        .filter(|end| *end > 0)
        .ok_or_else(|| format!("DLL name at RVA 0x{rva:x} is empty or unterminated"))?;
    let bytes = &bytes[..end];
    if bytes
        .iter()
        .any(|byte| !(32..=126).contains(byte) || b"/\\:".contains(byte))
    {
        return Err(format!(
            "DLL name at RVA 0x{rva:x} is not a plain ASCII filename"
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|error| format!("invalid DLL name: {error}"))
}

#[cfg(test)]
#[path = "windows_runtime_requirements_tests.rs"]
mod tests;
