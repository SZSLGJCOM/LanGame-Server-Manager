//! Lossless, offline editing of the verified Dragonwilds SPUD world-settings
//! contract. Filesystem ownership, stopped-process checks and backups belong to
//! the caller; this module only transforms validated bytes.

use std::collections::BTreeMap;

mod map;
pub(crate) mod metadata;
mod spud;

use spud::{
    Chunk, NamedTable, Reader, Result, Table, chunks, encode_chunk, encode_string, replace_chunk,
    unique,
};

#[derive(Debug, PartialEq)]
pub(crate) struct NativeWorldSettings {
    pub world_name: String,
    pub world_mode: u16,
    pub hardcore_state: u16,
    pub overrides: BTreeMap<String, f32>,
}

struct Info<'a> {
    prefix: &'a [u8],
    children: Vec<Chunk<'a>>,
    custom_index: usize,
    custom: NamedTable<'a>,
    archive_versions: &'a [u8],
}

struct WorldObject<'a> {
    object_index: usize,
    prefix: &'a [u8],
    children: Vec<Chunk<'a>>,
    properties_index: usize,
    properties: Table<'a>,
    mode_index: usize,
    hardcore_index: usize,
    map_index: usize,
    name_index: usize,
    guid_index: usize,
}

struct Global<'a> {
    prefix: &'a [u8],
    children: Vec<Chunk<'a>>,
    objects_index: usize,
    objects: Vec<Chunk<'a>>,
    world: WorldObject<'a>,
}

struct Save<'a> {
    children: Vec<Chunk<'a>>,
    info_index: usize,
    global_index: usize,
    info: Info<'a>,
    global: Global<'a>,
    entries: Vec<map::Entry<'a>>,
    settings: NativeWorldSettings,
}

fn parse_info(body: &[u8]) -> Result<Info<'_>> {
    let mut reader = Reader::new(body);
    if reader.u16()? != 8 {
        return Err("Unsupported Dragonwilds SPUD system version; load and save this world with the supported game first".into());
    }
    let archive_versions = reader.take(8)?;
    reader.u32()?; // FText flags; its original representation is retained.
    match reader.u8()? {
        255 => match reader.u32()? {
            0 => {}
            1 => {
                reader.string()?;
            }
            _ => return Err("Invalid Dragonwilds culture-invariant title flag".into()),
        },
        0 => {
            reader.string()?;
            reader.string()?;
            reader.string()?;
        }
        _ => return Err("Unsupported Dragonwilds save-title serialization".into()),
    }
    reader.string()?; // INFO timestamp string, not the CINF timestamp ticks.
    let prefix = &body[..reader.position];
    let children = chunks(&body[reader.position..])?;
    let custom_index = unique(&children, b"CINF")?;
    let custom = NamedTable::read(children[custom_index].body)?;
    Ok(Info {
        prefix,
        children,
        custom_index,
        custom,
        archive_versions,
    })
}

fn property_index(
    properties: &[(String, Option<String>, u16)],
    name: &str,
    kind: u16,
) -> Result<usize> {
    let matching: Vec<_> = properties
        .iter()
        .enumerate()
        .filter(|(_, p)| p.0 == name && p.1.as_deref() == Some("WorldSaveSettings"))
        .collect();
    if matching.len() != 1 || matching[0].1.2 != kind {
        return Err(format!(
            "Missing, ambiguous or unsupported WorldSaveSettings/{name} metadata"
        ));
    }
    Ok(matching[0].0)
}

