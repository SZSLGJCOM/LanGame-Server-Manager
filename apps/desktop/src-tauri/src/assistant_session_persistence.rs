use super::*;
use serde::{Deserialize, Serialize};

const CHECKPOINT_BYTES: usize = 512 * 1024;
const ARCHIVE_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantConversationSummary {
    conversation_id: String,
    revision: u64,
    title: String,
    updated_at_unix_ms: u64,
}

#[derive(Serialize)]
pub(crate) struct AssistantPublicMessage {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDocument {
    version: u32,
    id: String,
    binding_digest: String,
    revision: u64,
    saved_unix_ms: u64,
    cancelled: bool,
    history: Vec<ArchivedMessage>,
    user_requests: Vec<String>,
    pinned_context: Option<String>,
    checkpoint: Option<Value>,
}

// Opaque provider envelopes may contain reasoning or credentials. Reconstruct
// native messages from public content and calls after a restart, never replay
// persisted provider payloads or recover authorization from assistant text.
#[derive(Serialize, Deserialize)]
enum ArchivedMessage {
    User(String),
    Assistant {
        content: String,
        calls: Vec<ArchivedCall>,
    },
    ToolResult {
        call_id: String,
        name: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedCall {
    id: String,
    name: String,
    arguments: Value,
}

pub(super) fn binding_digest(binding: &AssistantSessionBinding) -> Result<String, String> {
    Ok(app_storage::assistant_session_binding_digest(&encoded(
        binding,
    )?))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn redacted(text: &str) -> String {
    crate::assistant::redact_assistant_provider_text(text)
}

fn redacted_value(value: &Value) -> Result<Value, String> {
    serde_json::from_str(&redacted(&value.to_string()))
        .map_err(|_| "Assistant archive evidence could not be redacted safely.".into())
}

impl AssistantSessionStore {
    /// Explicitly initialized at the async command boundary. Pure in-memory
    /// stores remain available to isolated protocol tests.
    pub(crate) async fn restore(&self, binding: &AssistantSessionBinding) -> Result<(), String> {
        let _gate = self.restore_gate.lock().await;
        let digest = binding_digest(binding)?;
        if self
            .archives
            .lock()
            .map_err(|_| unavailable())?
            .contains_key(&digest)
        {
            return Ok(());
        }
        let archive = Arc::new(app_storage::AssistantSessionArchive::for_database(
            std::path::Path::new(&binding.storage_identity[4]),
        )?);
        let records = archive.load().await?;
        self.load_errors.lock().map_err(|_| unavailable())?.clear();
        let mut restored = Vec::new();
        for (id, bytes) in records {
            let document: SessionDocument = match serde_json::from_slice(&bytes) {
                Ok(document) => document,
                Err(_) => {
                    self.load_errors.lock().map_err(|_| unavailable())?.insert(id, "This assistant conversation archive is damaged; no task or operation was restored. Delete this conversation or start a new one.".into());
                    continue;
                }
            };
            if document.binding_digest != digest {
                continue;
            }
            let session = restore_document(&id, document, binding.clone(), archive.clone());
            match session {
                Ok(session) => restored.push(session),
                Err(error) => {
                    self.load_errors
                        .lock()
                        .map_err(|_| unavailable())?
                        .insert(id, error);
                }
            }
        }
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let incoming = restored
            .iter()
            .filter(|session| !sessions.contains_key(&session.id))
            .count();
        let needed = sessions
            .len()
            .saturating_add(incoming)
            .saturating_sub(SESSION_LIMIT);
        let mut evictable = Vec::new();
        for (id, session) in sessions
            .iter()
            .filter(|(_, session)| &session.binding != binding)
        {
            let state = session.lock()?;
            if !state.busy {
                evictable.push((id.clone(), state.last_used));
            }
        }
        evictable.sort_by_key(|(_, used)| *used);
        if evictable.len() < needed {
            return Err("Active conversations occupy the session capacity; finish one before restoring another provider or storage context.".into());
        }
        for (id, _) in evictable.into_iter().take(needed) {
            if let Some(session) = sessions.remove(&id) {
                session.invalidate()?;
                self.archives
                    .lock()
                    .map_err(|_| unavailable())?
                    .remove(&binding_digest(&session.binding)?);
            }
        }
        for session in restored {
            sessions
                .entry(session.id.clone())
                .or_insert_with(|| Arc::new(session));
        }
        let mut archives = self.archives.lock().map_err(|_| unavailable())?;
        if archives.len() >= SESSION_LIMIT {
            archives.clear();
        }
        archives.insert(digest, archive);
        Ok(())
    }

    pub(crate) fn get_bound(
        &self,
        id: &str,
        binding: &AssistantSessionBinding,
    ) -> Result<Option<Arc<AssistantSession>>, String> {
        Ok(self
            .sessions
            .lock()
            .map_err(|_| unavailable())?
            .get(id)
            .filter(|session| &session.binding == binding)
            .cloned())
    }

    pub(crate) fn list_bound(
        &self,
        binding: &AssistantSessionBinding,
    ) -> Result<Vec<AssistantConversationSummary>, String> {
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let mut summaries = Vec::new();
        for session in sessions
            .values()
            .filter(|session| &session.binding == binding)
        {
            let state = session.lock()?;
            let age = state.last_used.elapsed();
            if !state.busy && age >= session.retention() {
                continue;
            }
            summaries.push(AssistantConversationSummary {
                conversation_id: session.id.clone(),
                revision: session.revision(),
                title: state
                    .user_requests
                    .first()
                    .map(|text| redacted(text).chars().take(96).collect())
                    .unwrap_or_else(|| "New conversation".into()),
                updated_at_unix_ms: now_ms()
                    .saturating_sub(age.as_millis().min(u128::from(u64::MAX)) as u64),
            });
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at_unix_ms));
        Ok(summaries)
    }

    pub(crate) async fn cancel_persisted(
        &self,
        id: &str,
        database: &std::path::Path,
    ) -> Result<bool, String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| unavailable())?
            .get(id)
            .cloned();
        if let Some(session) = session {
            let busy = self.cancel(id)?;
            session.flush().await?;
            return Ok(busy);
        }
        let archive = app_storage::AssistantSessionArchive::for_database(database)?;
        if let Some((_, bytes)) = archive
            .load()
            .await?
            .into_iter()
            .find(|(saved_id, _)| saved_id == id)
        {
            let mut document: SessionDocument = serde_json::from_slice(&bytes).map_err(
                |_| "The assistant archive is damaged; delete the conversation to remove it.",
            )?;
            document.cancelled = true;
            document.checkpoint = None;
            document.saved_unix_ms = now_ms();
            archive.write(id.to_owned(), encoded(&document)?).await?;
        }
        Ok(false)
    }

