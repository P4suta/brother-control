use brother_control::{
    config::ConfigStore,
    diagnose,
    ipp_adapter::Endpoint,
    types::{Failure, Host, Reading},
};
use std::time::Duration;

#[test]
fn diagnostics_keep_unreachable_protocols_and_missing_trust_distinct() -> Result<(), Failure> {
    let endpoint = Endpoint::new(Host::parse("127.0.0.1")?, 1, "/ipp/print")?;
    let report = diagnose::run(
        &ConfigStore::new(std::env::temp_dir().join("unused-brother-diagnostic")),
        &endpoint,
        Duration::from_millis(10),
    )?
    .result;
    assert!(matches!(report.status.ipp.reading, Reading::Failed { .. }));
    assert!(matches!(report.certificate.reading, Reading::Failed { .. }));
    assert!(matches!(
        report.authentication.reading,
        Reading::Missing { .. }
    ));
    Ok(())
}
