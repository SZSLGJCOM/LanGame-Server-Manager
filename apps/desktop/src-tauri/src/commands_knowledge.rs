use crate::knowledge_runtime::{KnowledgeRuntimeStatus, KnowledgeSyncJob};
use crate::state::DesktopState;
use app_knowledge::KnowledgeSettings;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSyncInput {
    pub module_id: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeCancelInput {
    pub job_id: String,
}

#[tauri::command]
pub async fn read_knowledge_status(
    state: tauri::State<'_, DesktopState>,
) -> Result<KnowledgeRuntimeStatus, String> {
    let _operation = state.begin_storage_context_operation("knowledge status read")?;
    state.knowledge.status().await
}

#[tauri::command]
pub async fn update_knowledge_settings(
    state: tauri::State<'_, DesktopState>,
    input: KnowledgeSettings,
) -> Result<(), String> {
    let _operation = state.begin_storage_context_operation("knowledge settings update")?;
    state.knowledge.update_settings(input).await
}

#[tauri::command]
pub async fn start_knowledge_sync(
    state: tauri::State<'_, DesktopState>,
    input: KnowledgeSyncInput,
) -> Result<KnowledgeSyncJob, String> {
    state
        .knowledge
        .start(&state, input.module_id, input.force)
        .await
}

#[tauri::command]
pub fn cancel_knowledge_sync(
    state: tauri::State<'_, DesktopState>,
    input: KnowledgeCancelInput,
) -> Result<bool, String> {
    state.knowledge.cancel(&input.job_id)
}
