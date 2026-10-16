//! Some games derive their loaded Mod set from more than the imported files.
use super::*;

pub(in crate::commands) fn validate_game_mod_mutation(module_id: &str) -> Result<(), String> {
    let (code, message, documentation_url) = match module_id {
        "astroneer" => (
            "manual_mod_integration_required",
            "Astroneer requires re-integration after its Mod list changes. LanGame does not yet regenerate the integrated package, so this operation was not performed. Use AstroModLoader Classic to install, enable, disable or remove Mods and regenerate 999-AstroModIntegrator_P.pak.",
            "https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers",
        ),
        "satisfactory" => (
            "mod_package_layout_unsupported",
            "Satisfactory requires server-compatible packages, .uplugin directory routing and coordinated loader and dependency management. LanGame does not yet implement this workflow, so this operation was not performed. Use SMM or ficsit-cli to install, enable, disable or remove server-compatible Mods.",
            "https://docs.ficsit.app/satisfactory-modding/latest/ForUsers/DedicatedServerSetup.html",
        ),
        _ => return Ok(()),
    };
    let details: Value = serde_json::json!({
        "code": code,
        "module_id": module_id,
        "message": message,
        "documentation_url": documentation_url,
    });
    Err(details.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astroneer_requires_reintegration_instead_of_reporting_file_changes_as_complete() {
        let error = validate_game_mod_mutation("astroneer").unwrap_err();
        let details: Value = serde_json::from_str(&error).unwrap();
        assert_eq!(details["code"], "manual_mod_integration_required");
        assert_eq!(details["module_id"], "astroneer");
        let message = details["message"].as_str().unwrap();
        assert!(message.contains("re-integration"));
        assert!(message.contains("AstroModLoader Classic"));
        assert!(message.contains("999-AstroModIntegrator_P.pak"));
        assert!(message.contains("not performed"));
        assert_eq!(
            details["documentation_url"],
            "https://astroneermodding.readthedocs.io/en/latest/guides/faq.html#can-i-use-mods-on-dedicated-servers"
        );
    }

    #[test]
    fn satisfactory_requires_its_external_manager_instead_of_generic_file_mutations() {
        let error = validate_game_mod_mutation("satisfactory").unwrap_err();
        let details: Value = serde_json::from_str(&error).unwrap();
        assert_eq!(details["code"], "mod_package_layout_unsupported");
        assert_eq!(details["module_id"], "satisfactory");
        let message = details["message"].as_str().unwrap();
        assert!(message.contains("server-compatible"));
        assert!(message.contains(".uplugin directory routing"));
        assert!(message.contains("SMM or ficsit-cli"));
        assert!(message.contains("not performed"));
        assert_eq!(
            details["documentation_url"],
            "https://docs.ficsit.app/satisfactory-modding/latest/ForUsers/DedicatedServerSetup.html"
        );
    }

    #[test]
    fn other_file_mod_workflows_keep_their_existing_management_boundary() {
        for module_id in [
            "abioticfactor",
            "sevendaystodie",
            "humanitz",
            "enshrouded",
            "valheim",
            "vrising",
            "corekeeper",
            "minecraft",
            "necesse",
            "rust",
            "windrose",
            "sonsoftheforest",
        ] {
            assert!(validate_game_mod_mutation(module_id).is_ok(), "{module_id}");
        }
    }
}
