use super::*;
use app_storage::player_history::{PlayerHistoryPage, PlayerHistoryQuery};

#[tauri::command]
pub async fn read_instance_player_history(
    state: tauri::State<'_, DesktopState>,
    input: PlayerHistoryQuery,
) -> Result<PlayerHistoryPage, String> {
    let _operation = state.begin_storage_context_operation("player history query")?;
    if input.search.as_ref().is_some_and(|value| value.len() > 256)
        || input
            .kind
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "join" | "leave" | "chat"))
        || input.before_id.is_some_and(|value| value <= 0)
        || input
            .since_unix_ms
            .zip(input.until_unix_ms)
            .is_some_and(|(since, until)| since > until)
    {
        return Err(String::from("Invalid player history filter."));
    }
    let storage = state.bootstrap_storage()?;
    app_storage::player_history::query_player_history(&storage.paths, input)
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn collect_player_history(
    state: &tauri::State<'_, DesktopState>,
) -> Result<(), String> {
    let _operation = state.begin_storage_context_operation("player history collection")?;
    let storage = state.bootstrap_storage()?;
    app_storage::player_history::collect_player_history(&storage.paths)
        .await
        .map_err(|error| error.to_string())
}
