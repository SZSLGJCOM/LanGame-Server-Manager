use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeSettings {
    pub auto_update: bool,
    pub interval_hours: u32,
}

impl Default for KnowledgeSettings {
    fn default() -> Self {
        Self {
            auto_update: true,
            interval_hours: 24,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub phase: String,
    pub module_id: Option<String>,
    pub source_id: Option<String>,
    pub completed: usize,
    pub total: usize,
    pub downloaded_bytes: u64,
    pub total_download_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub started_at: u64,
    pub finished_at: u64,
    pub changed_documents: usize,
    pub sources_succeeded: usize,
    pub sources_failed: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub revision: String,
    pub ready: bool,
    pub download_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub id: String,
    pub title: String,
    pub authority: String,
    pub kind: String,
    pub url: String,
    pub state: String,
    pub document_count: u64,
    pub chunk_count: u64,
    pub last_checked_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub module_id: String,
    pub scope: String,
    pub gaps: Vec<String>,
    pub sources: Vec<SourceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStatus {
    pub settings: KnowledgeSettings,
    pub model: ModelStatus,
    pub games: Vec<GameStatus>,
    pub last_run: Option<SyncReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub content_use: crate::ContentUse,
    pub url: String,
    pub title: String,
    pub authority: String,
    pub kind: String,
    pub source_id: String,
    pub retrieved_at: u64,
    pub content_sha256: String,
    pub source_state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEntry {
    pub id: String,
    pub citation_id: String,
    pub title: String,
    pub heading: String,
    pub snippet: String,
    pub offset_bytes: usize,
    pub source: Citation,
    pub semantic_score: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub module_id: String,
    pub game_scope: String,
    pub source_limitations: Vec<String>,
    pub query: String,
    pub entries: Vec<SearchEntry>,
    pub next_offset: Option<usize>,
    pub retrieval: &'static str,
    pub model: &'static str,
    pub evidence_notice: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPage {
    pub module_id: String,
    pub game_scope: String,
    pub source_limitations: Vec<String>,
    pub id: String,
    pub citation_id: String,
    pub title: String,
    pub body: String,
    pub offset_bytes: usize,
    pub next_offset_bytes: Option<usize>,
    pub total_bytes: usize,
    pub source: Citation,
    pub evidence_notice: &'static str,
}
