fn main() -> Result<(), String> {
    let device = brother_core::DeviceId::new([1; 16]).map_err(|error| error.to_string())?;
    let _change = brother_core::VerifiedChange {
        device,
        desired: brother_core::Setting::TonerSave(brother_core::Switch::On),
    };
    Ok(())
}
