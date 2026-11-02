//! Reads the saved platform account name, never the survivor's character name.
//!
//! Format references: Qowyn/ark-savegame-toolkit (5520d22), ArkProfile/GameObject;
//! Kakoen/ark-sa-save-tools (a07ee78), ArkArchive/ArkPropertyHeader/ArkUniqueNetIdRepl.
//! Only ASE profile v1 and ASA profile v7 are supported. World saves use different
//! version numbers and containers; they must not be passed to this reader.

use std::fmt;

pub(crate) const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_OBJECTS: usize = 64;
const MAX_PROPERTIES: usize = 4096;
const MAX_STRING_UNITS: usize = 512;
const ASA_PLAYER_CLASS: &str =
    "/Game/PrimalEarth/CoreBlueprints/PrimalPlayerDataBP.PrimalPlayerDataBP_C";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Edition {
    Ase,
    Asa,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProfileError {
    UnsupportedVersion(i32),
    Invalid(&'static str),
    LimitExceeded(&'static str),
    IdentityMismatch,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "Unsupported ARK profile version {version}")
            }
            Self::Invalid(field) => write!(f, "Invalid ARK profile {field}"),
            Self::LimitExceeded(field) => write!(f, "ARK profile {field} exceeds the read limit"),
            Self::IdentityMismatch => f.write_str("ARK profile account identity does not match"),
        }
    }
}

impl std::error::Error for ProfileError {}

type Result<T> = std::result::Result<T, ProfileError>;

/// A name is returned only after the profile's internal UniqueID matches the
/// requested account. A valid profile without a usable saved name returns None.
/// File I/O and freshness policy belong to the caller; this function is pure.
pub(crate) fn read_account_name(
    bytes: &[u8],
    expected_account_id: &str,
    edition: Edition,
) -> Result<Option<String>> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(ProfileError::LimitExceeded("file size"));
    }
    if expected_account_id.is_empty() || expected_account_id.len() > 256 {
        return Err(ProfileError::Invalid("expected account identity"));
    }
    let mut reader = Reader::new(bytes);
    let version = reader.i32()?;
    if !matches!((edition, version), (Edition::Ase, 1) | (Edition::Asa, 7)) {
        return Err(ProfileError::UnsupportedVersion(version));
    }
    if edition == Edition::Asa {
        reader.take(8)?; // UE package version fields, not an object count.
    }
    let count = reader.count(MAX_OBJECTS, "object count")?;
    let mut objects = Vec::with_capacity(count);
    let mut profile_offset = None;
    for _ in 0..count {
        reader.take(16)?; // Object GUID (also present in ASE profile v1).
        let class = reader.string()?;
        let is_profile = match edition {
            Edition::Ase => matches!(class.as_str(), "PrimalPlayerData" | "PrimalPlayerDataBP_C"),
            Edition::Asa => class == ASA_PLAYER_CLASS,
        };
        reader.boolean()?; // isItem
        let names = reader.count(MAX_OBJECTS, "object name count")?;
        for _ in 0..names {
            reader.string()?;
        }
        reader.boolean()?; // fromDataFile
        reader.i32()?; // dataFileIndex
        if reader.boolean()? {
            reader.take(if edition == Edition::Ase { 24 } else { 48 })?;
        }
        let offset = reader.count(bytes.len(), "property offset")?;
        if reader.i32()? != 0 {
            return Err(ProfileError::Invalid("object header trailer"));
        }
        if is_profile && profile_offset.replace(offset).is_some() {
            return Err(ProfileError::Invalid("duplicate player data object"));
        }
        objects.push(offset);
    }
    let offset = profile_offset.ok_or(ProfileError::Invalid("missing player data object"))?;
    objects.sort_unstable();
    if objects
        .iter()
        .any(|&value| value < reader.position || value >= bytes.len())
        || objects.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err(ProfileError::Invalid("object property offsets"));
    }
    let end = objects
        .iter()
        .copied()
        .find(|&value| value > offset)
        .unwrap_or(bytes.len());
    let mut properties = Reader::new(&bytes[offset..end]);
    if edition == Edition::Asa && properties.byte()? != 0 {
        return Err(ProfileError::Invalid("property block prefix"));
    }
    let mut result = None;
    for _ in 0..MAX_PROPERTIES {
        let Some(property) = read_property(&mut properties, edition)? else {
            return result.ok_or(ProfileError::Invalid("missing MyData"));
        };
        if property.name == "MyData" {
            property.require("StructProperty", Some("PrimalPlayerDataStruct"))?;
            if result.is_some() {
                return Err(ProfileError::Invalid("duplicate MyData"));
            }
            result = Some(read_player_data(
                property.payload,
                expected_account_id,
                edition,
            )?);
        }
    }
    Err(ProfileError::LimitExceeded("property count"))
}

