use super::*;

fn managed_body(status: &str, code: &str) -> String {
    format!(
        "OpenHuman API error ({status}): {{\"error\":{{\"errorCode\":\"{code}\",\"message\":\"x\"}}}}"
    )
}

#[path = "observability_crash_filter_integrations_tests.rs"]
mod crash_filter_integrations;
#[path = "observability_crash_filter_messages_tests.rs"]
mod crash_filter_messages;
#[path = "observability_error_classification_core_tests.rs"]
mod error_classification_core;
#[path = "observability_error_classification_network_tests.rs"]
mod error_classification_network;
#[path = "observability_error_classification_provider_tests.rs"]
mod error_classification_provider;
#[path = "observability_error_classification_session_tests.rs"]
mod error_classification_session;
#[path = "observability_error_classification_user_state_tests.rs"]
mod error_classification_user_state;
