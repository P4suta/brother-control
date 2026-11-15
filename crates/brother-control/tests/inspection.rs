use brother_control::{
    inspection::structure,
    types::{Failure, FailureKind},
};

#[test]
fn management_structure_never_exports_input_values_or_hidden_tokens() -> Result<(), Failure> {
    let page = structure(
        r#"<h2>Ecology</h2><form action="/general/ecology.html" method="post"><input type="hidden" name="csrf" value="fixture-hidden-token"><input type="password" name="password" value="fixture-password"><dl><dt>Quiet Mode</dt><dd><select name="B123"><option value="private-wire-token" selected>Off</option></select></dd></dl></form>"#,
    )?;
    let output = serde_json::to_string(&page)
        .map_err(|_| Failure::new(FailureKind::Storage, "fixture encoding failed"))?;
    for secret in [
        "fixture-hidden-token",
        "fixture-password",
        "private-wire-token",
    ] {
        assert!(!output.contains(secret));
    }
    assert!(output.contains("Quiet Mode"));
    assert!(output.contains("B123"));
    assert!(output.contains("adjacent_controls"));
    Ok(())
}
