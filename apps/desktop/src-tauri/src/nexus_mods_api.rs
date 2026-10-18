use super::*;
use serde::de::DeserializeOwned;

const API_BASE: &str = "https://api.nexusmods.com/v1";
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct NexusApi {
    client: reqwest::Client,
    key: reqwest::header::HeaderValue,
}

#[derive(Deserialize)]
pub(super) struct NexusAccount {
    pub(super) user_id: u64,
    pub(super) name: String,
    pub(super) is_premium: bool,
}

#[derive(Deserialize)]
pub(super) struct NexusMod {
    mod_id: u64,
    domain_name: String,
    pub(super) name: String,
    available: bool,
}

#[derive(Deserialize)]
pub(super) struct NexusFile {
    pub(super) file_id: u64,
    pub(super) name: String,
    pub(super) file_name: String,
    pub(super) version: String,
    pub(super) category_id: Option<u32>,
    pub(super) category_name: Option<String>,
    #[serde(default)]
    pub(super) description: String,
    size_in_bytes: Option<u64>,
    size_kb: Option<u64>,
    size: Option<u64>,
}

impl NexusFile {
    pub(super) fn exact_size_bytes(&self) -> Option<u64> {
        self.size_in_bytes
    }

    pub(super) fn size_bytes(&self) -> Result<u64, String> {
        self.size_in_bytes
            .or_else(|| {
                self.size_kb
                    .or(self.size)
                    .and_then(|size| size.checked_mul(1024))
            })
            .ok_or_else(|| metadata_error("Nexus did not return a valid file size."))
    }
}

#[derive(Deserialize)]
struct NexusFiles {
    files: Vec<NexusFile>,
}

#[derive(Deserialize)]
struct DownloadLink {
    #[serde(rename = "URI")]
    uri: String,
}