fn parse_global<'a>(body: &'a [u8], archive_versions: &[u8]) -> Result<Global<'a>> {
    let mut reader = Reader::new(body);
    if reader.string()? != "L_World" {
        return Err("Unsupported Dragonwilds world map".into());
    }
    let prefix = &body[..reader.position];
    let children = chunks(&body[reader.position..])?;
    let definitions = spud::metadata(children[unique(&children, b"META")?].body)?;
    let objects_index = unique(&children, b"GOBS")?;
    let objects = chunks(children[objects_index].body)?;
    let mut matching = Vec::new();
    for (object_index, object) in objects.iter().enumerate() {
        if object.tag != *b"NOBJ" {
            continue;
        }
        let mut reader = Reader::new(object.body);
        let class = usize::try_from(reader.u32()?).map_err(|_| "Object class index overflow")?;
        let definition = definitions
            .get(class)
            .ok_or("Invalid SPUD object class index")?;
        if definition.class_name != "/Script/Dominion.PersistenceSubsystem" {
            continue;
        }
        reader.string()?;
        if reader.u32()? != 0 || reader.take(8)? != archive_versions {
            return Err("Unsupported Dragonwilds persistence-object archive framing".into());
        }
        let object_prefix = &object.body[..reader.position];
        let object_children = chunks(&object.body[reader.position..])?;
        let properties_index = unique(&object_children, b"PROP")?;
        let mut property_reader = Reader::new(object_children[properties_index].body);
        let properties = Table::read(&mut property_reader)?;
        if properties.fields.len() != definition.properties.len() {
            return Err("SPUD object property count differs from metadata".into());
        }
        matching.push(WorldObject {
            object_index,
            prefix: object_prefix,
            children: object_children,
            properties_index,
            properties,
            mode_index: property_index(&definition.properties, "SurvivalDifficulty", 1)?,
            hardcore_index: property_index(&definition.properties, "HardcoreState", 1)?,
            map_index: property_index(&definition.properties, "CustomDifficultySettings", 64)?,
            name_index: property_index(&definition.properties, "WorldName", 30)?,
            guid_index: property_index(&definition.properties, "WorldSaveGuid", 23)?,
        });
    }
    if matching.len() != 1 {
        return Err("Expected exactly one Dragonwilds world-settings persistence object".into());
    }
    let world = matching
        .pop()
        .ok_or("Missing world-settings persistence object")?;
    Ok(Global {
        prefix,
        children,
        objects_index,
        objects,
        world,
    })
}

fn scalar_u32(bytes: &[u8]) -> Result<u32> {
    let mut reader = Reader::new(bytes);
    let value = reader.u32()?;
    reader.finish()?;
    Ok(value)
}

fn scalar_u16(bytes: &[u8]) -> Result<u16> {
    let mut reader = Reader::new(bytes);
    let value = reader.u16()?;
    reader.finish()?;
    Ok(value)
}

fn scalar_string(bytes: &[u8]) -> Result<String> {
    let mut reader = Reader::new(bytes);
    let value = reader.string()?;
    reader.finish()?;
    Ok(value)
}

fn parse(bytes: &[u8]) -> Result<Save<'_>> {
    let outer = chunks(bytes)?;
    if outer.len() != 1 || outer[0].tag != *b"SAVE" {
        return Err("Expected one complete, uncompressed Dragonwilds SAVE envelope".into());
    }
    let children = chunks(outer[0].body)?;
    let info_index = unique(&children, b"INFO")?;
    if info_index != 0 {
        return Err("Dragonwilds INFO must be the first SAVE chunk".into());
    }
    let global_index = unique(&children, b"GLOB")?;
    unique(&children, b"LVLS")?;
    let info = parse_info(children[info_index].body)?;
    let global = parse_global(children[global_index].body, info.archive_versions)?;
    let world = &global.world;
    let custom = &info.custom;
    let mode = scalar_u16(world.properties.fields[world.mode_index])?;
    if mode > 3 || scalar_u32(custom.field("SurvivalDifficulty")?)? != u32::from(mode) {
        return Err(
            "Dragonwilds world-mode copies disagree or contain an unsupported value".into(),
        );
    }
    let hardcore = scalar_u16(world.properties.fields[world.hardcore_index])?;
    if hardcore > 4 || scalar_u32(custom.field("HardcoreState")?)? != u32::from(hardcore) {
        return Err(
            "Dragonwilds hardcore-state copies disagree or contain an unsupported value".into(),
        );
    }
    let name = scalar_string(world.properties.fields[world.name_index])?;
    if scalar_string(custom.field("WorldName")?)? != name {
        return Err("Dragonwilds world-name copies disagree".into());
    }
    let mut guid = Vec::new();
    for field in ["GUID_A", "GUID_B", "GUID_C", "GUID_D"] {
        guid.extend_from_slice(&scalar_u32(custom.field(field)?)?.to_le_bytes());
    }
    if world.properties.fields[world.guid_index] != guid {
        return Err("Dragonwilds world-identity copies disagree".into());
    }
    let entries = map::decode(world.properties.fields[world.map_index])?;
    let mut overrides = BTreeMap::new();
    for entry in &entries {
        let value = map::float(custom.field(&entry.tag)?)?;
        if value.to_bits() != entry.value.to_bits() {
            return Err(format!(
                "Dragonwilds difficulty copies disagree for {}",
                entry.tag
            ));
        }
        overrides.insert(entry.tag.clone(), value);
    }
    if custom
        .names
        .iter()
        .any(|name| name.starts_with("Difficulty.") && !overrides.contains_key(name))
    {
        return Err(
            "Dragonwilds CINF contains difficulty overrides missing from the persistence map"
                .into(),
        );
    }
    let settings = NativeWorldSettings {
        world_name: name,
        world_mode: mode,
        hardcore_state: hardcore,
        overrides,
    };
    Ok(Save {
        children,
        info_index,
        global_index,
        info,
        global,
        entries,
        settings,
    })
}

