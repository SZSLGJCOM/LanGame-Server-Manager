use super::*;

#[test]
fn native_command_receipts_report_requested_without_claiming_disk_completion() {
    for response in [
        "Saved the game",
        "Saving the game (this may take a moment!)Saved the game",
        "",
        "World saved",
        "Game saved!",
    ] {
        assert_eq!(
            network_save_completion(
                "minecraft",
                "source_rcon",
                Some("save_world"),
                "save-all flush",
                response
            ),
            Ok(Some(WorldSaveCompletion::Requested))
        );
    }
    for response in [
        "Unknown command",
        "Not authorized",
        "Permission denied",
        "Saving failed",
        "Save failed: disk full",
        "Error executing command",
    ] {
        assert!(
            network_save_completion(
                "minecraft",
                "source_rcon",
                Some("save_world"),
                "save-all flush",
                response
            )
            .is_err(),
            "{response}"
        );
    }
    assert_eq!(
        network_save_completion(
            "minecraft",
            "source_rcon",
            None,
            "save-all flush",
            "Saved the game"
        ),
        Ok(None)
    );
}

#[test]
fn seven_days_native_telnet_save_rejections_are_not_success_receipts() {
    for response in [
        "*** ERROR: unknown command 'saveworld'",
        "Logon successful.\r\n*** ERROR: Exception while executing command 'saveworld'",
    ] {
        assert!(
            network_save_completion(
                "sevendaystodie",
                "telnet",
                Some("save_world"),
                "saveworld",
                response
            )
            .is_err()
        );
    }
    assert_eq!(
        network_save_completion(
            "sevendaystodie",
            "telnet",
            Some("save_world"),
            "saveworld",
            "Executing command 'saveworld'\r\nWorld saved without errors."
        ),
        Ok(Some(WorldSaveCompletion::Requested))
    );
}

#[test]
fn save_completion_serialization_is_independent_of_transport_delivery() {
    let receipt = InstanceRuntimeCommandResult {
        instance_id: "fixture".into(),
        process_key: "main".into(),
        display_name: "fixture".into(),
        pid: 1,
        command: "save".into(),
        response_text: None,
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Unconfirmed),
        submitted_at_unix_ms: 1,
    };
    let value = serde_json::to_value(&receipt).unwrap();
    assert_eq!(value["write_confirmation_pending"], false);
    assert_eq!(value["save_completion"], "unconfirmed");
    let non_save = InstanceRuntimeCommandResult {
        save_completion: None,
        ..receipt
    };
    assert!(
        serde_json::to_value(non_save)
            .unwrap()
            .get("save_completion")
            .is_none()
    );
}

fn descriptor(id: &str) -> ModuleDescriptor {
    app_modules::discover_modules(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules"),
    )
    .unwrap()
    .into_iter()
    .find(|module| module.summary.id == id)
    .unwrap()
}

#[test]
fn native_save_adapters_require_exact_game_actions_without_player_or_transport_overrides() {
    for (transport, id) in [
        ("satisfactory_https", "satisfactory"),
        ("astroneer_save", "astroneer"),
        ("valheim_native", "valheim"),
        ("dragonwilds_save", "runescapedragonwilds"),
        ("corekeeper_save", "corekeeper"),
        ("vrising_save", "vrising"),
        ("sonsoftheforest_save", "sonsoftheforest"),
        ("abioticfactor_save", "abioticfactor"),
        ("returntomoria_save", "returntomoria"),
        ("windrose_save", "windrose"),
        ("enshrouded_save", "enshrouded"),
        ("rimworld_save", "rimworld"),
        ("barotrauma_save", "barotrauma"),
        ("soulmask_save", "soulmask"),
        ("nightingale_save", "nightingale"),
        ("conanexiles_save", "conanexiles"),
        ("scum_save", "scum"),
    ] {
        let kind = NativeSave::from_transport(transport).unwrap();
        let module = descriptor(id);
        let action = module
            .runtime
            .player_actions
            .iter()
            .find(|action| action.id == "save_world")
            .unwrap();
        kind.validate_action(id, action).unwrap();
        assert!(kind.validate_action("other-game", action).is_err());
        for change in 0..8 {
            let mut invalid = action.clone();
            match change {
                0 => invalid.id = "raw".into(),
                1 => invalid.command_template = "shutdown".into(),
                2 => invalid.target_required = true,
                3 => invalid.destructive = true,
                4 => invalid.role_values.push("admin".into()),
                5 => invalid.process_key = Some("other".into()),
                6 => invalid.port_name = Some("other".into()),
                7 => invalid.enabled_setting_key = Some("other".into()),
                _ => unreachable!(),
            }
            assert!(
                kind.validate_action(id, &invalid).is_err(),
                "{id}: {change}"
            );
        }
    }
}

