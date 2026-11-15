use brother_core::{Change, DeviceId, Setting, Switch, VerifiedChange};

pub fn verified() -> Result<VerifiedChange, String> {
    let device = DeviceId::new([1; 16]).map_err(|error| error.to_string())?;
    let current = Setting::TonerSave(Switch::Off);
    Change::new(device, current, Setting::TonerSave(Switch::On))
        .and_then(|change| change.verify(device, current))
        .map_err(|error| error.to_string())
}

pub fn consume(_: VerifiedChange) {}
