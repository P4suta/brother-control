use brother_control::{
    discovery::PrivateSubnet,
    types::{Failure, FailureKind},
};

#[test]
fn explicit_probes_are_bounded_and_do_not_invent_printers() -> Result<(), Failure> {
    use brother_control::{discovery::probe, ipp_adapter::Endpoint, types::Host};
    use std::time::Duration;
    let endpoint = Endpoint::new(Host::parse("127.0.0.1")?, 1, "/ipp/print")?;
    let report = probe(std::slice::from_ref(&endpoint), Duration::from_millis(50))?;
    assert!(report.printers.is_empty());
    assert_eq!(report.attempts.len(), 1);
    assert!(matches!(
        report.attempts[0].reading,
        brother_control::types::Reading::Failed { .. }
    ));
    assert!(probe(&vec![endpoint; 257], Duration::from_secs(1)).is_err());
    assert!(probe(&[], Duration::ZERO).is_err());
    Ok(())
}

#[test]
fn scanning_requires_a_canonical_private_subnet_with_at_most_256_addresses() -> Result<(), Failure>
{
    for input in [
        "10.0.0.0/8",
        "192.168.1.2/24",
        "8.8.8.0/24",
        "0.0.0.0/0",
        "10.0.0.0/33",
    ] {
        assert!(PrivateSubnet::parse(input).is_err(), "{input}");
    }
    let subnet = PrivateSubnet::parse("192.168.7.0/24")?;
    let hosts = subnet.hosts(None)?;
    assert_eq!(hosts.len(), 254);
    assert_eq!(hosts.first().map(|host| host.as_str()), Some("192.168.7.1"));
    assert_eq!(
        hosts.last().map(|host| host.as_str()),
        Some("192.168.7.254")
    );
    let routed = PrivateSubnet::parse("192.168.7.11/32")?.hosts(Some(7))?;
    assert_eq!(
        routed.first().map(|host| host.as_str()),
        Some("fd7a:115c:a1e0:b1a:0:7:c0a8:70b")
    );
    assert_eq!(PrivateSubnet::parse("10.0.0.0/31")?.hosts(None)?.len(), 2);
    assert!(matches!(
        PrivateSubnet::parse("garbage").map_err(|failure| failure.kind),
        Err(FailureKind::InvalidInput)
    ));
    Ok(())
}
