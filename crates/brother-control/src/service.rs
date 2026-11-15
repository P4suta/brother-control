use crate::{
    ipp_adapter::{self, Endpoint, IppStatus},
    snmp_adapter::{self, SnmpStatus},
    types::{Failure, Host, Observation, Reading, Source},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusReport {
    pub host: Host,
    pub ipp: Observation<IppStatus>,
    pub snmp: Observation<SnmpStatus>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationName {
    AuthorizeCredentials,
    Setup,
    Status,
    Discover,
    Diagnose,
    FirmwareCheck,
    SettingsList,
    SettingsGet,
    SettingsExport,
    SettingsDiff,
    SettingsApply,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output<T> {
    pub operation: OperationName,
    pub result: T,
}

#[derive(Serialize)]
pub struct ErrorReport {
    pub error: Failure,
}

pub trait Report {
    fn succeeded(&self) -> bool;
}

pub fn reboot(_device: crate::types::Identity) -> Failure {
    Failure::new(
        crate::types::FailureKind::Unsupported,
        "remote reboot has no verified adapter for the supported printer models",
    )
}

impl Report for crate::inspection::Structure {
    fn succeeded(&self) -> bool {
        true
    }
}

impl Report for crate::web::CertificateInfo {
    fn succeeded(&self) -> bool {
        true
    }
}

impl Report for crate::settings::Snapshot {
    fn succeeded(&self) -> bool {
        self.values
            .iter()
            .any(|value| matches!(value.observation.reading, Reading::Known { .. }))
    }
}

impl Report for crate::settings::SettingsDiff {
    fn succeeded(&self) -> bool {
        true
    }
}

pub fn settings_get(
    store: &crate::config::ConfigStore,
    device: crate::types::Identity,
    timeout: Duration,
) -> Result<Output<crate::settings::Snapshot>, Failure> {
    use crate::settings::SettingsPort;
    let mut adapter =
        crate::device::Device::open(store.load(device)?, timeout, store.lease(device)?)?;
    Ok(Output {
        operation: OperationName::SettingsGet,
        result: adapter.read()?,
    })
}

pub fn settings_diff(
    store: &crate::config::ConfigStore,
    device: crate::types::Identity,
    desired: &[crate::settings::SettingValue],
    timeout: Duration,
) -> Result<Output<crate::settings::SettingsDiff>, Failure> {
    let snapshot = settings_get(store, device, timeout)?.result;
    Ok(Output {
        operation: OperationName::SettingsDiff,
        result: crate::settings::diff(&snapshot, desired)?,
    })
}

pub fn settings_apply<C: FnMut() -> bool>(
    store: &crate::config::ConfigStore,
    device: crate::types::Identity,
    difference: &crate::settings::SettingsDiff,
    timeout: Duration,
    cancelled: C,
) -> Result<Output<crate::settings::ApplyReport>, Failure> {
    if difference.device != device {
        return Err(Failure::new(
            crate::types::FailureKind::WrongDevice,
            "difference identifies another selected device",
        ));
    }
    difference.validate()?;
    let mut adapter =
        crate::device::Device::open(store.load(device)?, timeout, store.lease(device)?)?;
    Ok(Output {
        operation: OperationName::SettingsApply,
        result: crate::settings::apply(&mut adapter, difference, cancelled)?,
    })
}

impl Report for StatusReport {
    fn succeeded(&self) -> bool {
        matches!(self.ipp.reading, Reading::Known { .. })
    }
}

impl Report for crate::discovery::DiscoveryReport {
    fn succeeded(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn status(endpoint: &Endpoint, timeout: Duration) -> Result<Output<StatusReport>, Failure> {
    let reading = match ipp_adapter::query(endpoint, timeout) {
        Ok(value) => Reading::Known { value },
        Err(failure) => Reading::Failed { failure },
    };
    Ok(Output {
        operation: OperationName::Status,
        result: StatusReport {
            host: endpoint.host().clone(),
            ipp: Observation {
                source: Source::Ipp,
                observed_at_unix_seconds: ipp_adapter::now()?,
                reading,
            },
            snmp: snmp_adapter::query(endpoint.host(), timeout)?,
        },
    })
}

pub fn discover(window: Duration) -> Result<Output<crate::discovery::DiscoveryReport>, Failure> {
    Ok(Output {
        operation: OperationName::Discover,
        result: crate::discovery::discover(window)?,
    })
}