fn read_player_data(bytes: &[u8], expected: &str, edition: Edition) -> Result<Option<String>> {
    let mut reader = Reader::new(bytes);
    let mut name = None;
    let mut has_identity = false;
    for _ in 0..MAX_PROPERTIES {
        let Some(property) = read_property(&mut reader, edition)? else {
            reader.finish()?;
            if !has_identity {
                return Err(ProfileError::Invalid("missing account identity"));
            }
            return Ok(name
                .filter(|value: &String| !value.trim().is_empty() && !value.contains('\u{fffd}')));
        };
        match property.name.as_str() {
            "PlayerName" => {
                property.require("StrProperty", None)?;
                if name.is_some() {
                    return Err(ProfileError::Invalid("duplicate account name"));
                }
                let mut value = Reader::new(property.payload);
                let text = value.string()?;
                value.finish()?;
                if text.len() > 1024 || text.chars().count() > 256 {
                    return Err(ProfileError::LimitExceeded("account name"));
                }
                if text.chars().any(char::is_control) {
                    return Err(ProfileError::Invalid("account name characters"));
                }
                name = Some(text);
            }
            "UniqueID" => {
                property.require("StructProperty", Some("UniqueNetIdRepl"))?;
                if has_identity {
                    return Err(ProfileError::Invalid("duplicate account identity"));
                }
                verify_identity(property.payload, expected, edition)?;
                has_identity = true;
            }
            _ => {} // In particular, never recurse into MyPlayerCharacterConfig.
        }
    }
    Err(ProfileError::LimitExceeded("property count"))
}

fn verify_identity(bytes: &[u8], expected: &str, edition: Edition) -> Result<()> {
    let mut reader = Reader::new(bytes);
    let matches = match edition {
        Edition::Ase => {
            reader.i32()?; // UniqueNetIdRepl discriminator in ASE.
            reader.string()? == expected
        }
        Edition::Asa => {
            reader.byte()?; // UniqueNetIdRepl encoding flags.
            if reader.string()? != "RedpointEOS" || reader.byte()? != 16 {
                return Err(ProfileError::Invalid("EOS identity encoding"));
            }
            let id = reader.take(16)?;
            expected.len() == 32
                && expected.as_bytes().as_chunks::<2>().0.iter().zip(id).all(
                    |(digits, byte)| match (hex(digits[0]), hex(digits[1])) {
                        (Some(high), Some(low)) => high * 16 + low == *byte,
                        _ => false,
                    },
                )
        }
    };
    reader.finish()?;
    if matches {
        Ok(())
    } else {
        Err(ProfileError::IdentityMismatch)
    }
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

struct Property<'a> {
    name: String,
    kind: String,
    subtype: Option<String>,
    index: usize,
    native_serialization: bool,
    payload: &'a [u8],
}

impl Property<'_> {
    fn require(&self, kind: &str, subtype: Option<&str>) -> Result<()> {
        if self.kind != kind
            || self.subtype.as_deref() != subtype
            || self.index != 0
            || self.native_serialization && subtype != Some("UniqueNetIdRepl")
        {
            return Err(ProfileError::Invalid("account property type or index"));
        }
        Ok(())
    }
}

