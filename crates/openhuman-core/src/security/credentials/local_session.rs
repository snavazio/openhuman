//! LOCAL-ONLY FORK: the host-side session owner.
//!
//! Replaces `openhuman-tinyhumans`' `SessionManager` for the desktop shell and
//! the TUI. It keeps the same wire shapes the renderer already consumes
//! ([`SessionState`], [`CachedUser`], [`SessionEvent`]) and the same stable
//! `PREFIX:` error strings ([`SessionError`]), but there is no backend: only
//! the offline local session token (`header.payload.local`) is accepted, and
//! the current user is whatever the core stored with it. Cloud sign-in (login
//! token exchange, session JWTs, API keys) is refused with a `BACKEND:` error.
//!
//! The host reaches its core through [`CoreLink`]: JSON-RPC over loopback for
//! the desktop shell, an in-process `CoreRuntime` for the TUI.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::security::credentials::session_support::is_local_session_token;

const LOG_PREFIX: &str = "[session][local]";

pub const AUTH_SET_CREDENTIAL: &str = "openhuman.auth_set_credential";
pub const AUTH_CLEAR_CREDENTIAL: &str = "openhuman.auth_clear_credential";
pub const AUTH_GET_STATE: &str = "openhuman.auth_get_state";

/// Message returned for any attempt to sign in to a cloud account.
pub const CLOUD_SIGN_IN_DISABLED: &str = "cloud sign-in is disabled in this local-only build";

/// An RPC channel into one core. `invoke` returns the JSON-RPC `result`
/// (possibly wrapped in the core's `{result, logs}` envelope) or the error.
#[async_trait]
pub trait CoreLink: Send + Sync + 'static {
    async fn invoke(&self, method: &str, params: Value) -> Result<Value, String>;
}

/// Why a session operation did not complete. The `Display` form carries a
/// stable `PREFIX:` the renderer classifies on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// Cloud sign-in was attempted; this build has no backend.
    #[error("BACKEND: {0}")]
    CloudDisabled(String),
    /// The core refused or failed the RPC.
    #[error("CORE: {0}")]
    Core(String),
    /// The request itself was malformed.
    #[error("INVALID: {0}")]
    Invalid(String),
}

/// What the core reports about the credential it holds (`auth.get_state`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreAuthState {
    #[serde(default)]
    pub is_authenticated: bool,
    /// `"session"`, `"api-key"`, `"local"`, or absent when signed out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub user: Option<Value>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// The current user. Never stale: there is no backend to refresh from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedUser {
    pub user: Option<Value>,
    pub stale: bool,
    pub stale_seconds: Option<u64>,
}

/// Everything a UI needs to render the signed-in state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    #[serde(flatten)]
    pub core: CoreAuthState,
    pub current_user: Option<Value>,
    pub current_user_stale: bool,
    pub current_user_stale_seconds: Option<u64>,
}

/// Emitted to the renderer when the credential changes. `Expired` is kept for
/// wire compatibility; nothing in a local-only build produces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionEvent {
    Changed(SessionState),
    Expired { source: String },
}

/// Peel the core's `{result, logs}` / `{data}` envelopes off an RPC result.
pub fn unwrap_envelope(mut value: &Value) -> &Value {
    loop {
        match value.get("result").or_else(|| value.get("data")) {
            Some(next) => value = next,
            None => return value,
        }
    }
}

/// The one session owner for a host process.
pub struct LocalSessionManager<L: CoreLink> {
    link: Arc<L>,
    events: broadcast::Sender<SessionEvent>,
    /// Serialises store / logout so two calls cannot interleave.
    mutation: tokio::sync::Mutex<()>,
}

impl<L: CoreLink> LocalSessionManager<L> {
    pub fn new(link: Arc<L>) -> Arc<Self> {
        let (events, _) = broadcast::channel(32);
        Arc::new(Self {
            link,
            events,
            mutation: tokio::sync::Mutex::new(()),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.events.subscribe()
    }

    /// Cloud login-token exchange: not available in a local-only build.
    pub async fn login_with_token(&self, _token: &str) -> Result<SessionState, SessionError> {
        log::info!("{LOG_PREFIX} refused login-token exchange: {CLOUD_SIGN_IN_DISABLED}");
        Err(SessionError::CloudDisabled(
            CLOUD_SIGN_IN_DISABLED.to_string(),
        ))
    }

    /// Install the offline local session token with its user payload. Any
    /// other credential (session JWT, API key) is refused.
    pub async fn store_session_token(
        &self,
        token: &str,
        user: Option<Value>,
    ) -> Result<SessionState, SessionError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(SessionError::Invalid("token is required".to_string()));
        }
        if !is_local_session_token(token) {
            log::info!("{LOG_PREFIX} refused non-local credential: {CLOUD_SIGN_IN_DISABLED}");
            return Err(SessionError::CloudDisabled(
                CLOUD_SIGN_IN_DISABLED.to_string(),
            ));
        }
        let user = user
            .filter(|u| u.as_object().is_some_and(|m| !m.is_empty()))
            .ok_or_else(|| {
                SessionError::Invalid("local session requires a user payload".to_string())
            })?;
        {
            let _guard = self.mutation.lock().await;
            let params = json!({ "token": token, "kind": "local", "user": user });
            log::debug!("{LOG_PREFIX} {AUTH_SET_CREDENTIAL} kind=local");
            self.link
                .invoke(AUTH_SET_CREDENTIAL, params)
                .await
                .map_err(SessionError::Core)?;
        }
        self.changed().await
    }

    /// Sign out: clear every credential the core holds.
    pub async fn logout(&self) -> Result<SessionState, SessionError> {
        {
            let _guard = self.mutation.lock().await;
            log::debug!("{LOG_PREFIX} {AUTH_CLEAR_CREDENTIAL} kind=all");
            self.link
                .invoke(AUTH_CLEAR_CREDENTIAL, json!({}))
                .await
                .map_err(SessionError::Core)?;
        }
        self.changed().await
    }

    /// The core's own view of the credential it holds.
    pub async fn core_state(&self) -> Result<CoreAuthState, SessionError> {
        let value = self
            .link
            .invoke(AUTH_GET_STATE, json!({}))
            .await
            .map_err(SessionError::Core)?;
        serde_json::from_value(unwrap_envelope(&value).clone()).map_err(|e| {
            SessionError::Core(format!(
                "{AUTH_GET_STATE} returned an unexpected shape: {e}"
            ))
        })
    }

    /// The current user: the payload stored with the local session.
    /// `_force` is accepted for API compatibility; there is nothing to refresh.
    pub async fn current_user(&self, _force: bool) -> Result<CachedUser, SessionError> {
        let core = self.core_state().await?;
        Ok(CachedUser {
            user: core.is_authenticated.then(|| core.user.clone()).flatten(),
            stale: false,
            stale_seconds: None,
        })
    }

    /// Core state plus the current user, in one shape.
    pub async fn state(&self) -> Result<SessionState, SessionError> {
        let core = self.core_state().await?;
        let current_user = core.is_authenticated.then(|| core.user.clone()).flatten();
        Ok(SessionState {
            core,
            current_user,
            current_user_stale: false,
            current_user_stale_seconds: None,
        })
    }

    async fn changed(&self) -> Result<SessionState, SessionError> {
        let state = self.state().await?;
        // No subscribers is fine (e.g. the TUI never listens).
        let _ = self.events.send(SessionEvent::Changed(state.clone()));
        Ok(state)
    }
}

#[cfg(test)]
#[path = "local_session_tests.rs"]
mod tests;
