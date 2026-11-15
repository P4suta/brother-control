use brother_control::{
    config::ConfigStore,
    mcp::{Request, execute},
    types::{Failure, FailureKind},
};

#[test]
fn mcp_rejects_unknown_operations_and_credentials_in_normal_requests() -> Result<(), Failure> {
    for input in [
        r#"{"operation":"factory_reset"}"#,
        r#"{"operation":"authorize_credentials"}"#,
        r#"{"operation":"settings_list","password":"fixture-secret"}"#,
        r#"{"operation":"status","host":"https://printer.local"}"#,
    ] {
        assert!(serde_json::from_str::<Request>(input).is_err());
    }
    let request: Request = serde_json::from_str(r#"{"operation":"settings_list"}"#)
        .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid fixture request"))?;
    let result = execute(
        &ConfigStore::new(std::env::temp_dir().join("unused-brother-profile")),
        request,
        || false,
    )?;
    let structured = result.structured_content.ok_or_else(|| {
        Failure::new(
            FailureKind::UnknownResponse,
            "missing structured MCP result",
        )
    })?;
    assert_eq!(structured["operation"], "settings_list");
    assert_eq!(
        structured["result"]["settings"].as_array().map(Vec::len),
        Some(6)
    );
    assert_eq!(result.is_error, Some(false));
    Ok(())
}

#[test]
fn authenticated_operations_use_the_management_deadline_by_default() -> Result<(), Failure> {
    let request: Request = serde_json::from_str(
        r#"{"operation":"settings_get","device":"11111111-1111-4111-8111-111111111111"}"#,
    )
    .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid management fixture"))?;
    assert!(matches!(
        request,
        Request::SettingsGet {
            timeout_ms: 15000,
            ..
        }
    ));
    Ok(())
}

#[test]
fn firmware_check_is_available_without_accepting_update_requests() {
    assert!(
        serde_json::from_str::<Request>(r#"{"operation":"firmware_check","host":"printer.local"}"#)
            .is_ok()
    );
    assert!(
        serde_json::from_str::<Request>(
            r#"{"operation":"firmware_update","host":"printer.local"}"#
        )
        .is_err()
    );
}
#[test]
fn reboot_returns_the_explicit_unsupported_result() -> Result<(), String> {
    let request: brother_control::mcp::Request = serde_json::from_str(
        r#"{"operation":"reboot","device":"11111111-1111-4111-8111-111111111111"}"#,
    )
    .map_err(|error| error.to_string())?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = brother_control::config::ConfigStore::new(directory.path().to_path_buf());
    let failure = brother_control::mcp::execute(&store, request, || false)
        .err()
        .ok_or_else(|| "reboot unexpectedly succeeded".to_owned())?;
    assert_eq!(
        failure.kind,
        brother_control::types::FailureKind::Unsupported
    );
    Ok(())
}
