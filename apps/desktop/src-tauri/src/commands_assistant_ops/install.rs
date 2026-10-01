pub(super) async fn execute_assistant_install_action(
    state: tauri::State<'_, DesktopState>,
    module_id: &str,
    action: AssistantOperationAction,
) -> Result<app_steamcmd::ModuleInstallResult, String> {
    match action {
        AssistantOperationAction::InstallServer => {
            install_module_game(state, module_id.to_owned()).await
        }
        AssistantOperationAction::ValidateServer => {
            validate_module_game(state, module_id.to_owned()).await
        }
        _ => Err(String::from(
            "The operation is not a server file installation or validation.",
        )),
    }
}
