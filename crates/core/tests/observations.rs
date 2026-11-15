use brother_core::{DeviceId, InputError, Minutes, SupplyLevel};

#[test]
fn snmp_supply_sentinels_preserve_their_meaning() {
    assert_eq!(SupplyLevel::from_snmp(-1, 100), SupplyLevel::Other);
    assert_eq!(SupplyLevel::from_snmp(-2, 100), SupplyLevel::Unknown);
    assert_eq!(SupplyLevel::from_snmp(-3, 100), SupplyLevel::SomeRemaining);
    assert_eq!(SupplyLevel::from_snmp(25, 50), SupplyLevel::Percent(50));
    assert_eq!(SupplyLevel::from_snmp(0, 100), SupplyLevel::Percent(0));
    assert_eq!(SupplyLevel::from_snmp(50, -2), SupplyLevel::Unknown);
    assert_eq!(SupplyLevel::from_snmp(101, 100), SupplyLevel::Invalid);
    assert_eq!(SupplyLevel::from_snmp(-4, 100), SupplyLevel::Invalid);
}

#[test]
fn minutes_and_identity_refuse_unchecked_values() -> Result<(), InputError> {
    assert_eq!(Minutes::new(51), Err(InputError::OutOfRange));
    assert_eq!(Minutes::new(0)?.get(), 0);
    assert_eq!(DeviceId::new([0; 16]), Err(InputError::EmptyIdentity));
    assert_eq!(DeviceId::new([7; 16])?.bytes(), &[7; 16]);
    Ok(())
}
