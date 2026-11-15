use std::process::Command;

#[test]
fn settings_exposes_the_complete_read_diff_apply_workflow() -> std::io::Result<()> {
    let help = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["settings", "--help"])
        .output()?;
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for command in ["list", "get", "export", "diff", "apply"] {
        assert!(text.contains(command));
    }
    let help = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["settings", "apply", "--help"])
        .output()?;
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--device"));
    Ok(())
}

#[test]
fn setup_requires_explicit_trust_before_saving_credentials() -> std::io::Result<()> {
    let help = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["setup", "--help"])
        .output()?;
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("--inspect-certificate"));
    assert!(text.contains("--trust-sha256"));
    assert!(text.contains("--credential-env"));
    assert!(text.contains("15000"));
    let incomplete = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["setup", "--host", "printer.local"])
        .output()?;
    assert!(!incomplete.status.success());
    Ok(())
}

#[test]
fn discovery_supports_bounded_subnets_and_explicit_hosts() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["discover", "--help"])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--subnet"));
    assert!(text.contains("--via-site"));
    assert!(text.contains("--host"));
    Ok(())
}

#[test]
fn help_describes_the_status_command_without_color() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .arg("--help")
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(text.contains("status"));
    assert!(text.contains("discover"));
    assert!(text.contains("--json"));
    assert!(!text.contains('\u{1b}'));
    Ok(())
}

#[test]
fn unknown_commands_fail() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .arg("missing-command")
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn firmware_check_exposes_a_read_only_host_operation() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["firmware", "check", "--help"])
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--host"));
    Ok(())
}

#[test]
fn native_authorization_is_an_explicit_interactive_command() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["credentials", "authorize", "--help"])
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--device"));
    Ok(())
}

#[test]
fn json_operation_failures_use_the_same_error_contract_as_mcp() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let output = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .arg("--json")
        .arg("--config-dir")
        .arg(directory.path())
        .args([
            "settings",
            "get",
            "--device",
            "11111111-1111-4111-8111-111111111111",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    assert!(!output.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    assert_eq!(result["error"]["kind"], "storage");
    assert!(output.stderr.is_empty());
    Ok(())
}
#[test]
fn reboot_reports_unsupported_without_requesting_credentials() -> Result<(), String> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args([
            "--json",
            "reboot",
            "--device",
            "11111111-1111-4111-8111-111111111111",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    assert!(!output.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    assert_eq!(report["error"]["kind"], "unsupported");
    assert!(output.stderr.is_empty());
    Ok(())
}