#[test]
fn scum_current_token_launch_can_deliver_the_instance_bound_native_session() {
    let module = descriptor("scum");
    assert!(!module.runtime.requires_admin);
    assert_eq!(
        module
            .process
            .as_ref()
            .unwrap()
            .environment_template
            .get("__COMPAT_LAYER")
            .map(String::as_str),
        Some("RunAsInvoker")
    );
    let action = module
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == "save_world")
        .unwrap();
    assert_eq!(action.command_template, "save_persistent_state");
    assert_eq!(action.label, "Save persistent state");
}

#[test]
fn native_save_resolution_uses_the_module_contract_and_rejects_targeted_or_changed_actions() {
    use super::super::commands_runtime_actions::resolve_declared_runtime_action;
    for id in [
        "satisfactory",
        "astroneer",
        "valheim",
        "runescapedragonwilds",
        "corekeeper",
        "vrising",
        "sonsoftheforest",
        "abioticfactor",
        "returntomoria",
        "windrose",
        "enshrouded",
        "rimworld",
        "barotrauma",
        "soulmask",
        "nightingale",
        "conanexiles",
        "palworld",
        "scum",
        "humanitz",
        "unturned",
        "minecraft",
        "romestead",
        "necesse",
        "terraria",
    ] {
        let mut module = descriptor(id);
        let resolved =
            resolve_declared_runtime_action(&module, "save_world", None, None, None, false)
                .unwrap();
        validate_action(
            id,
            module
                .runtime
                .player_actions
                .iter()
                .find(|a| a.id == "save_world")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(resolved.runtime_action_id.as_deref(), Some("save_world"));
        assert!(
            resolve_declared_runtime_action(
                &module,
                "save_world",
                Some("player"),
                None,
                None,
                false
            )
            .is_err()
        );
        assert!(
            resolve_declared_runtime_action(
                &module,
                "save_world",
                None,
                Some("admin"),
                None,
                false
            )
            .is_err()
        );
        let action = module
            .runtime
            .player_actions
            .iter_mut()
            .find(|action| action.id == "save_world")
            .unwrap();
        action.command_template = "quit".into();
        assert!(
            resolve_declared_runtime_action(&module, "save_world", None, None, None, false)
                .is_err()
        );
    }
}

#[test]
fn existing_native_commands_never_resolve_to_retired_save_observers() {
    for (module_id, old_transport) in [
        ("minecraft", "minecraft_save"),
        ("projectzomboid", "projectzomboid_save"),
        ("terraria", "terraria_save"),
        ("unturned", "unturned_save"),
        ("necesse", "necesse_save"),
        ("romestead", "romestead_save"),
        ("sevendaystodie", "sevendaystodie_save"),
        ("palworld", "palworld_save"),
        ("humanitz", "humanitz_save"),
        ("arksurvivalevolved", "ase_save"),
        ("arksurvivalascended", "asa_save"),
    ] {
        let module = descriptor(module_id);
        let mut action = module
            .runtime
            .player_actions
            .iter()
            .find(|a| a.id == "save_world")
            .unwrap()
            .clone();
        validate_action(module_id, &action).unwrap();
        assert!(NativeSave::from_transport(&action.transport).is_none());
        action.transport = old_transport.into();
        assert!(validate_action(module_id, &action).is_err());
    }
}
