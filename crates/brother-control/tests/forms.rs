use brother_control::{
    types::{Failure, Model},
    web_settings::parse_page,
};
use brother_core::{Minutes, Setting, Switch};

const ECOLOGY: &str = r#"<title>Brother MFC-L2860DW</title>
<form method="post" action="/general/ecology.html">
<input type="hidden" name="csrf" value="fixture-token">
<dl><dt>Sleep Time</dt><dd><input type="text" name="B123" value="10"></dd>
<dt>Toner Save</dt><dd><select name="B456"><option value="0" selected>Off</option><option value="1">On</option></select></dd>
<dt>Quiet Mode</dt><dd><label><input type="radio" name="B789" value="0" checked>Off</label><label><input type="radio" name="B789" value="1">On</label></dd></dl>
<input type="submit" value="Submit"></form>"#;

#[test]
fn ordinary_changes_do_not_resubmit_the_eco_mode_preset() -> Result<(), Failure> {
    let document = ECOLOGY.replace("<dl>", r#"<dl><dt>Eco Mode</dt><dd><label><input type="radio" name="preset" value="0" checked>Off</label><label><input type="radio" name="preset" value="1">On</label></dd>"#);
    let page = parse_page(Model::MfcL2860dw, "/general/ecology.html", &document)?;
    let submission = page.submission(Setting::TonerSave(Switch::On).into())?;
    assert!(!submission.fields().iter().any(|(name, _)| name == "preset"));
    assert!(submission.fields().iter().any(|(name, _)| name == "csrf"));
    Ok(())
}

#[test]
fn known_forms_preserve_tokens_and_other_values_when_changing_one_setting() -> Result<(), Failure> {
    let page = parse_page(Model::MfcL2860dw, "/general/ecology.html", ECOLOGY)?;
    assert_eq!(page.values().len(), 3);
    let submission = page.submission(Setting::TonerSave(Switch::On).into())?;
    assert_eq!(submission.path(), "/general/ecology.html");
    let fields = submission.fields();
    assert!(
        fields
            .iter()
            .any(|(name, value)| name == "csrf" && value == "fixture-token")
    );
    assert!(
        fields
            .iter()
            .any(|(name, value)| name == "B123" && value == "10")
    );
    assert!(
        fields
            .iter()
            .any(|(name, value)| name == "B456" && value == "1")
    );
    let sleep = page.submission(
        Setting::Sleep(Minutes::new(3).map_err(|_| {
            brother_control::types::Failure::new(
                brother_control::types::FailureKind::InvalidInput,
                "fixture range",
            )
        })?)
        .into(),
    )?;
    assert!(
        sleep
            .fields()
            .iter()
            .any(|(name, value)| name == "B123" && value == "3")
    );
    Ok(())
}

#[test]
fn unknown_forms_values_and_model_changes_fail_closed() {
    assert!(parse_page(Model::HlL2375dw, "/general/ecology.html", ECOLOGY).is_err());
    assert!(
        parse_page(
            Model::MfcL2860dw,
            "/general/ecology.html",
            &ECOLOGY.replace(
                "action=\"/general/ecology.html\"",
                "action=\"https://attacker.invalid/\""
            )
        )
        .is_err()
    );
    assert!(
        parse_page(
            Model::MfcL2860dw,
            "/general/ecology.html",
            &ECOLOGY.replace(">Off<", ">Maybe<")
        )
        .is_err()
    );
    assert!(parse_page(Model::MfcL2860dw, "/net/ipv4.html", ECOLOGY).is_err());
}

#[test]
fn tray_settings_select_the_tray_control_in_nested_labels() -> Result<(), Failure> {
    let document = r#"<title>Brother MFC-L2860DW</title><form action="/print/tray.html" method="post">
    <dl><dt>Paper Type</dt><dd><dl><dt>Tray 1</dt><dd><select name="tray_type"><option value="0" selected>Plain Paper</option><option value="1">Recycled Paper</option></select></dd></dl></dd>
    <dt>Paper Size</dt><dd><dl><dt>Tray 1</dt><dd><select name="tray_size"><option value="a4" selected>A4</option><option value="b5">B5(JIS)</option></select></dd></dl></dd>
    <dt>Media Type</dt><dd><select name="manual_type"><option value="0" selected>Plain Paper</option></select></dd></dl></form>"#;
    let page = parse_page(Model::MfcL2860dw, "/print/tray.html", document)?;
    assert_eq!(page.values().len(), 2);
    let submission = page.submission(Setting::PaperSize(brother_core::PaperSize::B5).into())?;
    assert!(
        submission
            .fields()
            .iter()
            .any(|(name, value)| name == "tray_size" && value == "b5")
    );
    Ok(())
}

#[test]
fn tray_group_headers_bind_only_the_following_tray() -> Result<(), Failure> {
    let document = r#"<title>Brother MFC-L2860DW</title><form action="/print/tray.html" method="post"><dl>
    <dt>Paper Type</dt><dt>Tray 1</dt><dd><select name="tray_type"><option value="0" selected>Plain Paper</option></select></dd>
    <dt>Paper Size</dt><dt>Tray 1</dt><dd><select name="tray_size"><option value="a4" selected>A4</option></select></dd>
    <dt>Media Type</dt><dd><select name="manual_type"><option value="0" selected>Envelope</option></select></dd></dl></form>"#;
    let page = parse_page(Model::MfcL2860dw, "/print/tray.html", document)?;
    assert_eq!(page.values().len(), 2);
    assert!(
        page.values()
            .contains(&Setting::PaperType(brother_core::PaperType::Plain).into())
    );
    let legacy = document
        .replace("MFC-L2860DW", "HL-L2375DW series")
        .replace("Tray 1", "Tray #1");
    assert_eq!(
        parse_page(Model::HlL2375dw, "/print/tray.html", &legacy)?
            .values()
            .len(),
        2
    );
    Ok(())
}

#[test]
fn navigation_discovers_settings_pages_without_following_network_or_external_links()
-> Result<(), Failure> {
    let links = brother_control::web_settings::settings_links(
        r#"<a href="../general/ecology.html">Ecology</a><a href="/printer/print.html">Printer</a><a href="/net/ipv4.html">Network</a><a href="https://attacker.invalid/general/test.html">External</a>"#,
    )?;
    assert_eq!(
        links,
        Vec::from([
            "/general/ecology.html".to_owned(),
            "/printer/print.html".to_owned()
        ])
    );
    assert_eq!(
        brother_control::web_settings::settings_links(r#"<a href="/print/print.html">Print</a>"#)?,
        Vec::from(["/print/print.html".to_owned()])
    );
    assert!(
        brother_control::web_settings::settings_links(
            r#"<a href="/print/pcl.html">HP LaserJet</a><a href="/general/information.html">Maintenance Information</a><a href="/print/ecomodesettings.html">Eco Mode Settings</a>"#,
        )?
        .is_empty()
    );
    Ok(())
}
