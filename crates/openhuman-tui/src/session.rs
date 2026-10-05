//! The TUI's session owner. LOCAL-ONLY FORK: the core's
//! [`LocalSessionManager`] over an in-process [`CoreRuntime`] — no backend, no
//! HTTP, no bearer. Only the offline local session is accepted.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use openhuman_core::core::runtime::CoreRuntime;
use openhuman_core::security::credentials::local_session::{CoreLink, LocalSessionManager};

/// `CoreLink` over `CoreRuntime::invoke`.
pub struct InProcessLink(pub Arc<CoreRuntime>);

#[async_trait]
impl CoreLink for InProcessLink {
    async fn invoke(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.0.invoke(method, params).await
    }
}

static MANAGER: OnceLock<Arc<LocalSessionManager<InProcessLink>>> = OnceLock::new();

/// The one session manager for this TUI process, bound to `runtime` on first
/// use. A TUI process hosts exactly one runtime, so later callers get the
/// same manager regardless of the handle they pass.
pub fn session_manager(runtime: &Arc<CoreRuntime>) -> Arc<LocalSessionManager<InProcessLink>> {
    Arc::clone(MANAGER.get_or_init(|| {
        LocalSessionManager::new(Arc::new(InProcessLink(Arc::clone(runtime))))
    }))
}
