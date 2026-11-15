use brother_control::{
    snmp_adapter::{Remaining, remaining},
    types::Reading,
};

#[test]
fn supply_sentinels_are_distinct_from_zero_percent() {
    assert_eq!(
        remaining(0, 100),
        Reading::Known {
            value: Remaining::Percent(0)
        }
    );
    assert_eq!(
        remaining(-3, 100),
        Reading::Known {
            value: Remaining::SomeRemaining
        }
    );
    assert!(matches!(remaining(-1, 100), Reading::Unknown { .. }));
    assert!(matches!(remaining(-2, 100), Reading::Unknown { .. }));
    assert!(matches!(remaining(10, -2), Reading::Unknown { .. }));
    assert!(matches!(remaining(101, 100), Reading::Failed { .. }));
}
