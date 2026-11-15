use brother_control::{
    settings::{
        ApplyState, Difference, SettingObservation, SettingValue, SettingsDiff, SettingsPort,
        Snapshot, apply,
    },
    types::{Failure, FailureKind, Identity, Observation, Reading, Source},
};
use brother_core::{Setting, SettingKey, Switch, VerifiedChange};

struct Device {
    identity: Identity,
    value: Switch,
    writes: usize,
    fail: Option<FailureKind>,
}

impl SettingsPort for Device {
    fn read(&mut self) -> Result<Snapshot, Failure> {
        Ok(Snapshot {
            device: self.identity,
            values: Vec::from([SettingObservation {
                key: SettingKey::TonerSave,
                observation: Observation {
                    source: Source::WebManagement,
                    observed_at_unix_seconds: 12,
                    reading: Reading::Known {
                        value: SettingValue::from(Setting::TonerSave(self.value)),
                    },
                },
            }]),
        })
    }
    fn write(&mut self, change: VerifiedChange) -> Result<(), Failure> {
        assert_eq!(change.device(), self.identity.core()?);
        self.writes += 1;
        if let Setting::TonerSave(value) = change.desired() {
            self.value = value;
        }
        match self.fail {
            Some(kind) => Err(Failure::new(kind, "injected write failure")),
            None => Ok(()),
        }
    }
}

fn fixture() -> Result<(Device, SettingsDiff), Failure> {
    let identity = Identity::parse("11111111-1111-4111-8111-111111111111")?;
    Ok((
        Device {
            identity,
            value: Switch::Off,
            writes: 0,
            fail: None,
        },
        SettingsDiff {
            schema: 1,
            device: identity,
            changes: Vec::from([Difference {
                before: SettingValue::from(Setting::TonerSave(Switch::Off)),
                desired: SettingValue::from(Setting::TonerSave(Switch::On)),
            }]),
        },
    ))
}

#[test]
fn apply_checks_current_values_and_reads_back_without_retrying() -> Result<(), Failure> {
    let (mut device, difference) = fixture()?;
    let report = apply(&mut device, &difference, || false)?;
    assert!(matches!(
        report.outcomes[0].state,
        ApplyState::Verified { .. }
    ));
    assert_eq!(device.writes, 1);
    let stale = apply(&mut device, &difference, || false)?;
    assert!(
        matches!(&stale.outcomes[0].state, ApplyState::Rejected { failure } if failure.kind == FailureKind::Stale)
    );
    assert_eq!(device.writes, 1);
    Ok(())
}

#[test]
fn ambiguous_writes_and_cancellation_preserve_their_outcomes() -> Result<(), Failure> {
    let (mut device, difference) = fixture()?;
    device.fail = Some(FailureKind::OutcomeUnknown);
    let report = apply(&mut device, &difference, || false)?;
    assert!(matches!(
        report.outcomes[0].state,
        ApplyState::Uncertain { .. }
    ));
    if let ApplyState::Uncertain { readback, .. } = &report.outcomes[0].state {
        assert!(matches!(readback, Reading::Known { .. }));
    }
    assert_eq!(device.writes, 1);
    let (mut device, difference) = fixture()?;
    let report = apply(&mut device, &difference, || true)?;
    assert!(matches!(report.outcomes[0].state, ApplyState::Cancelled));
    assert_eq!(device.writes, 0);
    let other = Identity::parse("22222222-2222-4222-8222-222222222222")?;
    device.identity = other;
    let report = apply(&mut device, &difference, || false)?;
    assert!(
        matches!(&report.outcomes[0].state, ApplyState::Rejected { failure } if failure.kind == FailureKind::WrongDevice)
    );
    assert_eq!(device.writes, 0);
    Ok(())
}

#[test]
fn partial_application_and_cancel_before_send_do_not_hide_completed_changes() -> Result<(), Failure>
{
    let (mut device, mut difference) = fixture()?;
    difference.changes.push(Difference {
        before: SettingValue::from(Setting::QuietMode(Switch::Off)),
        desired: SettingValue::from(Setting::QuietMode(Switch::On)),
    });
    let report = apply(&mut device, &difference, || false)?;
    assert!(matches!(
        report.outcomes[0].state,
        ApplyState::Verified { .. }
    ));
    assert!(matches!(
        report.outcomes[1].state,
        ApplyState::Rejected { .. }
    ));
    assert_eq!(device.writes, 1);
    let (mut device, difference) = fixture()?;
    let mut polls = 0;
    let report = apply(&mut device, &difference, || {
        polls += 1;
        polls == 2
    })?;
    assert!(matches!(report.outcomes[0].state, ApplyState::Cancelled));
    assert_eq!(device.writes, 0);
    Ok(())
}

struct CoupledDevice {
    device: Device,
    quiet: Switch,
}

impl SettingsPort for CoupledDevice {
    fn read(&mut self) -> Result<Snapshot, Failure> {
        let mut snapshot = self.device.read()?;
        snapshot.values.push(SettingObservation {
            key: SettingKey::QuietMode,
            observation: Observation {
                source: Source::WebManagement,
                observed_at_unix_seconds: 12,
                reading: Reading::Known {
                    value: Setting::QuietMode(self.quiet).into(),
                },
            },
        });
        Ok(snapshot)
    }

    fn write(&mut self, change: VerifiedChange) -> Result<(), Failure> {
        if let Setting::QuietMode(value) = change.desired() {
            self.quiet = value;
            self.device.value = Switch::Off;
            self.device.writes += 1;
            Ok(())
        } else {
            self.device.write(change)
        }
    }
}

#[test]
fn a_later_write_cannot_leave_an_earlier_result_verified_after_it_changes() -> Result<(), Failure> {
    let (device, mut difference) = fixture()?;
    let mut device = CoupledDevice {
        device,
        quiet: Switch::Off,
    };
    difference.changes.push(Difference {
        before: Setting::QuietMode(Switch::Off).into(),
        desired: Setting::QuietMode(Switch::On).into(),
    });
    let report = apply(&mut device, &difference, || false)?;
    assert!(matches!(&report.outcomes[0].state, ApplyState::Uncertain {
        readback: Reading::Known { value }, ..
    } if *value == SettingValue::from(Setting::TonerSave(Switch::Off))));
    assert!(matches!(
        report.outcomes[1].state,
        ApplyState::Verified { .. }
    ));
    assert_eq!(device.device.writes, 2);
    Ok(())
}
