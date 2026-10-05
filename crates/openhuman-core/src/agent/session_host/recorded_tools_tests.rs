use super::*;

fn spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("{name} description"),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "to": { "type": "string" } }
        }),
    }
}

fn integration(
    toolkit: &str,
    connected: bool,
    gated_tools: Vec<crate::agent::prompts::GatedIntegrationTool>,
) -> crate::agent::prompts::ConnectedIntegration {
    crate::agent::prompts::ConnectedIntegration {
        toolkit: toolkit.into(),
        description: String::new(),
        tools: Vec::new(),
        gated_tools,
        connected,
        connections: Vec::new(),
        non_active_status: None,
    }
}

#[test]
fn only_composio_slugs_count_as_integration_actions() {
    assert!(is_integration_action_name("GMAIL_SEND_EMAIL"));
    assert!(is_integration_action_name("GOOGLECALENDAR_CREATE_EVENT"));
    assert!(!is_integration_action_name("memory_recall"));
    assert!(!is_integration_action_name("tool_search"));
    assert!(!is_integration_action_name("GMAIL"));
    assert!(!is_integration_action_name("_SEND"));
}

#[test]
fn recorded_actions_are_filtered_from_a_mixed_declaration_set() {
    let recorded = vec![
        spec("web_fetch"),
        spec("GMAIL_SEND_EMAIL"),
        spec("research"),
    ];
    let actions = recorded_integration_actions(&recorded);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].name, "GMAIL_SEND_EMAIL");
}

#[test]
fn unavailable_authorization_does_not_rebuild_recorded_actions() {
    let recorded = vec![spec("GMAIL_SEND_EMAIL"), spec("GMAIL_FETCH_EMAILS")];
    let rebuilt = rehydrate_integration_actions(&recorded, &[], &[], false);
    assert!(rebuilt.is_empty());
}

#[test]
fn non_integration_declarations_are_never_rehydrated() {
    let recorded = vec![spec("web_fetch"), spec("GMAIL_SEND_EMAIL")];
    let integrations = vec![
        integration("web", true, Vec::new()),
        integration("gmail", true, Vec::new()),
    ];

    let rebuilt = rehydrate_integration_actions(&recorded, &[], &integrations, true);
    let names: Vec<&str> = rebuilt.iter().map(|tool| tool.name()).collect();
    assert_eq!(names, vec!["GMAIL_SEND_EMAIL"]);
}

#[test]
fn a_live_action_is_not_rebuilt_from_the_record() {
    let recorded = vec![spec("GMAIL_SEND_EMAIL"), spec("SLACK_SEND_MESSAGE")];
    let integrations = vec![
        integration("gmail", true, Vec::new()),
        integration("slack", true, Vec::new()),
    ];
    let live: Vec<Box<dyn Tool>> =
        rehydrate_integration_actions(&[spec("GMAIL_SEND_EMAIL")], &[], &integrations, true);
    let rebuilt = rehydrate_integration_actions(&recorded, &live, &integrations, true);
    let names: Vec<&str> = rebuilt.iter().map(|tool| tool.name()).collect();
    assert_eq!(names, vec!["SLACK_SEND_MESSAGE"]);
}

#[test]
fn a_rebuilt_declaration_is_byte_identical_to_the_recorded_one() {
    let recorded = vec![spec("GMAIL_SEND_EMAIL")];
    let integrations = vec![integration("gmail", true, Vec::new())];
    let first = rehydrate_integration_actions(&recorded, &[], &integrations, true);
    let second = rehydrate_integration_actions(&recorded, &[], &integrations, true);
    let mut rebuilt = first[0].spec();
    rebuilt
        .parameters
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .expect("rebuilt properties")
        .remove("connection_id");
    assert_eq!(
        serde_json::to_string(&rebuilt).unwrap(),
        serde_json::to_string(&recorded[0]).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&first[0].spec()).unwrap(),
        serde_json::to_string(&second[0].spec()).unwrap()
    );
}

#[test]
fn authoritative_integrations_do_not_restore_revoked_or_gated_actions() {
    let recorded = vec![spec("GMAIL_SEND_EMAIL"), spec("SLACK_SEND_MESSAGE")];
    let integrations = vec![
        integration(
            "gmail",
            true,
            vec![crate::agent::prompts::GatedIntegrationTool {
                name: "GMAIL_SEND_EMAIL".into(),
                description: String::new(),
                required_scope: "write".into(),
                unlock_paths: Vec::new(),
            }],
        ),
        integration("slack", false, Vec::new()),
    ];

    let rebuilt = rehydrate_integration_actions(&recorded, &[], &integrations, true);
    assert!(rebuilt.is_empty());
}

#[test]
fn recorded_search_tools_keep_only_role_tools() {
    let recorded = vec![
        spec("web_search_tool"),
        spec("GMAIL_SEND_EMAIL"),
        spec("web_answer_tool"),
        spec("parallel_search"),
        spec("web_fetch"),
    ];
    let names: Vec<String> = recorded_search_tools(&recorded)
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    assert_eq!(names, vec!["web_search_tool", "web_answer_tool"]);
}

#[cfg(feature = "modules")]
#[test]
fn missing_search_tools_are_rebuilt_once_with_their_recorded_declaration() {
    let recorded = vec![
        spec("web_answer_tool"),
        spec("web_answer_tool"),
        spec("web_search_tool"),
    ];
    let live: Vec<Box<dyn Tool>> = vec![Box::new(crate::search::TinySearchTool::recorded(
        tinysearch_bus::ToolSpec {
            name: "web_search_tool".into(),
            description: "live".into(),
            parameters: serde_json::json!({"type": "object"}),
        },
    ))];
    let rebuilt = rehydrate_search_tools(&recorded, &[live.as_slice()], "test-agent");
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].name(), "web_answer_tool");
    assert_eq!(rebuilt[0].description(), "web_answer_tool description");
}

#[test]
fn stubs_only_recorded_declarations_without_an_executor() {
    let snapshot = vec![
        spec("wallet_status"),
        spec("web_fetch"),
        spec("never_recorded"),
        spec("wallet_status"),
    ];
    let recorded = HashSet::from(["wallet_status".to_string(), "web_fetch".to_string()]);
    let executable = HashSet::from(["web_fetch".to_string()]);
    let (stubs, missing) = stub_unavailable_recorded_tools(&snapshot, &recorded, &executable);
    let names = stubs.iter().map(|tool| tool.name()).collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["wallet_status"],
        "live tools win; duplicates collapse"
    );
    assert_eq!(missing, vec!["never_recorded".to_string()]);
}

#[test]
fn stub_keeps_recorded_declaration_verbatim() {
    let recorded = spec("web3_swap_execute");
    let stub = UnavailableRecordedTool::new(recorded.clone());
    let declared = stub.spec();
    assert_eq!(declared.name, recorded.name);
    assert_eq!(declared.description, recorded.description);
    assert_eq!(
        declared.parameters, recorded.parameters,
        "prefix cache needs the same declaration"
    );
    assert_eq!(stub.permission_level(), tinytools::PermissionLevel::None);
}

#[tokio::test]
async fn stub_call_returns_a_readable_tool_error() {
    let stub = UnavailableRecordedTool::new(spec("wallet_status"));
    let result = stub
        .execute(serde_json::json!({}))
        .await
        .expect("a refusal is a tool result, not a failed turn");
    assert!(result.is_error);
    let text = serde_json::to_string(&result.content).expect("content serializes");
    assert!(text.contains("wallet_status"), "{text}");
    assert!(text.contains("not available in this build"), "{text}");
}
