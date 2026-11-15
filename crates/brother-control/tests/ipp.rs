use brother_control::{
    ipp_adapter::{Endpoint, PrinterState, parse_response, request_bytes},
    types::{FailureKind, Host, Reading},
};

fn attribute(
    buffer: &mut Vec<u8>,
    tag: u8,
    name: &str,
    value: &[u8],
) -> Result<(), std::num::TryFromIntError> {
    buffer.push(tag);
    buffer.extend_from_slice(&u16::try_from(name.len())?.to_be_bytes());
    buffer.extend_from_slice(name.as_bytes());
    buffer.extend_from_slice(&u16::try_from(value.len())?.to_be_bytes());
    buffer.extend_from_slice(value);
    Ok(())
}

fn response(state: i32, reason: &str) -> Result<Vec<u8>, std::num::TryFromIntError> {
    let mut bytes = Vec::from([1, 1, 0, 0, 0, 0, 0, 1, 1]);
    attribute(&mut bytes, 0x47, "attributes-charset", b"utf-8")?;
    attribute(&mut bytes, 0x48, "attributes-natural-language", b"en")?;
    bytes.push(4);
    attribute(
        &mut bytes,
        0x45,
        "printer-uuid",
        b"urn:uuid:12345678-1234-11db-8000-001122334455",
    )?;
    attribute(
        &mut bytes,
        0x41,
        "printer-make-and-model",
        b"Brother MFC-L2860DW",
    )?;
    attribute(&mut bytes, 0x23, "printer-state", &state.to_be_bytes())?;
    attribute(&mut bytes, 0x44, "printer-state-reasons", reason.as_bytes())?;
    bytes.push(3);
    Ok(bytes)
}

#[test]
fn ipp_requests_only_read_attributes() -> Result<(), brother_control::types::Failure> {
    let endpoint = Endpoint::new(Host::parse("printer.local")?, 631, "/ipp/print")?;
    let bytes = request_bytes(&endpoint)?;
    assert_eq!(bytes.get(2..4), Some([0, 11].as_slice()));
    assert!(Endpoint::new(Host::parse("printer.local")?, 0, "/ipp/print").is_err());
    assert!(Endpoint::new(Host::parse("printer.local")?, 631, "/../admin").is_err());
    Ok(())
}

#[test]
fn sleeping_and_unknown_state_are_not_reported_as_ready()
-> Result<(), brother_control::types::Failure> {
    let endpoint = Endpoint::new(Host::parse("printer.local")?, 631, "/ipp/print")?;
    let sleep = response(3, "power-save").map_err(|_| {
        brother_control::types::Failure::new(FailureKind::InvalidInput, "fixture is too large")
    })?;
    let status = parse_response(&sleep, &endpoint, 123)?;
    assert_eq!(
        status.state.reading,
        Reading::Known {
            value: PrinterState::Sleeping
        }
    );
    assert_eq!(status.state.observed_at_unix_seconds, 123);
    let unknown = response(999, "none").map_err(|_| {
        brother_control::types::Failure::new(FailureKind::InvalidInput, "fixture is too large")
    })?;
    assert!(matches!(
        parse_response(&unknown, &endpoint, 123)?.state.reading,
        Reading::Unknown { .. }
    ));
    Ok(())
}

#[test]
fn malformed_or_unrelated_responses_fail_closed() -> Result<(), brother_control::types::Failure> {
    let endpoint = Endpoint::new(Host::parse("printer.local")?, 631, "/ipp/print")?;
    assert!(parse_response(b"not an IPP packet", &endpoint, 1).is_err());
    let mut bytes = response(3, "none").map_err(|_| {
        brother_control::types::Failure::new(FailureKind::InvalidInput, "fixture is too large")
    })?;
    bytes[7] = 2;
    assert_eq!(
        parse_response(&bytes, &endpoint, 1).map(|_| ()),
        Err(brother_control::types::Failure::new(
            FailureKind::UnknownResponse,
            "IPP response request ID does not match"
        ))
    );
    Ok(())
}
