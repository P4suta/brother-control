use std::process::Command;

#[test]
fn unknown_checks_fail_instead_of_succeeding_silently() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("missing-check")
        .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown check"));
    Ok(())
}

#[test]
fn invalid_domain_constructions_and_reused_capabilities_fail_to_compile() -> std::io::Result<()> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("compiler output");
    let output = Command::new("just")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .arg("compile-fail")
        .env("CARGO_TARGET_DIR", &target)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        target.join("compile-fail").is_dir(),
        "compile-fail output must follow CARGO_TARGET_DIR"
    );
    assert!(
        target.join("check-runner").is_dir(),
        "verification bootstrap output must follow CARGO_TARGET_DIR"
    );
    Ok(())
}
