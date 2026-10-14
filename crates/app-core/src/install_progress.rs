use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhase {
    Queued,
    Preparing,
    Downloading,
    Extracting,
    Installing,
    Verifying,
    Ready,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallVerificationStage {
    SteamFiles,
    PackageFiles,
    RecordingBaseline,
}

/// Local file processing counters, kept separate from network download bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallVerificationProgress {
    pub stage: InstallVerificationStage,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub completed_files: u64,
    pub total_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub percent: Option<f32>,
    pub elapsed_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<InstallVerificationProgress>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BackgroundJob;
    use serde_json::json;

    #[test]
    fn install_phases_use_stable_snake_case_values() {
        for (phase, expected) in [
            (InstallPhase::Queued, "queued"),
            (InstallPhase::Preparing, "preparing"),
            (InstallPhase::Downloading, "downloading"),
            (InstallPhase::Extracting, "extracting"),
            (InstallPhase::Installing, "installing"),
            (InstallPhase::Verifying, "verifying"),
            (InstallPhase::Ready, "ready"),
        ] {
            assert_eq!(serde_json::to_value(phase).unwrap(), json!(expected));
            assert_eq!(
                serde_json::from_value::<InstallPhase>(json!(expected)).unwrap(),
                phase
            );
        }
    }

    #[test]
    fn install_progress_round_trips_unknown_transfer_size() {
        let progress = InstallProgress {
            phase: InstallPhase::Downloading,
            downloaded_bytes: Some(4096),
            total_bytes: None,
            percent: None,
            elapsed_seconds: 17,
            verification: None,
        };
        let value = serde_json::to_value(&progress).unwrap();
        assert_eq!(
            value,
            json!({
                "phase": "downloading",
                "downloaded_bytes": 4096,
                "total_bytes": null,
                "percent": null,
                "elapsed_seconds": 17,
            })
        );
        assert_eq!(
            serde_json::from_value::<InstallProgress>(value).unwrap(),
            progress
        );
    }

    #[test]
    fn background_job_accepts_absent_install_progress() {
        let job: BackgroundJob = serde_json::from_value(json!({
            "id": "instance-start-test",
            "kind": "StartInstance",
            "label": "Start fixture instance",
            "status": "Pending",
            "progress_percent": 0.0,
            "target_id": "fixture-instance",
            "detail": null,
            "output_excerpt": null,
        }))
        .unwrap();
        assert!(job.install_progress.is_none());
        assert_eq!(
            serde_json::to_value(job).unwrap()["install_progress"],
            json!(null)
        );
    }

    #[test]
    fn verification_counters_round_trip_without_claiming_network_bytes() {
        for stage in [
            InstallVerificationStage::SteamFiles,
            InstallVerificationStage::PackageFiles,
            InstallVerificationStage::RecordingBaseline,
        ] {
            let progress = InstallProgress {
                phase: InstallPhase::Verifying,
                downloaded_bytes: None,
                total_bytes: None,
                percent: None,
                elapsed_seconds: 20,
                verification: Some(InstallVerificationProgress {
                    stage,
                    completed_bytes: 64,
                    total_bytes: 128,
                    completed_files: 1,
                    total_files: 2,
                }),
            };
            let encoded = serde_json::to_value(&progress).unwrap();
            assert!(encoded["downloaded_bytes"].is_null());
            assert!(encoded["total_bytes"].is_null());
            assert_eq!(
                serde_json::from_value::<InstallProgress>(encoded).unwrap(),
                progress
            );
        }
    }
}
