use brother_control::{
    settings::{SettingObservation, SettingValue, SettingsDiff, Snapshot, diff},
    types::{Failure, Identity, Observation, Reading, Source},
};
use brother_core::{Setting, Switch};

#[test]
fn exported_differences_can_be_read_without_stripping_the_cli_envelope() -> Result<(), String> {
    let input = r#"{"operation":"settings_diff","result":{"schema":1,"device":"11111111-1111-4111-8111-111111111111","changes":[]}}"#;
    let document: brother_control::settings::DifferenceDocument =
        serde_json::from_str(input).map_err(|error| error.to_string())?;
    assert!(
        document
            .into_difference()
            .map_err(|error| error.to_string())?
            .changes
            .is_empty()
    );
    assert!(
        serde_json::from_str::<brother_control::settings::DifferenceDocument>(
            &input.replace("settings_diff", "settings_apply")
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn exported_values_keep_their_device_binding_when_used_as_desired_settings() -> Result<(), String> {
    let input = r#"{"operation":"settings_export","result":{"device":"11111111-1111-4111-8111-111111111111","values":[{"key":"toner_save","observation":{"source":"web_management","observed_at_unix_seconds":12,"reading":{"state":"known","value":{"key":"toner_save","value":"on"}}}}]}}"#;
    let device = Identity::parse("11111111-1111-4111-8111-111111111111")
        .map_err(|error| error.to_string())?;
    let document: brother_control::settings::DesiredDocument =
        serde_json::from_str(input).map_err(|error| error.to_string())?;
    assert_eq!(
        document
            .values_for(device)
            .map_err(|error| error.to_string())?,
        Vec::from([SettingValue::from(Setting::TonerSave(Switch::On))])
    );
    let document: brother_control::settings::DesiredDocument =
        serde_json::from_str(input).map_err(|error| error.to_string())?;
    assert!(
        document
            .values_for(
                Identity::parse("22222222-2222-4222-8222-222222222222")
                    .map_err(|error| error.to_string())?
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn settings_json_validates_values_before_building_a_device_bound_diff() -> Result<(), Failure> {
    let desired: SettingValue = serde_json::from_str(r#"{"key":"toner_save","value":"on"}"#)
        .map_err(|_| {
            Failure::new(
                brother_control::types::FailureKind::InvalidInput,
                "invalid fixture",
            )
        })?;
    assert_eq!(desired.core(), Setting::TonerSave(Switch::On));
    for input in [
        r#"{"key":"sleep","value":51}"#,
        r#"{"key":"network","value":1}"#,
        r#"{"key":"quiet_mode","value":"unknown"}"#,
    ] {
        assert!(serde_json::from_str::<SettingValue>(input).is_err());
    }
    let device = Identity::parse("11111111-1111-4111-8111-111111111111")?;
    let snapshot = Snapshot {
        device,
        values: Vec::from([SettingObservation {
            key: brother_core::SettingKey::TonerSave,
            observation: Observation {
                source: Source::WebManagement,
                observed_at_unix_seconds: 12,
                reading: Reading::Known {
                    value: SettingValue::from(Setting::TonerSave(Switch::Off)),
                },
            },
        }]),
    };
    let difference = diff(&snapshot, &[desired])?;
    assert_eq!(difference.device, device);
    assert_eq!(difference.changes.len(), 1);
    let encoded = serde_json::to_vec(&difference).map_err(|_| {
        Failure::new(
            brother_control::types::FailureKind::Storage,
            "fixture serialization failed",
        )
    })?;
    let decoded: SettingsDiff = serde_json::from_slice(&encoded).map_err(|_| {
        Failure::new(
            brother_control::types::FailureKind::InvalidInput,
            "fixture deserialization failed",
        )
    })?;
    assert_eq!(
        decoded.changes[0].before.core(),
        Setting::TonerSave(Switch::Off)
    );
    assert!(diff(&snapshot, &[desired, desired]).is_err());
    Ok(())
}
