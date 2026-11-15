use brother_control::{
    config::Fingerprint,
    types::{Failure, FailureKind},
    web::{certificate_info, login_form},
};

fn invalid(_: impl std::fmt::Display) -> Failure {
    Failure::new(FailureKind::InvalidInput, "invalid web fixture")
}

#[test]
fn certificates_require_a_dns_identity_and_preserve_the_exact_fingerprint() -> Result<(), Failure> {
    let certified = rcgen::generate_simple_self_signed(Vec::from(["printer.local".to_owned()]))
        .map_err(invalid)?;
    let certificate = certified.cert.der();
    let info = certificate_info(certificate.as_ref())?;
    assert_eq!(
        info.tls_host.as_ref().map(|host| host.as_str()),
        Some("printer.local")
    );
    assert_eq!(info.sha256, Fingerprint::of(certificate.as_ref()));
    assert!(certificate_info(b"untrusted malformed certificate").is_err());
    Ok(())
}

#[test]
fn legacy_certificate_inspection_does_not_invent_a_dns_identity() -> Result<(), Failure> {
    let certified = rcgen::generate_simple_self_signed(Vec::<String>::new()).map_err(invalid)?;
    let info = certificate_info(certified.cert.der().as_ref())?;
    assert!(info.tls_host.is_none());
    assert_eq!(info.sha256, Fingerprint::of(certified.cert.der().as_ref()));
    Ok(())
}

#[test]
fn login_uses_the_observed_password_field_and_refuses_unknown_forms() -> Result<(), Failure> {
    let form = login_form(
        r#"<title>Brother MFC-L2860DW</title><form method="post" action="/home/status.html"><input type="password" id="LogBox" name="B10ed"><input type="hidden" name="loginurl" value="/home/status.html"></form>"#,
    )?;
    assert_eq!(form.password_field(), "B10ed");
    assert!(login_form(r#"<title>Brother HL-L2375DW series</title><form method="post" action="/general/status.html"><input type="password" id="LogBox" name="Babc"><input type="hidden" id="csrf" name="csrf" value="token"></form>"#).is_ok());
    for document in [
        "<title>Unknown device</title>",
        r#"<form method="post" action="https://attacker.invalid/"><input type="password" name="password"></form>"#,
        r#"<form method="post" action="/home/status.html"><input type="password" name="a"><input type="password" name="b"></form>"#,
    ] {
        assert!(login_form(document).is_err());
    }
    Ok(())
}

#[test]
fn login_navigation_accepts_relative_redirects_only_within_the_trusted_origin()
-> Result<(), Failure> {
    use brother_control::web::navigation_url;
    let origin = "https://printer.local:443/home/status.html";
    assert_eq!(
        navigation_url(origin, "../home/status.html")?.as_str(),
        "https://printer.local/home/status.html"
    );
    assert_eq!(
        navigation_url(origin, "/admin/password.html")?.path(),
        "/admin/password.html"
    );
    for location in [
        "https://attacker.invalid/",
        "http://printer.local/",
        "https://printer.local:8443/",
        "//attacker.invalid/",
    ] {
        assert!(navigation_url(origin, location).is_err());
    }
    Ok(())
}
