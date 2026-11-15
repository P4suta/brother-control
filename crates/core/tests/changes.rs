use brother_core::{Change, ChangeError, DeviceId, InputError, Minutes, Setting, Switch};

#[test]
fn a_change_requires_the_same_setting_kind() -> Result<(), InputError> {
    let id = DeviceId::new([1; 16])?;
    assert_eq!(
        Change::new(
            id,
            Setting::Sleep(Minutes::new(5)?),
            Setting::TonerSave(Switch::On)
        ),
        Err(ChangeError::DifferentSetting)
    );
    Ok(())
}

#[test]
fn a_stale_change_or_other_device_cannot_be_verified() -> Result<(), InputError> {
    let id = DeviceId::new([1; 16])?;
    let other = DeviceId::new([2; 16])?;
    let before = Setting::TonerSave(Switch::Off);
    let desired = Setting::TonerSave(Switch::On);
    let change = Change::new(id, before, desired);
    assert_eq!(
        change.as_ref().map(|c| c.verify(id, desired)),
        Ok(Err(ChangeError::Stale))
    );
    assert_eq!(
        change.as_ref().map(|c| c.verify(other, before)),
        Ok(Err(ChangeError::WrongDevice))
    );
    Ok(())
}

#[test]
fn verification_binds_identity_and_desired_value() -> Result<(), ChangeError> {
    let id = DeviceId::new([1; 16]).map_err(|_| ChangeError::WrongDevice)?;
    let desired = Setting::QuietMode(Switch::On);
    let verified = Change::new(id, Setting::QuietMode(Switch::Off), desired)?
        .verify(id, Setting::QuietMode(Switch::Off))?;
    assert_eq!(verified.device(), id);
    assert_eq!(verified.desired(), desired);
    Ok(())
}
