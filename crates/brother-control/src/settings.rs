use crate::types::{Failure, FailureKind, Identity, Observation, Reading};
use brother_core::{Change, Duplex, Minutes, PaperSize, PaperType, Setting, SettingKey, Switch};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(remote = "Switch", rename_all = "snake_case")]
enum SwitchWire {
    Off,
    On,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "Duplex", rename_all = "snake_case")]
enum DuplexWire {
    Off,
    LongEdge,
    ShortEdge,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "PaperSize", rename_all = "snake_case")]
enum PaperSizeWire {
    A4,
    A5,
    A6,
    Letter,
    Legal,
    Executive,
    B5,
    B6,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "PaperType", rename_all = "snake_case")]
enum PaperTypeWire {
    Plain,
    Thin,
    Thick,
    Thicker,
    Recycled,
    Bond,
    Label,
    Envelope,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "SettingKey", rename_all = "snake_case")]
pub enum SettingKeyWire {
    Sleep,
    TonerSave,
    QuietMode,
    Duplex,
    PaperSize,
    PaperType,
}

pub const KEYS: [SettingKey; 6] = [
    SettingKey::Sleep,
    SettingKey::TonerSave,
    SettingKey::QuietMode,
    SettingKey::Duplex,
    SettingKey::PaperSize,
    SettingKey::PaperType,
];

pub fn parse_key(input: &str) -> Result<SettingKey, Failure> {
    match input {
        "sleep" => Ok(SettingKey::Sleep),
        "toner_save" => Ok(SettingKey::TonerSave),
        "quiet_mode" => Ok(SettingKey::QuietMode),
        "duplex" => Ok(SettingKey::Duplex),
        "paper_size" => Ok(SettingKey::PaperSize),
        "paper_type" => Ok(SettingKey::PaperType),
        _ => Err(Failure::new(
            FailureKind::InvalidInput,
            "unknown managed setting key",
        )),
    }
}

