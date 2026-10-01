use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::assistant::AssistantToolMessage;

#[path = "assistant_session_history.rs"]
mod history;
#[path = "assistant_session_persistence.rs"]
mod persistence;
use history::{
    active_messages, append, check_capacity, encoded, history_page, message_groups,
    select_pinned_window,
};
pub(crate) use persistence::{AssistantConversationSummary, AssistantPublicMessage};

const SESSION_LIMIT: usize = 16;
const IDLE_TTL: Duration = Duration::from_secs(8 * 60 * 60);
const ARCHIVE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const HISTORY_BYTES: usize = 2 * 1024 * 1024;
const HISTORY_MESSAGES: usize = 1024;
const WINDOW_BYTES: usize = 24 * 1024;
const WINDOW_MESSAGES: usize = 64;
const PAGE_BYTES: usize = 12 * 1024;
pub(crate) const USER_REQUEST_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssistantHistorySource {
    #[default]
    Messages,
    UserRequests,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssistantHistoryRequest {
    #[serde(default)]
    pub source: AssistantHistorySource,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "history_page_limit")]
    pub limit: usize,
    #[serde(default)]
    pub message_offset_bytes: usize,
}

fn history_page_limit() -> usize {
    4
}

impl Default for AssistantHistoryRequest {
    fn default() -> Self {
        Self {
            source: AssistantHistorySource::Messages,
            offset: 0,
            limit: history_page_limit(),
            message_offset_bytes: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AssistantSessionBinding {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub storage_identity: [String; 6],
}

#[derive(Debug, Default)]
pub(crate) struct AssistantSessionStore {
    sessions: Mutex<HashMap<String, Arc<AssistantSession>>>,
    archives: Mutex<HashMap<String, Arc<app_storage::AssistantSessionArchive>>>,
    restore_gate: tokio::sync::Mutex<()>,
    load_errors: Mutex<HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub(crate) struct AssistantSessionSnapshot {
    pub revision: u64,
    pub busy: bool,
}

#[derive(Debug)]
pub(crate) struct AssistantSession {
    id: String,
    binding: AssistantSessionBinding,
    state: Mutex<SessionState>,
    revision: AtomicU64,
    cancelled: AtomicBool,
    removed: AtomicBool,
    cancellation_epoch: AtomicU64,
    changed: Notify,
    archive: Option<Arc<app_storage::AssistantSessionArchive>>,
    disk_access: tokio::sync::Mutex<()>,
    pub(crate) progress: crate::assistant::AssistantSessionProgress,
    recovered: AtomicBool,
}

#[derive(Debug)]
struct SessionState {
    busy: bool,
    last_used: Instant,
    history: Vec<AssistantToolMessage>,
    window_start: usize,
    user_requests: Vec<String>,
    pinned_context: Option<String>,
    checkpoint: Option<Value>,
}

pub(crate) struct AssistantSessionLease {
    session: Arc<AssistantSession>,
}

impl AssistantSessionStore {
    pub(crate) fn inspect(
        &self,
        id: &str,
        binding: &AssistantSessionBinding,
    ) -> Result<Option<AssistantSessionSnapshot>, String> {
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let Some(session) = sessions.get(id) else {
            if let Some(error) = self.load_errors.lock().map_err(|_| unavailable())?.get(id) {
                return Err(error.clone());
            }
            return Ok(None);
        };
        if &session.binding != binding {
            return Ok(None);
        }
        let state = session.lock()?;
        if !state.busy && Instant::now().duration_since(state.last_used) >= session.retention() {
            drop(state);
            if let Some(expired) = sessions.remove(id) {
                expired.invalidate()?;
            }
            return Ok(None);
        }
        Ok(Some(AssistantSessionSnapshot {
            revision: session.revision(),
            busy: state.busy,
        }))
    }

    pub(crate) fn begin(
        &self,
        id: Option<&str>,
        binding: AssistantSessionBinding,
        new_user_turn: bool,
    ) -> Result<AssistantSessionLease, String> {
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let now = Instant::now();
        let mut expired = Vec::new();
        for (key, session) in sessions.iter() {
            let state = session.lock()?;
            if !state.busy && now.duration_since(state.last_used) >= session.retention() {
                expired.push(key.clone());
            }
        }
        for key in expired {
            if let Some(session) = sessions.remove(&key) {
                session.invalidate()?;
            }
        }
        let session = match id {
            Some(id) => {
                let session = sessions
                    .get(id)
                    .ok_or("Assistant session expired or was removed; start a new conversation.")?;
                if session.binding != binding {
                    return Err("Assistant session provider, model or storage changed; start a new conversation.".into());
                }
                Arc::clone(session)
            }
            None => {
                if sessions.len() >= SESSION_LIMIT {
                    let mut oldest = None;
                    for (key, session) in sessions.iter() {
                        let state = session.lock()?;
                        if !state.busy
                            && oldest
                                .as_ref()
                                .is_none_or(|(_, when)| state.last_used < *when)
                        {
                            oldest = Some((key.clone(), state.last_used));
                        }
                    }
                    let (key, _) = oldest.ok_or(
                        "Assistant sessions are busy; wait for an existing task to finish.",
                    )?;
                    if let Some(session) = sessions.remove(&key) {
                        session.invalidate()?;
                        if session.binding != binding {
                            self.archives
                                .lock()
                                .map_err(|_| unavailable())?
                                .remove(&persistence::binding_digest(&session.binding)?);
                        }
                    }
                }
                let id = uuid::Uuid::new_v4().simple().to_string();
                let archive = {
                    let archives = self.archives.lock().map_err(|_| unavailable())?;
                    let archive = archives
                        .get(&persistence::binding_digest(&binding)?)
                        .cloned();
                    if !archives.is_empty() && archive.is_none() {
                        return Err("Assistant provider or storage changed while opening its archive; retry in the current context.".into());
                    }
                    archive
                };
                let session = Arc::new(AssistantSession {
                    id: id.clone(),
                    binding,
                    state: Mutex::new(SessionState {
                        busy: false,
                        last_used: now,
                        history: Vec::new(),
                        window_start: 0,
                        user_requests: Vec::new(),
                        pinned_context: None,
                        checkpoint: None,
                    }),
                    revision: AtomicU64::new(0),
                    cancelled: AtomicBool::new(false),
                    removed: AtomicBool::new(false),
                    cancellation_epoch: AtomicU64::new(0),
                    changed: Notify::new(),
                    archive,
                    disk_access: tokio::sync::Mutex::new(()),
                    progress: crate::assistant::AssistantSessionProgress::default(),
                    recovered: AtomicBool::new(false),
                });
                sessions.insert(id, Arc::clone(&session));
                session
            }
        };
        {
            let mut state = session.lock()?;
            if state.busy {
                return Err("Assistant session already has an active task.".into());
            }
            if new_user_turn {
                let revision = session
                    .revision()
                    .checked_add(1)
                    .ok_or("Assistant session generation exhausted.")?;
                session.revision.store(revision, Ordering::SeqCst);
                session.cancelled.store(false, Ordering::SeqCst);
                state.checkpoint = None;
                session.recovered.store(false, Ordering::SeqCst);
                session.progress.begin(revision)?;
            }
            session.check_active()?;
            state.busy = true;
            state.last_used = now;
        }
        Ok(AssistantSessionLease { session })
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<bool, String> {
        let sessions = self.sessions.lock().map_err(|_| unavailable())?;
        match sessions.get(id) {
            Some(session) => session.cancel(),
            None => Ok(false),
        }
    }

    pub(crate) fn remove(&self, id: &str) -> Result<bool, String> {
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let Some(session) = sessions.get(id) else {
            return Ok(false);
        };
        let busy = session.invalidate()?;
        sessions.remove(id);
        Ok(busy)
    }
}

impl AssistantSessionLease {
    pub(crate) fn session(&self) -> Arc<AssistantSession> {
        Arc::clone(&self.session)
    }
}

impl Drop for AssistantSessionLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.session.state.lock() {
            state.busy = false;
            state.last_used = Instant::now();
        }
    }
}

impl AssistantSession {
    fn retention(&self) -> Duration {
        if self.archive.is_some() {
            ARCHIVE_TTL
        } else {
            IDLE_TTL
        }
    }
    fn lock(&self) -> Result<MutexGuard<'_, SessionState>, String> {
        self.state.lock().map_err(|_| unavailable())
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    pub(crate) fn record_progress(
        &self,
        revision: u64,
        kind: &str,
        text: &str,
        tool_name: Option<&str>,
    ) -> Result<(), String> {
        self.progress.record(revision, kind, text, tool_name)
    }

    pub(crate) fn check_active(&self) -> Result<(), String> {
        if self.removed.load(Ordering::SeqCst) {
            Err("Assistant session was removed; its evidence and confirmations are invalid.".into())
        } else if self.cancelled.load(Ordering::SeqCst) {
            Err("Assistant task was cancelled; completed operations have not been undone.".into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn cancel(&self) -> Result<bool, String> {
        let mut state = self.lock()?;
        state.checkpoint = None;
        state.last_used = Instant::now();
        self.signal_cancel();
        Ok(state.busy)
    }

    fn invalidate(&self) -> Result<bool, String> {
        let mut state = self.lock()?;
        self.removed.store(true, Ordering::SeqCst);
        self.signal_cancel();
        state.history.clear();
        state.user_requests.clear();
        state.pinned_context = None;
        state.window_start = 0;
        state.checkpoint = None;
        Ok(state.busy)
    }

    fn signal_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancellation_epoch.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
        self.progress.cancel();
    }

    pub(crate) async fn cancelled(&self) {
        let epoch = self.cancellation_epoch.load(Ordering::SeqCst);
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.check_active().is_err()
                || self.cancellation_epoch.load(Ordering::SeqCst) != epoch
            {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn messages(&self) -> Result<Vec<AssistantToolMessage>, String> {
        let state = self.lock()?;
        self.check_active()?;
        let (_, pending) = message_groups(&state.history)?;
        if pending {
            return Err("Assistant history has pending tool results; finish or cancel that tool batch before resuming.".into());
        }
        active_messages(&self.id, &state)
    }

    /// The caller supplies the current task context explicitly; internal user-
    /// role feedback must never displace its original constraints after archival.
    /// Reload messages() after changing this pin, before replacing a snapshot.
    pub(crate) fn pin_context(&self, context: &str) -> Result<(), String> {
        let mut state = self.lock()?;
        self.check_writable(&state)?;
        if context.trim().is_empty() || encoded(context)?.len() > WINDOW_BYTES - 1024 {
            return Err("Assistant task context is empty or exceeds its bounded active window; existing evidence was retained.".into());
        }
        check_capacity(&state.history, &state.user_requests, Some(context))?;
        let start = select_pinned_window(&self.id, &state.history, context)?;
        state.pinned_context = Some(context.to_string());
        state.window_start = start;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn append_messages(
        &self,
        messages: Vec<AssistantToolMessage>,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        self.check_writable(&state)?;
        append(&self.id, &mut state, messages, false)
    }

    /// Cancellation stops future actions, but cannot erase the observed result
    /// of a confirmed write that already crossed its commit boundary.
    pub(crate) fn append_completed_operation(
        &self,
        messages: Vec<AssistantToolMessage>,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        if self.removed.load(Ordering::SeqCst) || !state.busy {
            return Err("Completed operation evidence requires its live session lease.".into());
        }
        append(&self.id, &mut state, messages, true)
    }

    /// A window replacement may append to its exact prior snapshot, never erase
    /// history or overwrite a concurrently changed window.
    pub(crate) fn replace_messages(
        &self,
        messages: Vec<AssistantToolMessage>,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        self.check_writable(&state)?;
        let prefix = active_messages(&self.id, &state)?;
        if messages.len() < prefix.len() || encoded(&messages[..prefix.len()])? != encoded(&prefix)?
        {
            return Err("Assistant history replacement changed its captured window; reload the current session.".into());
        }
        append(
            &self.id,
            &mut state,
            messages.into_iter().skip(prefix.len()).collect(),
            false,
        )
    }

    fn check_writable(&self, state: &SessionState) -> Result<(), String> {
        self.check_active()?;
        if !state.busy {
            return Err("Assistant history may only change while its task lease is held.".into());
        }
        Ok(())
    }

    /// Only the command boundary registers external user requests. Model output
    /// and internal control messages must not become sources of authorization.
    pub(crate) fn register_user_request(&self, prompt: &str) -> Result<(), String> {
        let mut state = self.lock()?;
        self.check_writable(&state)?;
        if prompt.trim().is_empty() {
            return Err("Assistant user request is empty.".into());
        }
        if state.user_requests.len() >= USER_REQUEST_LIMIT {
            return Err("Assistant session has reached its user-request limit; start a new conversation without discarding this task's evidence.".into());
        }
        let mut requests = state.user_requests.clone();
        requests.push(prompt.to_owned());
        check_capacity(&state.history, &requests, state.pinned_context.as_deref())?;
        state.user_requests = requests;
        Ok(())
    }

    pub(crate) fn source_user_requests(&self) -> Result<Vec<String>, String> {
        let state = self.lock()?;
        self.check_active()?;
        Ok(state.user_requests.clone())
    }

    #[cfg(test)]
    pub(crate) fn prior_user_requests(&self) -> Result<Vec<String>, String> {
        let mut requests = self.source_user_requests()?;
        requests.pop();
        Ok(requests)
    }

    pub(crate) fn read_history(&self, request: AssistantHistoryRequest) -> Result<Value, String> {
        let state = self.lock()?;
        self.check_active()?;
        history_page(&self.id, self.revision(), &state, request)
    }
}

fn unavailable() -> String {
    "Assistant session state is unavailable.".into()
}

#[cfg(test)]
#[path = "assistant_sessions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "assistant_session_history_tests.rs"]
mod history_tests;
