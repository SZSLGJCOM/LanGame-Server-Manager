//! Public website authentication observations; no account data or persistent truth.
use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::{BrowserState, error};

pub(super) const SCRIPT: &str = include_str!("nexus_browser_auth.js");

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthState {
    #[default]
    Unknown,
    SignedOut,
    SignedIn,
}

impl AuthState {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::SignedOut => "signed_out",
            Self::SignedIn => "signed_in",
        }
    }
}

#[tauri::command]
pub(crate) fn read_nexus_website_auth_state(app: tauri::AppHandle) -> Result<AuthState, String> {
    app.state::<BrowserState>()
        .auth_state
        .lock()
        .map(|state| *state)
        .map_err(|_| error("nexus_browser_unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nexus_auth_cache_is_process_local_and_signed_out_replaces_signed_in() {
        let state = BrowserState::default();
        assert_eq!(*state.auth_state.lock().unwrap(), AuthState::Unknown);
        state.remember_auth(AuthState::SignedIn).unwrap();
        assert_eq!(*state.auth_state.lock().unwrap(), AuthState::SignedIn);
        state.remember_auth(AuthState::SignedOut).unwrap();
        assert_eq!(*state.auth_state.lock().unwrap(), AuthState::SignedOut);
        state.remember_auth(AuthState::Unknown).unwrap();
        assert_eq!(*state.auth_state.lock().unwrap(), AuthState::Unknown);
        assert_eq!(
            *BrowserState::default().auth_state.lock().unwrap(),
            AuthState::Unknown
        );
    }

    #[test]
    fn nexus_auth_observation_accepts_only_the_fixed_enum_or_absence() {
        for state in [
            AuthState::Unknown,
            AuthState::SignedOut,
            AuthState::SignedIn,
        ] {
            assert_eq!(
                serde_json::from_str::<Option<AuthState>>(&format!("\"{}\"", state.name()))
                    .unwrap(),
                Some(state)
            );
        }
        assert_eq!(
            serde_json::from_str::<Option<AuthState>>("null").unwrap(),
            None
        );
        for invalid in ["true", "{}", "\"username\"", "\"SIGNED_IN\""] {
            assert!(serde_json::from_str::<Option<AuthState>>(invalid).is_err());
        }
    }
}
