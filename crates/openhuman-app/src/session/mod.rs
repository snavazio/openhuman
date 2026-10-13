//! The desktop session owner. LOCAL-ONLY FORK: there is no backend account;
//! the core's `LocalSessionManager` accepts only the offline local session.
//! This module gives it a link to the embedded (or gateway) core, exposes it
//! to the renderer as Tauri commands, and forwards its change events as Tauri
//! events. The core holds the credential (`auth.set_credential`).

pub(crate) mod commands;
mod link;

use std::sync::Arc;

use openhuman_core::security::credentials::local_session::{LocalSessionManager, SessionEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::core_process::CoreProcessHandle;
use crate::AppRuntime;

pub(crate) use link::HttpCoreLink;

/// Tauri event emitted whenever the credential or the current user changes.
/// Payload: `local_session::SessionState`.
pub const AUTH_CHANGED_EVENT: &str = "auth://changed";
/// Tauri event emitted when the backend rejected the stored credential and it
/// has been cleared. Payload: `{ source }`.
pub const AUTH_EXPIRED_EVENT: &str = "auth://expired";

/// Managed Tauri state: the one session manager for this shell process.
pub struct SessionHost {
    pub manager: Arc<LocalSessionManager<HttpCoreLink>>,
}

impl SessionHost {
    pub fn new(desktop: CoreProcessHandle) -> Self {
        let link = Arc::new(HttpCoreLink::new(desktop));
        Self {
            manager: LocalSessionManager::new(link),
        }
    }
}

/// Register the session host on the app and start forwarding its events to
/// the renderer.
pub fn install(app: &AppHandle<AppRuntime>, desktop: CoreProcessHandle) {
    let host = SessionHost::new(desktop);
    let mut events = host.manager.subscribe();
    app.manage(host);

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(SessionEvent::Changed(state)) => {
                    log::debug!(
                        "[session] auth changed authenticated={} credential={}",
                        state.core.is_authenticated,
                        state.core.credential.as_deref().unwrap_or("none")
                    );
                    if let Err(e) = handle.emit(AUTH_CHANGED_EVENT, state) {
                        log::debug!("[session] failed to emit {AUTH_CHANGED_EVENT}: {e}");
                    }
                }
                Ok(SessionEvent::Expired { source }) => {
                    log::warn!(
                        "[session] backend rejected the stored credential (source={source})"
                    );
                    if let Err(e) =
                        handle.emit(AUTH_EXPIRED_EVENT, serde_json::json!({ "source": source }))
                    {
                        log::debug!("[session] failed to emit {AUTH_EXPIRED_EVENT}: {e}");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::debug!("[session] event forwarder lagged by {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
