use super::*;

/// TAURI-RUST-CGE: skill-install fetch 4xx (a missing/renamed catalog
/// `SKILL.md`) is expected user-input state — the before_send net must drop
/// it, while a genuine 5xx remote failure and unrelated domains stay
/// reportable.

#[test]
fn message_failure_classifier_matches_canonical_status_phrases() {
    for msg in [
        "rpc.invoke_method failed: GET /teams failed (502 Bad Gateway)",
        "GET /teams/me/usage failed (503 Service Unavailable)",
        "downstream returned (504 Gateway Timeout): retry budget exhausted",
        "OpenHuman API error (520 <unknown status code>): cf",
        "POST /channels/telegram/typing failed (429 Too Many Requests)",
        "auth connect failed: 503 Service Unavailable",
    ] {
        assert!(
            is_transient_message_failure(msg),
            "{msg:?} must be classified as transient"
        );
    }
}

#[test]
fn message_failure_classifier_matches_transport_phrases() {
    for msg in [
        "integrations.get failed: composio/tools → operation timed out",
        "GET https://api.example.com → connection forcibly closed (os 10054)",
        "POST /v1/foo → tls handshake eof",
        "error sending request for url (https://api.example.com)",
    ] {
        assert!(
            is_transient_message_failure(msg),
            "{msg:?} must be classified as transient"
        );
    }
}

#[test]
fn message_failure_classifier_keeps_unrelated_messages() {
    for msg in [
        "rpc.invoke_method failed: schema validation error",
        "process 502 exited unexpectedly",
        "GET /teams failed (404 Not Found)",
        "GET /teams failed (500 Internal Server Error)",
        "unrelated error with port 5023",
        "",
    ] {
        assert!(
            !is_transient_message_failure(msg),
            "{msg:?} must not be classified as transient"
        );
    }
}

#[test]
fn report_error_or_expected_does_not_panic() {
    report_error_or_expected(
        "local ai is disabled",
        "rpc",
        "invoke_method",
        &[("method", "openhuman.inference_prompt")],
    );
    report_error_or_expected(
        "ollama API key not set",
        "agent",
        "provider_chat",
        &[("provider", "ollama")],
    );
    // #2079 / #2076 / #2202 — exercises the expected_error_kind
    // ProviderConfigRejection branch AND the report_expected_message
    // skip-log arm (the agent/web-channel re-report demotion path).
    report_error_or_expected(
        "agent.run_single failed: custom_openai API error (400 Bad Request): \
         The supported API model names are deepseek-v4-pro or deepseek-v4-flash, \
         but you passed reasoning-v1.",
        "agent",
        "native_chat",
        &[("provider", "custom_openai")],
    );
    report_error_or_expected(
        "custom_openai API error (400): invalid temperature: only 1 is allowed for this model",
        "web_channel",
        "run_chat_task",
        &[("provider", "custom_openai")],
    );
}

#[test]
fn is_insufficient_credits_message_matches_verbatim_cron_402() {
    // Verbatim TAURI-RUST-514 body as it reaches the cron `report_error`
    // call site (`domain=cron`, `operation=agent_job`): the message-level
    // matcher must catch it so the cron halt skips the leaking report.
    assert!(is_insufficient_credits_message(
        "openrouter API error (402 Payment Required): {\"error\":{\"message\":\"This \
         request requires more credits, or fewer max_tokens. You requested up to 65536 \
         tokens, but can only afford 5081.\"}}",
    ));
    assert!(is_insufficient_credits_message(
        "custom_openai API error (402 Payment Required): insufficient balance",
    ));
}

#[test]
fn is_insufficient_credits_message_requires_402_and_credit_phrase() {
    // A 402 without a credit phrase, and a credit phrase without a 402
    // status, must both stay reportable (could be a real defect).
    assert!(!is_insufficient_credits_message(
        "provider API error (402): some unrelated condition"
    ));
    assert!(!is_insufficient_credits_message(
        "provider API error (500): internal error, insufficient memory"
    ));
    // The status must be the 402, not a digit in the body.
    assert!(!is_insufficient_credits_message(
        "provider API error (400): can only afford 402 tokens"
    ));
    // codex P2: the status prefix "(402 Payment Required)" itself contains
    // the phrase "payment required". An unrelated body behind a real 402
    // must NOT be classified as insufficient-credits (else the cron halt
    // would suppress a genuine 402 defect). The credit signal must live in
    // the BODY, not the formatted status prefix.
    assert!(!is_insufficient_credits_message(
        "provider API error (402 Payment Required): some unrelated condition"
    ));
    // But a body that literally carries the credit signal still matches,
    // even when the status prefix also says "Payment Required".
    assert!(is_insufficient_credits_message(
        "provider API error (402 Payment Required): your account has insufficient balance"
    ));
}

#[test]
fn ollama_cloud_internal_500_reraise_routes_through_expected_path() {
    // TAURI-RUST-5MV — the actionable message the emit sites raise must
    // demote to `TransientUpstreamHttp` when re-reported at the agent / RPC
    // boundary (`provider_chat` → `report_error_or_expected`), so the
    // `domain=agent` half of the flood is suppressed too.
    let reraise = crate::inference::provider::ollama_cloud_internal_500_user_message(
        Some("minimax-m3:cloud"),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
    );
    assert_eq!(
        expected_error_kind(&reraise),
        Some(ExpectedErrorKind::TransientUpstreamHttp)
    );
}
