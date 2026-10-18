//! Nexus v1 integration: explicit file selection, local credentials, and bounded transfers.
//! The v3 file/download endpoints remain experimental; see the official API specification.
use super::*;
use app_core::ModuleModSourceSpec;

#[path = "nexus_mods_api.rs"]
mod api;
#[path = "nexus_mods_download.rs"]
mod download;
#[path = "nexus_mods_reference.rs"]
mod reference;
pub(super) use download::download_selected_files;
use reference::{NexusReference, parse_reference, source_domain};

const MAX_REFERENCES: usize = 32;
const MAX_FILES: usize = 4096;

#[derive(Serialize)]
pub(crate) struct NexusAccountStatus {
    pub(crate) configured: bool,
    pub(crate) user_name: Option<String>,
    pub(crate) is_premium: bool,
}

#[derive(Serialize)]
pub(crate) struct NexusModFileChoice {
    pub(crate) file_id: String,
    pub(crate) name: String,
    pub(crate) file_name: String,
    pub(crate) version: String,
    pub(crate) category: String,
    pub(crate) size_bytes: u64,
    pub(crate) description: String,
    pub(crate) downloadable: bool,
    pub(crate) unavailable_reason: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct NexusModReview {
    pub(crate) reference_index: usize,
    pub(crate) game_domain: String,
    pub(crate) mod_id: String,
    pub(crate) mod_name: String,
    pub(crate) page_url: String,
    pub(crate) files: Vec<NexusModFileChoice>,
    pub(crate) selected_file_id: Option<String>,
}

// A reference can contain a short-lived download secret. Never derive Debug or Serialize.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NexusFileSelection {
    pub(crate) reference: String,
    pub(crate) file_id: String,
}

fn failure(code: &str, message: &str) -> String {
    json!({"code": code, "message": message}).to_string()
}

pub(super) async fn validate_api_key(api_key: &str) -> Result<NexusAccountStatus, String> {
    let api = api::NexusApi::new(api_key)?;
    let account = api.account().await?;
    Ok(NexusAccountStatus {
        configured: true,
        user_name: Some(account.name),
        is_premium: account.is_premium,
    })
}

pub(super) async fn review_references(
    source: &ModuleModSourceSpec,
    accepts: &[String],
    references: &[String],
    api_key: &str,
) -> Result<Vec<NexusModReview>, String> {
    validate_batch_len(references.len())?;
    let domain = source_domain(source)?;
    // Parse the complete batch before issuing any network request.
    let parsed = references
        .iter()
        .map(|value| parse_reference(value, &domain))
        .collect::<Result<Vec<_>, _>>()?;
    let api = api::NexusApi::new(api_key)?;
    let account = api.account().await?;
    let mut reviews = Vec::with_capacity(parsed.len());
    for (reference_index, reference) in parsed.iter().enumerate() {
        reference.validate_authorization(&account, unix_seconds()?)?;
        let metadata = api.mod_info(reference).await?;
        let files = api.files(reference).await?;
        let choices = files
            .iter()
            .map(|file| file_choice(file, accepts, reference))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(id) = &reference.file_id
            && !choices.iter().any(|file| &file.file_id == id)
        {
            return Err(failure(
                "nexus_file_unavailable",
                "The file in this Nexus link is no longer listed for this Mod.",
            ));
        }
        reviews.push(NexusModReview {
            reference_index,
            game_domain: reference.domain.clone(),
            mod_id: reference.mod_id.clone(),
            mod_name: metadata.name,
            page_url: reference.page_url(),
            files: choices,
            selected_file_id: reference.file_id.clone(),
        });
    }
    Ok(reviews)
}

fn validate_batch_len(len: usize) -> Result<(), String> {
    if len == 0 || len > MAX_REFERENCES {
        return Err(failure(
            "nexus_reference_invalid",
            "Select between 1 and 32 Nexus Mods files.",
        ));
    }
    Ok(())
}

fn unix_seconds() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| failure("nexus_download_expired", "The system clock is invalid."))
}

fn file_choice(
    file: &api::NexusFile,
    accepts: &[String],
    reference: &NexusReference,
) -> Result<NexusModFileChoice, String> {
    let size_bytes = file.size_bytes()?;
    let unavailable_reason = if !matches!(file.category_id, Some(1..=5)) {
        Some(
            "This Nexus file is removed, archived, or has no recognized downloadable category."
                .into(),
        )
    } else if safe_filename(&file.file_name).is_err() {
        Some("This file has an unsafe filename and cannot be installed automatically.".into())
    } else if !accepted_filename(&file.file_name, accepts) {
        Some("This file format is not supported by this server. Extract RAR/7z archives and import the compatible files locally.".into())
    } else if size_bytes > MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES {
        Some("This file exceeds the 256 MiB automatic download limit. Download it on the Nexus website and import it locally.".into())
    } else if reference.is_nxm
        && reference.file_id.as_deref() != Some(file.file_id.to_string().as_str())
    {
        Some("This nxm link authorizes a different file. Use a Mod page or a new nxm link to select this file.".into())
    } else {
        None
    };
    Ok(NexusModFileChoice {
        file_id: file.file_id.to_string(),
        name: file.name.clone(),
        file_name: file.file_name.clone(),
        version: file.version.clone(),
        category: file.category_name.clone().unwrap_or_default(),
        size_bytes,
        description: file.description.clone(),
        downloadable: unavailable_reason.is_none(),
        unavailable_reason,
    })
}

fn accepted_filename(filename: &str, accepts: &[String]) -> bool {
    let Some(extension) = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
    else {
        return false;
    };
    // Archive extraction only supports ZIP. Never treat an archive as a loose payload.
    !matches!(extension.to_ascii_lowercase().as_str(), "rar" | "7z")
        && accepts.iter().any(|accepted| {
            accepted
                .trim()
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        })
}

fn safe_filename(filename: &str) -> Result<(), String> {
    let stem = filename
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if filename.is_empty()
        || filename.len() > 240
        || filename.ends_with(['.', ' '])
        || filename
            .chars()
            .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
        || reserved
    {
        return Err(failure(
            "nexus_file_unavailable",
            "Nexus returned an unsafe file name.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "nexus_mods_tests.rs"]
mod tests;
