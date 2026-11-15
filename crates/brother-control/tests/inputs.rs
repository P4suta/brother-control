use brother_control::types::{Host, Identity, Model};

#[test]
fn host_and_identity_validate_untrusted_input() {
    for value in [
        "https://printer.local/",
        "user:secret@printer",
        "printer:443",
        "a\nb",
        "../printer",
        "",
    ] {
        assert!(Host::parse(value).is_err());
    }
    assert!(Host::parse("printer.local.").is_ok());
    assert!(Host::parse("192.0.2.1").is_ok());
    assert!(Host::parse("fd7a:115c:a1e0::1").is_ok());
    assert!(Identity::parse("00000000-0000-0000-0000-000000000000").is_err());
}

#[test]
fn models_match_only_the_approved_hardware() {
    assert_eq!(
        Model::parse("Brother HL-L2375DW series"),
        Ok(Model::HlL2375dw)
    );
    assert_eq!(Model::parse("Brother MFC-L2860DW"), Ok(Model::MfcL2860dw));
    assert!(Model::parse("Brother MFC-L2860DWE").is_err());
    assert!(Model::parse("Unknown printer").is_err());
}
