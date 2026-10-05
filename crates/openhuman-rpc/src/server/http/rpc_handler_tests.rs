use serde_json::json;

use super::rpc_handler;
use crate::server::testing::EnvVarGuard;
use openhuman_core::core::invoke::default_state;

#[tokio::test(flavor = "current_thread")]
async fn structured_rpc_error_envelope_passes_through_generic_dispatch() {
    // The transport layer must surface any controller-emitted
    // `StructuredRpcError` payload without inspecting the method name —
    // this is what makes the boundary domain-agnostic. We register a
    // throwaway method-name on a thread-scoped op and confirm the
    // wire-shape carries the `kind`/`thread_id` data verbatim.
    use axum::body::to_bytes;
    use axum::extract::State;
    use axum::Json;

    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let _env = EnvVarGuard::set_many(vec![(
        "OPENHUMAN_WORKSPACE",
        workspace.path().as_os_str().to_os_string(),
    )]);

    let stale_thread_request = crate::RpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(7),
        method: "openhuman.threads_generate_title".to_string(),
        params: json!({ "thread_id": "thread-ghost" }),
    };
    let response = rpc_handler(State(default_state()), Json(stale_thread_request)).await;
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("json response");
    assert_eq!(body["error"]["data"]["kind"], "ThreadNotFound");
    assert_eq!(body["error"]["data"]["thread_id"], "thread-ghost");
    // The structured-error message must be human-readable on the wire —
    // never the encoded sentinel envelope.
    let message = body["error"]["message"].as_str().expect("error message");
    assert!(
        !message.contains("__OPENHUMAN_STRUCTURED_RPC_ERROR_V1__"),
        "sentinel-encoded envelope leaked onto the wire: {message}"
    );
    assert!(message.contains("thread-ghost"));
}