pub(crate) fn decode_world_settings(bytes: &[u8]) -> Result<NativeWorldSettings> {
    Ok(parse(bytes)?.settings)
}

pub(crate) fn patch_world_settings(
    bytes: &[u8],
    world_mode: u16,
    overrides: &BTreeMap<String, f32>,
) -> Result<Vec<u8>> {
    if world_mode > 3 {
        return Err("Unsupported Dragonwilds world mode".into());
    }
    let save = parse(bytes)?;
    let encoded_map = map::encode(&save.entries, overrides)?;
    let custom = &save.info.custom;
    let mut names = Vec::new();
    let mut fields = Vec::new();
    for (index, name) in custom.names.iter().enumerate() {
        let original = custom.table.fields[index];
        if save.settings.overrides.contains_key(name) {
            if let Some(value) = overrides.get(name) {
                names.push(custom.raw_names[index].to_vec());
                fields.push(value.to_le_bytes().to_vec());
            }
        } else {
            names.push(custom.raw_names[index].to_vec());
            fields.push(if name == "SurvivalDifficulty" {
                u32::from(world_mode).to_le_bytes().to_vec()
            } else {
                original.to_vec()
            });
        }
    }
    for (tag, value) in overrides {
        if !save.settings.overrides.contains_key(tag) {
            // A new tag must not shadow an unrelated CINF property.
            if custom.names.contains(tag) {
                return Err("New difficulty override shadows a native CINF property".into());
            }
            names.push(encode_string(tag)?);
            fields.push(value.to_le_bytes().to_vec());
        }
    }
    let new_custom = encode_chunk(b"CINF", &NamedTable::encode(&names, &fields)?)?;
    let mut info_body = save.info.prefix.to_vec();
    info_body.extend_from_slice(&replace_chunk(
        &save.info.children,
        save.info.custom_index,
        &new_custom,
    ));
    let new_info = encode_chunk(b"INFO", &info_body)?;

    let world = &save.global.world;
    let mut properties: Vec<Vec<u8>> = world.properties.fields.iter().map(|v| v.to_vec()).collect();
    properties[world.mode_index] = world_mode.to_le_bytes().to_vec();
    properties[world.map_index] = encoded_map;
    let new_properties = encode_chunk(b"PROP", &Table::encode(&properties)?)?;
    let mut object_body = world.prefix.to_vec();
    object_body.extend_from_slice(&replace_chunk(
        &world.children,
        world.properties_index,
        &new_properties,
    ));
    let new_object = encode_chunk(b"NOBJ", &object_body)?;
    let new_objects = encode_chunk(
        b"GOBS",
        &replace_chunk(&save.global.objects, world.object_index, &new_object),
    )?;
    let mut global_body = save.global.prefix.to_vec();
    global_body.extend_from_slice(&replace_chunk(
        &save.global.children,
        save.global.objects_index,
        &new_objects,
    ));
    let new_global = encode_chunk(b"GLOB", &global_body)?;

    let mut body = Vec::new();
    for (index, child) in save.children.iter().enumerate() {
        body.extend_from_slice(if index == save.info_index {
            &new_info
        } else if index == save.global_index {
            &new_global
        } else {
            child.raw
        });
    }
    let result = encode_chunk(b"SAVE", &body)?;
    // Reparse the final envelope and both representations before returning any bytes.
    let validated = decode_world_settings(&result)?;
    if validated.world_mode != world_mode || validated.overrides != *overrides {
        return Err("Dragonwilds world-settings output validation failed".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