    pub(crate) async fn delete_persisted(
        &self,
        id: &str,
        database: &std::path::Path,
    ) -> Result<bool, String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| unavailable())?
            .get(id)
            .cloned();
        let busy = if let Some(session) = session {
            let busy = self.remove(id)?;
            session.flush().await?;
            busy
        } else {
            app_storage::AssistantSessionArchive::for_database(database)?
                .remove(id.to_owned())
                .await?;
            false
        };
        self.load_errors
            .lock()
            .map_err(|_| unavailable())?
            .remove(id);
        Ok(busy)
    }
}

impl AssistantSession {
    pub(crate) fn public_messages(&self) -> Result<(Vec<AssistantPublicMessage>, bool), String> {
        let state = self.lock()?;
        let requests: Vec<_> = state
            .user_requests
            .iter()
            .map(|text| redacted(text.trim()))
            .collect();
        let mut request_index = 0;
        let mut messages = Vec::new();
        for message in &state.history {
            match message {
                AssistantToolMessage::User(text)
                    if requests
                        .get(request_index)
                        .is_some_and(|request| request == &redacted(text.trim())) =>
                {
                    messages.push(AssistantPublicMessage {
                        role: "user".into(),
                        content: requests[request_index].clone(),
                    });
                    request_index += 1;
                }
                AssistantToolMessage::Assistant(reply) if !reply.content.trim().is_empty() => {
                    messages.push(AssistantPublicMessage {
                        role: "assistant".into(),
                        content: redacted(&reply.content),
                    });
                }
                AssistantToolMessage::ToolResult { name, content, .. }
                    if matches!(name.as_str(), "confirmed_operation" | "operation_error") =>
                {
                    if let Ok(receipt) = serde_json::from_str::<Value>(content) {
                        let data = &receipt["data"];
                        let text = data["message"].as_str().or_else(|| data["error"].as_str());
                        if let Some(text) = text {
                            let mut content = redacted(text);
                            if let Some(summary) = data["verification"]["summary"].as_str() {
                                content.push('\n');
                                content.push_str(&redacted(summary));
                            }
                            messages.push(AssistantPublicMessage {
                                role: "assistant".into(),
                                content,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        for request in requests.into_iter().skip(request_index) {
            messages.push(AssistantPublicMessage {
                role: "user".into(),
                content: request,
            });
        }
        let mut truncated = false;
        for message in &mut messages {
            if message.content.len() > 8192 {
                message
                    .content
                    .truncate(message.content.floor_char_boundary(8192));
                truncated = true;
            }
        }
        let mut start = messages.len().saturating_sub(120);
        let mut bytes = messages[start..]
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
        while bytes > 64 * 1024 && start < messages.len() {
            bytes -= messages[start].content.len();
            start += 1;
        }
        truncated |= start > 0;
        Ok((messages.into_iter().skip(start).collect(), truncated))
    }

    pub(crate) fn checkpoint(&self) -> Result<Option<Value>, String> {
        self.check_active()?;
        Ok(self.lock()?.checkpoint.clone())
    }

    pub(crate) fn set_checkpoint(&self, checkpoint: Option<Value>) -> Result<(), String> {
        let mut state = self.lock()?;
        self.check_writable(&state)?;
        if encoded(&checkpoint)?.len() > CHECKPOINT_BYTES {
            return Err("Assistant task checkpoint exceeds its bounded archive.".into());
        }
        state.checkpoint = checkpoint;
        Ok(())
    }

    pub(crate) fn needs_recovery(&self) -> bool {
        self.recovered.load(Ordering::SeqCst)
    }

    pub(crate) fn recovered(&self) {
        self.recovered.store(false, Ordering::SeqCst);
    }

    pub(crate) async fn flush(&self) -> Result<(), String> {
        let Some(archive) = &self.archive else {
            return Ok(());
        };
        let _disk = self.disk_access.lock().await;
        if self.removed.load(Ordering::SeqCst) {
            return archive.remove(self.id.clone()).await;
        }
        let bytes = {
            let state = self.lock()?;
            let history = state
                .history
                .iter()
                .map(ArchivedMessage::from_message)
                .collect::<Result<Vec<_>, _>>()?;
            let document = SessionDocument {
                version: ARCHIVE_VERSION,
                id: self.id.clone(),
                binding_digest: binding_digest(&self.binding)?,
                revision: self.revision(),
                saved_unix_ms: now_ms(),
                cancelled: self.cancelled.load(Ordering::SeqCst),
                history,
                user_requests: state
                    .user_requests
                    .iter()
                    .map(|request| redacted(request))
                    .collect(),
                pinned_context: state.pinned_context.as_deref().map(redacted),
                checkpoint: state.checkpoint.clone(),
            };
            encoded(&document)?
        };
        archive.write(self.id.clone(), bytes).await
    }
}

fn restore_document(
    id: &str,
    document: SessionDocument,
    binding: AssistantSessionBinding,
    archive: Arc<app_storage::AssistantSessionArchive>,
) -> Result<AssistantSession, String> {
    if document.version != ARCHIVE_VERSION
        || document.id != id
        || document.revision == u64::MAX
        || document.user_requests.len() > USER_REQUEST_LIMIT
        || now_ms().saturating_sub(document.saved_unix_ms) >= ARCHIVE_TTL.as_millis() as u64
        || encoded(&document.checkpoint)?.len() > CHECKPOINT_BYTES
    {
        return Err("Assistant conversation archive is expired or has an unsupported format; no task was restored.".into());
    }
    let mut history: Vec<_> = document
        .history
        .into_iter()
        .map(ArchivedMessage::into_message)
        .collect();
    let (_, pending) = message_groups(&history)?;
    if pending {
        let mut calls = std::collections::VecDeque::new();
        for message in &history {
            match message {
                AssistantToolMessage::Assistant(reply) => calls.extend(reply.calls.iter().cloned()),
                AssistantToolMessage::ToolResult { .. } => {
                    calls.pop_front();
                }
                AssistantToolMessage::User(_) => {}
            }
        }
        for call in calls {
            history.push(AssistantToolMessage::ToolResult {
                call_id: call.id, name: call.name, is_error: true,
                content: json!({"ok":false,"outcome":"unknown_after_restart","error":"The application stopped before recording this tool result. Do not replay a mutation; inspect current state and request a new confirmation."}).to_string(),
            });
        }
    }
    check_capacity(
        &history,
        &document.user_requests,
        document.pinned_context.as_deref(),
    )?;
    let window_start =
        history::select_completed_window(id, &history, document.pinned_context.as_deref())?;
    let age = Duration::from_millis(now_ms().saturating_sub(document.saved_unix_ms));
    let progress = crate::assistant::AssistantSessionProgress::default();
    progress.begin(document.revision)?;
    if document.cancelled {
        progress.cancel();
    }
    Ok(AssistantSession {
        id: id.to_owned(),
        binding,
        state: Mutex::new(SessionState {
            busy: false,
            last_used: Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
            history,
            window_start,
            user_requests: document.user_requests,
            pinned_context: document.pinned_context,
            checkpoint: if document.cancelled {
                None
            } else {
                document.checkpoint
            },
        }),
        revision: AtomicU64::new(document.revision),
        cancelled: AtomicBool::new(document.cancelled),
        removed: AtomicBool::new(false),
        cancellation_epoch: AtomicU64::new(0),
        changed: Notify::new(),
        archive: Some(archive),
        disk_access: tokio::sync::Mutex::new(()),
        progress,
        recovered: AtomicBool::new(true),
    })
}

impl ArchivedMessage {
    fn from_message(message: &AssistantToolMessage) -> Result<Self, String> {
        Ok(match message {
            AssistantToolMessage::User(text) => Self::User(redacted(text)),
            AssistantToolMessage::Assistant(reply) => Self::Assistant {
                content: redacted(&reply.content),
                calls: reply
                    .calls
                    .iter()
                    .map(|call| {
                        Ok(ArchivedCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            arguments: redacted_value(&call.arguments)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
            },
            AssistantToolMessage::ToolResult {
                call_id,
                name,
                content,
                is_error,
            } => Self::ToolResult {
                call_id: call_id.clone(),
                name: name.clone(),
                content: redacted(content),
                is_error: *is_error,
            },
        })
    }

    fn into_message(self) -> AssistantToolMessage {
        match self {
            Self::User(text) => AssistantToolMessage::User(text),
            Self::Assistant { content, calls } => {
                AssistantToolMessage::Assistant(crate::assistant::AssistantToolReply {
                    content,
                    calls: calls
                        .into_iter()
                        .map(|call| crate::assistant::AssistantToolCall {
                            id: call.id,
                            name: call.name,
                            arguments: call.arguments,
                        })
                        .collect(),
                    raw_message: Value::Null,
                })
            }
            Self::ToolResult {
                call_id,
                name,
                content,
                is_error,
            } => AssistantToolMessage::ToolResult {
                call_id,
                name,
                content,
                is_error,
            },
        }
    }
}

#[cfg(test)]
#[path = "assistant_session_persistence_tests.rs"]
mod tests;
