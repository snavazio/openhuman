use super::*;
use std::sync::Mutex;

/// A fake core: records every call and serves a configurable auth state.
struct FakeCore {
    calls: Mutex<Vec<(String, Value)>>,
    state: Mutex<Value>,
}

impl FakeCore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            state: Mutex::new(json!({ "isAuthenticated": false })),
        })
    }
    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl CoreLink for FakeCore {
    async fn invoke(&self, method: &str, params: Value) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push((method.to_string(), params.clone()));
        match method {
            AUTH_SET_CREDENTIAL => {
                let user = params.get("user").cloned().unwrap_or(Value::Null);
                *self.state.lock().unwrap() = json!({
                    "isAuthenticated": true,
                    "credential": "local",
                    "userId": "local",
                    "user": user,
                });
                Ok(json!({ "result": { "isAuthenticated": true } }))
            }
            AUTH_CLEAR_CREDENTIAL => {
                *self.state.lock().unwrap() = json!({ "isAuthenticated": false });
                Ok(json!({ "result": {} }))
            }
            // Wrapped the way the real core wraps results.
            AUTH_GET_STATE => Ok(json!({ "result": self.state.lock().unwrap().clone(), "logs": [] })),
            other => Err(format!("unexpected method {other}")),
        }
    }
}

const LOCAL_TOKEN: &str = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJsb2NhbCJ9.local";

fn local_user() -> Value {
    json!({ "_id": "local", "id": "local", "name": "Local User" })
}

#[tokio::test]
async fn stores_a_local_session_and_emits_changed() {
    let core = FakeCore::new();
    let manager = LocalSessionManager::new(Arc::clone(&core));
    let mut events = manager.subscribe();

    let state = manager
        .store_session_token(LOCAL_TOKEN, Some(local_user()))
        .await
        .expect("local session must be accepted");

    assert!(state.core.is_authenticated);
    assert_eq!(state.core.credential.as_deref(), Some("local"));
    assert_eq!(state.current_user, Some(local_user()));
    assert!(!state.current_user_stale);

    let (method, params) = &core.calls()[0];
    assert_eq!(method, AUTH_SET_CREDENTIAL);
    assert_eq!(params["kind"], "local");
    assert_eq!(params["token"], LOCAL_TOKEN);

    match events.recv().await.expect("a changed event") {
        SessionEvent::Changed(s) => assert!(s.core.is_authenticated),
        other => panic!("unexpected event {other:?}"),
    }
}

#[tokio::test]
async fn refuses_a_cloud_session_jwt_without_calling_the_core() {
    let core = FakeCore::new();
    let manager = LocalSessionManager::new(Arc::clone(&core));
    let err = manager
        .store_session_token("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1In0.c2lnbmF0dXJl", Some(local_user()))
        .await
        .expect_err("a JWT must be refused");
    assert!(err.to_string().starts_with("BACKEND: "), "{err}");
    assert!(core.calls().is_empty());
}

#[tokio::test]
async fn requires_a_token_and_a_user_payload() {
    let manager = LocalSessionManager::new(FakeCore::new());
    let empty = manager.store_session_token("  ", Some(local_user())).await;
    assert!(empty.unwrap_err().to_string().starts_with("INVALID: "));
    let no_user = manager.store_session_token(LOCAL_TOKEN, None).await;
    assert!(no_user.unwrap_err().to_string().starts_with("INVALID: "));
    let empty_user = manager.store_session_token(LOCAL_TOKEN, Some(json!({}))).await;
    assert!(empty_user.unwrap_err().to_string().starts_with("INVALID: "));
}

#[tokio::test]
async fn login_with_token_is_disabled() {
    let core = FakeCore::new();
    let manager = LocalSessionManager::new(Arc::clone(&core));
    let err = manager.login_with_token("one-time-token").await.unwrap_err();
    assert_eq!(err, SessionError::CloudDisabled(CLOUD_SIGN_IN_DISABLED.to_string()));
    assert!(core.calls().is_empty());
}

#[tokio::test]
async fn logout_clears_and_reports_signed_out() {
    let core = FakeCore::new();
    let manager = LocalSessionManager::new(Arc::clone(&core));
    manager
        .store_session_token(LOCAL_TOKEN, Some(local_user()))
        .await
        .unwrap();
    let state = manager.logout().await.unwrap();
    assert!(!state.core.is_authenticated);
    assert_eq!(state.current_user, None);
    assert!(core.calls().iter().any(|(m, _)| m == AUTH_CLEAR_CREDENTIAL));
}

#[tokio::test]
async fn current_user_is_the_stored_payload_and_never_stale() {
    let manager = LocalSessionManager::new(FakeCore::new());
    let before = manager.current_user(true).await.unwrap();
    assert_eq!(before.user, None);
    manager
        .store_session_token(LOCAL_TOKEN, Some(local_user()))
        .await
        .unwrap();
    let after = manager.current_user(true).await.unwrap();
    assert_eq!(after.user, Some(local_user()));
    assert!(!after.stale);
    assert_eq!(after.stale_seconds, None);
}

#[test]
fn session_state_serialises_with_flattened_core_fields() {
    let state = SessionState {
        core: CoreAuthState {
            is_authenticated: true,
            credential: Some("local".into()),
            ..Default::default()
        },
        current_user: Some(local_user()),
        ..Default::default()
    };
    let v = serde_json::to_value(&state).unwrap();
    assert_eq!(v["isAuthenticated"], true);
    assert_eq!(v["credential"], "local");
    assert_eq!(v["currentUser"]["id"], "local");
    assert_eq!(v["currentUserStale"], false);
    let ev = serde_json::to_value(SessionEvent::Expired { source: "x".into() }).unwrap();
    assert_eq!(ev["type"], "expired");
}