impl NexusApi {
    pub(super) fn new(api_key: &str) -> Result<Self, String> {
        if api_key.is_empty() || api_key.len() > 4096 {
            return Err(failure(
                "nexus_auth_invalid",
                "Enter a valid Nexus API key.",
            ));
        }
        let mut key = reqwest::header::HeaderValue::from_str(api_key)
            .map_err(|_| failure("nexus_auth_invalid", "Enter a valid Nexus API key."))?;
        key.set_sensitive(true);
        let client = reqwest::Client::builder()
            .user_agent(concat!("LanGameServerManager/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| network_error())?;
        Ok(Self { client, key })
    }

    fn request(&self, url: reqwest::Url) -> Result<reqwest::Request, String> {
        // Authentication is attached to this single fixed-origin request, never client defaults.
        if url.scheme() != "https"
            || url.host_str() != Some("api.nexusmods.com")
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || !url.path().starts_with("/v1/")
        {
            return Err(metadata_error("Invalid Nexus API request origin."));
        }
        self.client
            .get(url)
            .header("apikey", self.key.clone())
            .header("Application-Name", "LanGameServerManager")
            .header("Application-Version", env!("CARGO_PKG_VERSION"))
            .header(reqwest::header::ACCEPT, "application/json")
            .build()
            .map_err(|_| network_error())
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        self.get_with_download_permission(path, false).await
    }

    async fn get_with_download_permission<T: DeserializeOwned>(
        &self,
        path: &str,
        free_keyless_download: bool,
    ) -> Result<T, String> {
        let url = reqwest::Url::parse(&format!("{API_BASE}{path}"))
            .map_err(|_| metadata_error("Invalid Nexus API request path."))?;
        let request = self.request(url)?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|_| network_error())?;
        read_json(response, MAX_METADATA_BYTES, free_keyless_download).await
    }

    pub(super) async fn account(&self) -> Result<NexusAccount, String> {
        let account: NexusAccount = self.get("/users/validate").await?;
        if account.user_id == 0 || account.name.is_empty() || account.name.len() > 512 {
            return Err(metadata_error("Nexus returned invalid account details."));
        }
        Ok(account)
    }

    pub(super) async fn mod_info(&self, reference: &NexusReference) -> Result<NexusMod, String> {
        let metadata: NexusMod = self.get(&mod_path(reference)).await?;
        if metadata.mod_id.to_string() != reference.mod_id
            || metadata.domain_name != reference.domain
        {
            return Err(failure(
                "nexus_game_mismatch",
                "Nexus returned a different Mod or game than requested.",
            ));
        }
        if !metadata.available || metadata.name.is_empty() || metadata.name.len() > 1024 {
            return Err(failure(
                "nexus_file_unavailable",
                "This Nexus Mod is hidden, removed, or unavailable to this account.",
            ));
        }
        Ok(metadata)
    }

    pub(super) async fn files(&self, reference: &NexusReference) -> Result<Vec<NexusFile>, String> {
        let files: NexusFiles = self.get(&format!("{}/files", mod_path(reference))).await?;
        if files.files.len() > MAX_FILES {
            return Err(metadata_error(
                "Nexus returned too many files for this Mod.",
            ));
        }
        let mut ids = HashSet::new();
        for file in &files.files {
            validate_file_metadata(file)?;
            if !ids.insert(file.file_id) {
                return Err(metadata_error("Nexus returned duplicate file IDs."));
            }
        }
        Ok(files.files)
    }

    pub(super) async fn file(
        &self,
        reference: &NexusReference,
        file_id: &str,
    ) -> Result<NexusFile, String> {
        let file: NexusFile = self
            .get(&format!("{}/files/{file_id}", mod_path(reference)))
            .await?;
        validate_file_metadata(&file)?;
        if file.file_id.to_string() != file_id {
            return Err(metadata_error(
                "Nexus returned a different file than selected.",
            ));
        }
        Ok(file)
    }

    pub(super) async fn download_urls(
        &self,
        reference: &NexusReference,
        file_id: &str,
        free_keyless_download: bool,
    ) -> Result<Vec<String>, String> {
        let links: Vec<DownloadLink> = self
            .get_with_download_permission(
                &download_path(reference, file_id)?,
                free_keyless_download,
            )
            .await?;
        if links.is_empty() || links.len() > 32 {
            return Err(metadata_error("Nexus returned no usable download mirror."));
        }
        Ok(links.into_iter().map(|link| link.uri).collect())
    }
}

fn mod_path(reference: &NexusReference) -> String {
    format!("/games/{}/mods/{}", reference.domain, reference.mod_id)
}

fn download_path(reference: &NexusReference, file_id: &str) -> Result<String, String> {
    let mut path = format!("{}/files/{file_id}/download_link", mod_path(reference));
    if let Some(authorization) = &reference.authorization {
        let query = reqwest::Url::parse_with_params(
            "https://api.nexusmods.com/",
            [
                ("key", authorization.key.as_str()),
                ("expires", &authorization.expires.to_string()),
            ],
        )
        .map_err(|_| metadata_error("Invalid Nexus download authorization."))?;
        path.push('?');
        path.push_str(query.query().unwrap_or_default());
    }
    Ok(path)
}

fn validate_file_metadata(file: &NexusFile) -> Result<(), String> {
    if file.file_id == 0
        || file.name.len() > 1024
        || file.file_name.len() > 1024
        || file.version.len() > 512
        || file.description.len() > 64 * 1024
        || file
            .category_name
            .as_ref()
            .is_some_and(|category| category.len() > 128)
    {
        return Err(metadata_error("Nexus returned invalid file details."));
    }
    file.size_bytes()?;
    Ok(())
}

async fn read_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
    limit: usize,
    free_keyless_download: bool,
) -> Result<T, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 => failure(
                "nexus_auth_invalid",
                "The Nexus API key is invalid or has been revoked.",
            ),
            429 => failure(
                "nexus_rate_limited",
                "Nexus API rate limit reached. Try again after the reset time.",
            ),
            403 if free_keyless_download => failure(
                "nexus_premium_required",
                "Nexus requires Premium or an authorized Download with Manager (nxm) link for this file.",
            ),
            403 | 404 | 410 => failure(
                "nexus_file_unavailable",
                "This Nexus Mod or file is not available to this account.",
            ),
            _ => network_error(),
        });
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(metadata_error(
            "Nexus metadata exceeds its response size limit.",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| network_error())? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(metadata_error(
                "Nexus metadata exceeds its response size limit.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    // Parsing errors and server bodies can contain secrets; report a bounded local message.
    serde_json::from_slice(&bytes).map_err(|_| metadata_error("Nexus returned invalid metadata."))
}

fn metadata_error(message: &str) -> String {
    failure("nexus_file_unavailable", message)
}
fn network_error() -> String {
    failure(
        "nexus_network_error",
        "Could not contact the Nexus API. Check the connection and try again.",
    )
}

#[cfg(test)]
#[path = "nexus_mods_api_tests.rs"]
mod tests;
