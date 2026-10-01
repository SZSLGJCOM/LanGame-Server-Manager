use crate::assistant::{
    AssistantConnectionCheckInput, AssistantConnectionCheckOutput,
    cancel_assistant_connection_check, check_assistant_connection,
};

#[tauri::command]
pub async fn assistant_check_connection(
    input: AssistantConnectionCheckInput,
) -> Result<AssistantConnectionCheckOutput, String> {
    check_assistant_connection(&input).await
}

#[tauri::command]
pub fn assistant_cancel_connection_check(request_id: String) -> Result<bool, String> {
    cancel_assistant_connection_check(&request_id)
}
