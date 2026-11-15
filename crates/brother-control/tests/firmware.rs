use brother_control::{
    firmware::{Comparison, Component, ComponentVersion, Version, compare, latest_components},
    types::{Failure, Model},
};

#[test]
fn firmware_comparison_requires_the_same_component_and_numeric_version_contract()
-> Result<(), Failure> {
    let current = ComponentVersion {
        component: Component::Main,
        version: Version::parse("1.13")?,
    };
    let latest = ComponentVersion {
        component: Component::Main,
        version: Version::parse("1.14")?,
    };
    assert!(matches!(
        compare(&current, &latest),
        Comparison::UpdateAvailable
    ));
    assert!(matches!(compare(&latest, &latest), Comparison::Current));
    let network = ComponentVersion {
        component: Component::Network,
        version: Version::parse("1.13")?,
    };
    assert!(matches!(
        compare(&network, &latest),
        Comparison::Unknown { .. }
    ));
    for input in ["", "A", "1.14-beta", "1..14", "1.65536"] {
        assert!(Version::parse(input).is_err());
    }
    Ok(())
}

#[test]
fn the_official_catalog_uses_the_latest_history_entry_and_component_labels() -> Result<(), Failure>
{
    let html = r#"<title>MFC-L2860DW | Brother</title><dl id="pane1"><dt>2026/10/15 <nobr>メイン：1.14</nobr> / <nobr>Sub1：1.11</nobr></dt><dt>2026/06/11 <nobr>メイン：1.13</nobr></dt></dl>"#;
    let components = latest_components(Model::MfcL2860dw, html)?;
    assert_eq!(components.len(), 2);
    assert_eq!(components[0].component, Component::Main);
    assert_eq!(components[0].version.to_string(), "1.14");
    assert!(latest_components(Model::HlL2375dw, html).is_err());
    assert!(latest_components(Model::MfcL2860dw, &html.replace("メイン：", "Unknown：")).is_err());
    Ok(())
}

#[test]
fn firmware_links_stay_within_the_official_product_catalog() -> Result<(), Failure> {
    use brother_control::firmware::firmware_page;
    let link = r#"<a href="/g/b/downloadend.aspx?c=jp&amp;lang=ja&amp;prod=mfcl2860dw&amp;dlid=fixture&amp;type3=375">Firmware Update Tool</a>"#;
    assert_eq!(
        firmware_page(Model::MfcL2860dw, link)?.host_str(),
        Some("support.brother.com")
    );
    assert!(firmware_page(Model::HlL2375dw, link).is_err());
    assert!(
        firmware_page(
            Model::MfcL2860dw,
            &link.replace(
                "/g/b/downloadend.aspx",
                "https://attacker.invalid/g/b/downloadend.aspx"
            )
        )
        .is_err()
    );
    Ok(())
}
