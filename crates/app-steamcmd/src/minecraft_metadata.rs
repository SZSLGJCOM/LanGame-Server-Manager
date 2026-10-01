use serde::Deserialize;
use sha1::{Digest, Sha1};

use super::http_download::fetch_text_validated;
use super::{
    InstallDeadline, MinecraftVersionDetails, MinecraftVersionManifestEntry, SteamCmdError,
};

pub(super) fn deserialize_sha1<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(serde::de::Error::custom(
            "Minecraft version metadata SHA1 must contain 40 hexadecimal characters",
        ));
    }
    Ok(value)
}

pub(super) async fn fetch_minecraft_version_details(
    client: &reqwest::Client,
    selected: &MinecraftVersionManifestEntry,
    deadline: InstallDeadline,
) -> Result<MinecraftVersionDetails, SteamCmdError> {
    fetch_text_validated(client, &selected.url, deadline, 4 * 1024 * 1024, |text| {
        parse_version_details(text, selected)
    })
    .await
    .map(|(_, details)| details)
}

fn parse_version_details(
    text: &str,
    selected: &MinecraftVersionManifestEntry,
) -> Result<MinecraftVersionDetails, String> {
    // The manifest hashes the exact UTF-8 response bytes, not reserialized JSON.
    let actual = Sha1::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual.eq_ignore_ascii_case(&selected.sha1) {
        return Err(format!(
            "Minecraft version metadata SHA1 mismatch for `{}`",
            selected.id
        ));
    }
    let details: MinecraftVersionDetails =
        serde_json::from_str(text).map_err(|error| error.to_string())?;
    if details.id != selected.id {
        return Err(format!(
            "Minecraft version metadata id `{}` does not match requested version `{}`",
            details.id, selected.id
        ));
    }
    Ok(details)
}

#[cfg(test)]
#[path = "minecraft_metadata_tests.rs"]
mod tests;