fn read_property<'a>(reader: &mut Reader<'a>, edition: Edition) -> Result<Option<Property<'a>>> {
    let name = reader.string()?;
    if name == "None" {
        return Ok(None);
    }
    if name.is_empty() {
        return Err(ProfileError::Invalid("property name"));
    }
    let (kind, subtype, size, index, native_serialization) = if edition == Edition::Asa {
        let (kind, subtype, arguments) = read_type(reader, 0, &mut 0)?;
        if (kind == "StructProperty" && arguments != 1) || (kind == "StrProperty" && arguments != 0)
        {
            return Err(ProfileError::Invalid("property type arguments"));
        }
        let size = reader.count(MAX_PROFILE_BYTES, "property size")?;
        let flags = reader.byte()?;
        // UE's HasBinaryOrNativeSerialize (0x08) adds no header bytes.
        // Real ASA UniqueNetIdRepl properties use it for the binary EOS payload.
        let allowed_flags = match kind.as_str() {
            "BoolProperty" => 0x11,
            "StructProperty" => 0x09,
            _ => 1,
        };
        if flags & !allowed_flags != 0 || (kind == "BoolProperty" && size != 0) {
            return Err(ProfileError::Invalid("property flags"));
        }
        let index = if flags & 1 != 0 {
            reader.count(MAX_PROPERTIES, "array index")?
        } else {
            0
        };
        (kind, subtype, size, index, flags & 0x08 != 0)
    } else {
        let kind = reader.string()?;
        let size = reader.count(MAX_PROFILE_BYTES, "property size")?;
        let index = reader.count(MAX_PROPERTIES, "array index")?;
        let subtype = match kind.as_str() {
            "StructProperty" | "ArrayProperty" | "ByteProperty" => Some(reader.string()?),
            "BoolProperty" => {
                if reader.byte()? > 1 || size != 0 {
                    return Err(ProfileError::Invalid("boolean property"));
                }
                None
            }
            "Int8Property" | "Int16Property" | "IntProperty" | "Int64Property"
            | "UInt16Property" | "UInt32Property" | "UInt64Property" | "FloatProperty"
            | "DoubleProperty" | "NameProperty" | "StrProperty" | "ObjectProperty" => None,
            _ => return Err(ProfileError::Invalid("unsupported ASE property type")),
        };
        (kind, subtype, size, index, false)
    };
    Ok(Some(Property {
        name,
        kind,
        subtype,
        index,
        native_serialization,
        payload: reader.take(size)?,
    }))
}

// UE 5.5 serializes a type tree: FString name, i32 child count, then children.
// MyData's chain is StructProperty -> PrimalPlayerDataStruct -> /Script/ShooterGame.
fn read_type(
    reader: &mut Reader<'_>,
    depth: usize,
    nodes: &mut usize,
) -> Result<(String, Option<String>, usize)> {
    *nodes += 1;
    if depth > 4 || *nodes > 16 {
        return Err(ProfileError::LimitExceeded("property type tree"));
    }
    let name = reader.string()?;
    let count = reader.count(4, "property subtype count")?;
    let mut first = None;
    for index in 0..count {
        let (child, _, _) = read_type(reader, depth + 1, nodes)?;
        if index == 0 {
            first = Some(child);
        }
    }
    Ok((name, first, count))
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(ProfileError::Invalid("length overflow"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(ProfileError::Invalid("truncated data"))?;
        self.position = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn count(&mut self, maximum: usize, field: &'static str) -> Result<usize> {
        let value = usize::try_from(self.i32()?).map_err(|_| ProfileError::Invalid(field))?;
        if value > maximum {
            return Err(ProfileError::LimitExceeded(field));
        }
        Ok(value)
    }

    fn boolean(&mut self) -> Result<bool> {
        match self.i32()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProfileError::Invalid("object boolean")),
        }
    }

    fn string(&mut self) -> Result<String> {
        let length = self.i32()?;
        if length == 0 {
            return Ok(String::new());
        }
        let units = length
            .checked_abs()
            .ok_or(ProfileError::Invalid("string length"))? as usize;
        if units > MAX_STRING_UNITS {
            return Err(ProfileError::LimitExceeded("string length"));
        }
        if length > 0 {
            let bytes = self.take(units)?;
            if bytes[units - 1] != 0
                || !bytes[..units - 1].is_ascii()
                || bytes[..units - 1].contains(&0)
            {
                return Err(ProfileError::Invalid("ASCII string"));
            }
            String::from_utf8(bytes[..units - 1].to_vec())
                .map_err(|_| ProfileError::Invalid("ASCII string"))
        } else {
            let bytes = self.take(units * 2)?;
            let mut words: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            if words.pop() != Some(0) || words.contains(&0) {
                return Err(ProfileError::Invalid("UTF-16 terminator"));
            }
            String::from_utf16(&words).map_err(|_| ProfileError::Invalid("UTF-16 string"))
        }
    }

    fn finish(&self) -> Result<()> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(ProfileError::Invalid("property payload length"))
        }
    }
}

#[cfg(test)]
#[path = "ark_profile_tests.rs"]
pub(crate) mod tests;
