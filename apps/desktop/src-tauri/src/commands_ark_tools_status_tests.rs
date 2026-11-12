use super::*;

#[test]
fn ark_tools_status_reports_an_intact_previous_bundle_as_installed_and_updatable() {
    let status = ToolsStatus::from(install::InstallStatus {
        ready: true,
        matches_bundle: false,
        can_uninstall: true,
        issue: None,
        framework_version: "AsaApi 2.11",
    });
    assert_eq!(
        serde_json::to_value(status).unwrap(),
        json!({
            "installed": true,
            "updateAvailable": true,
            "connected": false,
            "canUninstall": true,
            "issue": null
        })
    );
}

#[test]
fn ark_tools_status_does_not_report_an_update_for_current_or_unverified_files() {
    for (ready, matches_bundle, can_uninstall, issue) in [
        (true, true, true, None),
        (
            false,
            false,
            false,
            Some("Prepare the ARK server extension while this instance is stopped"),
        ),
        (
            false,
            false,
            true,
            Some("Managed extension file is missing"),
        ),
    ] {
        let status = ToolsStatus::from(install::InstallStatus {
            ready,
            matches_bundle,
            can_uninstall,
            issue: issue.map(str::to_owned),
            framework_version: "AsaApi 2.11",
        });
        assert_eq!(status.installed, ready);
        assert!(!status.update_available);
        assert!(!status.connected);
        assert_eq!(status.can_uninstall, can_uninstall);
        assert_eq!(status.issue.as_deref(), issue);
    }
}
