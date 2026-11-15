use brother_control::{
    discovery::{decode_packet, txt_properties},
    types::{Failure, FailureKind, Model},
};
use hickory_proto::{
    op::{Message, OpCode},
    rr::{
        Name, RData, Record,
        rdata::{SRV, TXT},
    },
};

fn failure(_: impl std::fmt::Display) -> Failure {
    Failure::new(FailureKind::InvalidInput, "invalid discovery fixture")
}

#[test]
fn txt_keys_are_case_insensitive_and_first_value_wins() -> Result<(), Failure> {
    let properties = txt_properties(b"\x04ty=A\x04TY=B\x0arp=ipp/x\x00");
    assert!(properties.is_err());
    let properties = txt_properties(b"\x04ty=A\x04TY=B\x08rp=ipp/x")?;
    assert_eq!(properties.get("ty").map(String::as_str), Some("A"));
    assert_eq!(properties.get("rp").map(String::as_str), Some("ipp/x"));
    Ok(())
}

#[test]
fn service_records_preserve_identity_model_host_port_and_resource() -> Result<(), Failure> {
    let name = Name::from_ascii("Printer._ipp._tcp.local.").map_err(failure)?;
    let mut message = Message::response(0, OpCode::Query);
    message.add_answer(Record::from_rdata(
        name.clone(),
        120,
        RData::SRV(SRV::new(
            0,
            0,
            631,
            Name::from_ascii("printer.local.").map_err(failure)?,
        )),
    ));
    message.add_additional(Record::from_rdata(
        name,
        120,
        RData::TXT(TXT::new(Vec::from([
            "ty=Brother MFC-L2860DW".to_owned(),
            "rp=ipp/print".to_owned(),
            "UUID=12345678-1234-11db-8000-001122334455".to_owned(),
        ]))),
    ));
    let printers = decode_packet(&message.to_vec().map_err(failure)?, 12)?;
    assert_eq!(printers.len(), 1);
    let printer = printers.first().ok_or_else(|| failure("missing printer"))?;
    assert_eq!(printer.printer.model, Model::MfcL2860dw);
    assert_eq!(printer.endpoint.host().as_str(), "printer.local");
    assert_eq!(printer.endpoint.port(), 631);
    assert_eq!(printer.endpoint.resource(), "/ipp/print");
    assert_eq!(printer.observed_at_unix_seconds, 12);
    assert!(decode_packet(b"malformed", 12).is_err());
    Ok(())
}