#[derive(Serialize)]
pub struct ManagedKey {
    #[serde(with = "SettingKeyWire")]
    pub key: SettingKey,
}
#[derive(Serialize)]
pub struct Catalog {
    pub settings: Vec<ManagedKey>,
}
impl crate::service::Report for Catalog {
    fn succeeded(&self) -> bool {
        true
    }
}
pub fn catalog() -> Catalog {
    Catalog {
        settings: KEYS.into_iter().map(|key| ManagedKey { key }).collect(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desired {
    pub values: Vec<SettingValue>,
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct DesiredDocument(DesiredInput);

#[derive(Deserialize)]
#[serde(untagged)]
enum DesiredInput {
    Values(Desired),
    Export(ExportedSettings),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportedSettings {
    operation: ExportOperation,
    result: Snapshot,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportOperation {
    SettingsExport,
}

impl DesiredDocument {
    pub fn values_for(self, device: Identity) -> Result<Vec<SettingValue>, Failure> {
        match self.0 {
            DesiredInput::Values(desired) => Ok(desired.values),
            DesiredInput::Export(ExportedSettings {
                operation: ExportOperation::SettingsExport,
                result,
            }) => {
                if result.device != device {
                    return Err(Failure::new(
                        FailureKind::WrongDevice,
                        "export belongs to another selected device",
                    ));
                }
                result
                    .values
                    .iter()
                    .map(|value| result.current(value.key))
                    .collect()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct DifferenceDocument(DifferenceInput);

#[derive(Deserialize)]
#[serde(untagged)]
enum DifferenceInput {
    Bare(SettingsDiff),
    Output(ExportedDifference),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportedDifference {
    operation: DifferenceOperation,
    result: SettingsDiff,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DifferenceOperation {
    SettingsDiff,
}

impl DifferenceDocument {
    pub fn into_difference(self) -> Result<SettingsDiff, Failure> {
        let difference = match self.0 {
            DifferenceInput::Bare(value)
            | DifferenceInput::Output(ExportedDifference {
                operation: DifferenceOperation::SettingsDiff,
                result: value,
            }) => value,
        };
        difference.validate()?;
        Ok(difference)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SettingWire", into = "SettingWire")]
pub struct SettingValue(Setting);

impl SettingValue {
    pub const fn core(self) -> Setting {
        self.0
    }
    pub const fn key(self) -> SettingKey {
        self.0.key()
    }
}

impl From<Setting> for SettingValue {
    fn from(value: Setting) -> Self {
        Self(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "key", rename_all = "snake_case", deny_unknown_fields)]
enum SettingWire {
    Sleep {
        value: u8,
    },
    TonerSave {
        #[serde(with = "SwitchWire")]
        value: Switch,
    },
    QuietMode {
        #[serde(with = "SwitchWire")]
        value: Switch,
    },
    Duplex {
        #[serde(with = "DuplexWire")]
        value: Duplex,
    },
    PaperSize {
        #[serde(with = "PaperSizeWire")]
        value: PaperSize,
    },
    PaperType {
        #[serde(with = "PaperTypeWire")]
        value: PaperType,
    },
}

impl TryFrom<SettingWire> for SettingValue {
    type Error = Failure;
    fn try_from(value: SettingWire) -> Result<Self, Self::Error> {
        Ok(Self(match value {
            SettingWire::Sleep { value } => Setting::Sleep(Minutes::new(value).map_err(|_| {
                Failure::new(
                    FailureKind::InvalidInput,
                    "sleep time must be between 0 and 50 minutes",
                )
            })?),
            SettingWire::TonerSave { value } => Setting::TonerSave(value),
            SettingWire::QuietMode { value } => Setting::QuietMode(value),
            SettingWire::Duplex { value } => Setting::Duplex(value),
            SettingWire::PaperSize { value } => Setting::PaperSize(value),
            SettingWire::PaperType { value } => Setting::PaperType(value),
        }))
    }
}

impl From<SettingValue> for SettingWire {
    fn from(value: SettingValue) -> Self {
        match value.0 {
            Setting::Sleep(value) => Self::Sleep { value: value.get() },
            Setting::TonerSave(value) => Self::TonerSave { value },
            Setting::QuietMode(value) => Self::QuietMode { value },
            Setting::Duplex(value) => Self::Duplex { value },
            Setting::PaperSize(value) => Self::PaperSize { value },
            Setting::PaperType(value) => Self::PaperType { value },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingObservation {
    #[serde(with = "SettingKeyWire")]
    pub key: SettingKey,
    pub observation: Observation<SettingValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub device: Identity,
    pub values: Vec<SettingObservation>,
}

impl Snapshot {
    pub fn current(&self, key: SettingKey) -> Result<SettingValue, Failure> {
        let mut values = self.values.iter().filter(|value| value.key == key);
        let value = values.next().ok_or_else(|| {
            Failure::new(
                FailureKind::Unsupported,
                "requested setting was not observed",
            )
        })?;
        if values.next().is_some() {
            return Err(Failure::new(
                FailureKind::UnknownResponse,
                "duplicate setting observations",
            ));
        }
        match &value.observation.reading {
            Reading::Known { value } if value.key() == key => Ok(*value),
            Reading::Failed { failure } => Err(failure.clone()),
            Reading::Known { .. }
            | Reading::Missing { .. }
            | Reading::Unknown { .. }
            | Reading::Unsupported { .. } => Err(Failure::new(
                FailureKind::Unsupported,
                "requested setting has no known current value",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Difference {
    pub before: SettingValue,
    pub desired: SettingValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsDiff {
    pub schema: u16,
    pub device: Identity,
    pub changes: Vec<Difference>,
}

impl SettingsDiff {
    pub fn validate(&self) -> Result<(), Failure> {
        if self.schema != 1 || self.changes.len() > KEYS.len() {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "invalid settings difference schema or change count",
            ));
        }
        for (index, change) in self.changes.iter().enumerate() {
            Change::new(
                self.device.core()?,
                change.before.core(),
                change.desired.core(),
            )
            .map_err(|_| {
                Failure::new(
                    FailureKind::InvalidInput,
                    "setting difference contains mismatched keys",
                )
            })?;
            if self.changes[..index]
                .iter()
                .any(|previous| previous.desired.key() == change.desired.key())
            {
                return Err(Failure::new(
                    FailureKind::InvalidInput,
                    "setting difference repeats a setting",
                ));
            }
        }
        Ok(())
    }
}

pub fn diff(snapshot: &Snapshot, desired: &[SettingValue]) -> Result<SettingsDiff, Failure> {
    if desired.len() > KEYS.len() {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "too many desired settings",
        ));
    }
    let mut changes = Vec::new();
    for (index, &value) in desired.iter().enumerate() {
        if desired[..index]
            .iter()
            .any(|previous| previous.key() == value.key())
        {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "desired settings repeat a key",
            ));
        }
        let before = snapshot.current(value.key())?;
        if before != value {
            changes.push(Difference {
                before,
                desired: value,
            });
        }
    }
    let result = SettingsDiff {
        schema: 1,
        device: snapshot.device,
        changes,
    };
    result.validate()?;
    Ok(result)
}

pub trait SettingsPort {
    fn read(&mut self) -> Result<Snapshot, Failure>;
    fn write(&mut self, change: brother_core::VerifiedChange) -> Result<(), Failure>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ApplyState {
    Verified {
        observed: SettingValue,
    },
    Rejected {
        failure: Failure,
    },
    Uncertain {
        failure: Failure,
        readback: Reading<SettingValue>,
    },
    Cancelled,
    NotAttempted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub change: Difference,
    pub state: ApplyState,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplyReport {
    pub device: Identity,
    pub outcomes: Vec<ApplyOutcome>,
}

impl crate::service::Report for ApplyReport {
    fn succeeded(&self) -> bool {
        self.outcomes
            .iter()
            .all(|outcome| matches!(outcome.state, ApplyState::Verified { .. }))
    }
}

fn verified_change<P: SettingsPort>(
    port: &mut P,
    difference: &SettingsDiff,
    change: Difference,
) -> Result<brother_core::VerifiedChange, Failure> {
    let current = port.read()?;
    let key = change.before.key();
    let change = Change::new(
        difference.device.core()?,
        change.before.core(),
        change.desired.core(),
    )
    .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid setting change"))?;
    if current.device != difference.device {
        return Err(Failure::new(
            FailureKind::WrongDevice,
            "device identity changed before the write",
        ));
    }
    let value = current.current(key)?;
    change
        .verify(current.device.core()?, value.core())
        .map_err(|error| {
            let kind = match error {
                brother_core::ChangeError::DifferentSetting => FailureKind::InvalidInput,
                brother_core::ChangeError::WrongDevice => FailureKind::WrongDevice,
                brother_core::ChangeError::Stale => FailureKind::Stale,
            };
            Failure::new(
                kind,
                "current setting differs from the expected device or value",
            )
        })
}

fn read_back<P: SettingsPort>(
    port: &mut P,
    device: Identity,
    key: SettingKey,
) -> Result<SettingValue, Failure> {
    let current = port.read()?;
    if current.device != device {
        return Err(Failure::new(
            FailureKind::WrongDevice,
            "another device answered after the write",
        ));
    }
    current.current(key)
}

pub fn apply<P: SettingsPort, C: FnMut() -> bool>(
    port: &mut P,
    difference: &SettingsDiff,
    mut cancelled: C,
) -> Result<ApplyReport, Failure> {
    difference.validate()?;
    let mut outcomes = Vec::new();
    let mut stopped = false;
    for &change in &difference.changes {
        let state = if stopped {
            ApplyState::NotAttempted
        } else if cancelled() {
            ApplyState::Cancelled
        } else {
            match verified_change(port, difference, change) {
                Err(failure) => ApplyState::Rejected { failure },
                Ok(_) if cancelled() => ApplyState::Cancelled,
                Ok(verified) => match port.write(verified) {
                    Err(failure)
                        if matches!(
                            failure.kind,
                            FailureKind::OutcomeUnknown | FailureKind::Timeout
                        ) =>
                    {
                        let readback =
                            match read_back(port, difference.device, change.desired.key()) {
                                Ok(value) => Reading::Known { value },
                                Err(failure) => Reading::Failed { failure },
                            };
                        ApplyState::Uncertain { failure, readback }
                    }
                    Err(failure) => ApplyState::Rejected { failure },
                    Ok(()) => match read_back(port, difference.device, change.desired.key()) {
                        Ok(observed) if observed == change.desired => {
                            ApplyState::Verified { observed }
                        }
                        Ok(observed) => ApplyState::Uncertain {
                            failure: Failure::new(
                                FailureKind::OutcomeUnknown,
                                "readback did not match the requested value",
                            ),
                            readback: Reading::Known { value: observed },
                        },
                        Err(failure) => ApplyState::Uncertain {
                            failure: Failure::new(
                                FailureKind::OutcomeUnknown,
                                "write was sent but readback failed",
                            ),
                            readback: Reading::Failed { failure },
                        },
                    },
                },
            }
        };
        stopped |= !matches!(state, ApplyState::Verified { .. });
        outcomes.push(ApplyOutcome { change, state });
    }
    if outcomes.len() > 1
        && outcomes
            .iter()
            .any(|outcome| matches!(outcome.state, ApplyState::Verified { .. }))
    {
        let final_snapshot = port.read();
        for outcome in &mut outcomes {
            if !matches!(outcome.state, ApplyState::Verified { .. }) {
                continue;
            }
            let reading = match &final_snapshot {
                Ok(snapshot) if snapshot.device == difference.device => {
                    snapshot.current(outcome.change.desired.key())
                }
                Ok(_) => Err(Failure::new(
                    FailureKind::WrongDevice,
                    "another device answered during final verification",
                )),
                Err(failure) => Err(failure.clone()),
            };
            let readback = match reading {
                Ok(value) if value == outcome.change.desired => continue,
                Ok(value) => Reading::Known { value },
                Err(failure) => Reading::Failed { failure },
            };
            outcome.state = ApplyState::Uncertain {
                failure: Failure::new(
                    FailureKind::OutcomeUnknown,
                    "setting was not confirmed during final verification",
                ),
                readback,
            };
        }
    }
    Ok(ApplyReport {
        device: difference.device,
        outcomes,
    })
}
