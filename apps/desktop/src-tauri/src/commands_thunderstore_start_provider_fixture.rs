//! Test-only provider boundary. It replaces metadata/ZIP delivery, never the
//! resolver, runtime verification, unpacker, ownership record or publisher.
use super::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
pub(in crate::commands) struct FixtureProvider {
    pub packages: BTreeMap<String, serde_json::Value>,
    pub archives: BTreeMap<String, Vec<u8>>,
    pub queries: Mutex<Vec<(String, Option<String>)>>,
    pub downloads: Mutex<Vec<String>>,
    pub fail_metadata: bool,
    pub fail_download: bool,
    pub cancel_metadata: bool,
    pub mutate_target: Option<PathBuf>,
}

impl FixtureProvider {
    pub(super) fn metadata(
        &self,
        request: &ThunderstorePackageReference,
        cancellation: Option<&app_steamcmd::InstallCancellation>,
    ) -> Result<ThunderstorePackageMetadata, String> {
        let project = format!("{}-{}", request.namespace, request.package);
        self.queries
            .lock()
            .unwrap()
            .push((project.clone(), request.version.clone()));
        if self.cancel_metadata
            && let Some(cancellation) = cancellation
        {
            cancellation.cancel();
        }
        if let Some(path) = &self.mutate_target {
            fs::write(path, b"operator wrote during network query").unwrap();
        }
        if self.fail_metadata {
            return Err("synthetic provider network failure".into());
        }
        let key = request
            .version
            .as_ref()
            .map(|version| format!("{project}-{version}"))
            .unwrap_or(project);
        serde_json::from_value(
            self.packages
                .get(&key)
                .cloned()
                .ok_or("Fixture metadata request was not declared")?,
        )
        .map_err(|_| "Fixture response schema is invalid".into())
    }
    pub(super) fn archive(
        &self,
        project: &str,
        version: &str,
    ) -> Result<PathBuf, ThunderstorePreparationFailure> {
        let key = format!("{project}-{version}");
        self.downloads.lock().unwrap().push(key.clone());
        if self.fail_download {
            return Err(ThunderstorePreparationFailure {
                message: "synthetic package network failure".into(),
                reason: Some(app_core::RuntimeModUpdateReason::Network),
            });
        }
        let path = std::env::temp_dir().join(format!(
            "langame-ts-provider-fixture-{}.zip",
            uuid::Uuid::new_v4().simple()
        ));
        let bytes = self
            .archives
            .get(&key)
            .ok_or_else(|| "Fixture archive request was not declared".to_owned())?;
        fs::write(&path, bytes).map_err(|error| error.to_string())?;
        Ok(path)
    }
}
