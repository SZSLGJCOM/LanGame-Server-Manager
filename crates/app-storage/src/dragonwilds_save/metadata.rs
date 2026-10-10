use super::{parse_info, scalar_string};

pub(crate) struct WorldMetadata {
    pub name: String,
    pub saved_at_ticks: i64,
}

/// Dedicated-server selection uses CINF WorldName and TimeOfSave, independently
/// of INFO's timestamp and filesystem mtime. Read only the bounded INFO body.
pub(crate) fn read_world_metadata_info(body: &[u8]) -> Result<WorldMetadata, String> {
    let info = parse_info(body)?;
    let name = scalar_string(info.custom.field("WorldName")?)?;
    let time: [u8; 8] = info
        .custom
        .field("TimeOfSave")?
        .try_into()
        .map_err(|_| "Invalid Dragonwilds CINF TimeOfSave field".to_owned())?;
    let saved_at_ticks = i64::from_le_bytes(time);
    if saved_at_ticks <= 0 || saved_at_ticks > 3_155_378_975_999_999_999 {
        return Err("Invalid native world-save timestamp".into());
    }
    Ok(WorldMetadata {
        name,
        saved_at_ticks,
    })
}
