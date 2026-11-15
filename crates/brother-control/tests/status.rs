use brother_control::{
    ipp_adapter::Endpoint,
    service,
    types::{Failure, Host, Reading, Source},
};
use std::time::Duration;

#[test]
fn unreachable_status_preserves_both_protocol_failures() -> Result<(), Failure> {
    let endpoint = Endpoint::new(Host::parse("127.0.0.1")?, 1, "/ipp/print")?;
    let report = service::status(&endpoint, Duration::from_millis(10))?.result;
    assert_eq!(report.ipp.source, Source::Ipp);
    assert!(matches!(report.ipp.reading, Reading::Failed { .. }));
    assert_eq!(report.snmp.source, Source::Snmp);
    assert!(matches!(report.snmp.reading, Reading::Known { .. }));
    if let Reading::Known { value } = report.snmp.reading {
        assert!(matches!(value.description, Reading::Failed { .. }));
        assert!(value.supply_failure.is_some());
    }
    Ok(())
}
