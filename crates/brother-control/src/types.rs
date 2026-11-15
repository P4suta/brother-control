use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {reason}")]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub kind: FailureKind,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<FailurePhase>,
}

impl Failure {
    pub fn new(kind: FailureKind, reason: &str) -> Self {
        Self {
            kind,
            reason: reason.to_owned(),
            phase: None,
        }
    }

    pub fn at(mut self, phase: FailurePhase) -> Self {
        self.phase = Some(phase);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePhase {
    LoginPage,
    LoginSubmission,
    LoginRedirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    InvalidInput,
    Unreachable,
    Timeout,
    Authentication,
    Certificate,
    Unsupported,
    UnknownResponse,
    WrongDevice,
    Stale,
    Cancelled,
    Busy,
    Storage,
    OutcomeUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Host(String);

impl Host {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        if let Ok(address) = input.parse::<IpAddr>() {
            return Ok(Self(address.to_string()));
        }
        let input = input.trim_end_matches('.');
        if input.is_empty()
            || input.len() > 253
            || !input.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "expected a hostname or IP address without a scheme, port, or credentials",
            ));
        }
        Ok(Self(input.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn authority(&self) -> String {
        match self.0.parse::<IpAddr>() {
            Ok(IpAddr::V6(_)) => format!("[{}]", self.0),
            Ok(IpAddr::V4(_)) | Err(_) => self.0.clone(),
        }
    }
}

impl TryFrom<String> for Host {
    type Error = Failure;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl std::str::FromStr for Host {
    type Err = Failure;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl From<Host> for String {
    fn from(value: Host) -> Self {
        value.0
    }
}
impl fmt::Display for Host {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Identity(Uuid);

impl Identity {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        let input = input.strip_prefix("urn:uuid:").unwrap_or(input);
        let uuid = Uuid::parse_str(input)
            .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid printer UUID"))?;
        brother_core::DeviceId::new(*uuid.as_bytes()).map_err(|_| {
            Failure::new(FailureKind::InvalidInput, "printer UUID must not be empty")
        })?;
        Ok(Self(uuid))
    }

    pub fn core(self) -> Result<brother_core::DeviceId, Failure> {
        brother_core::DeviceId::new(*self.0.as_bytes())
            .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid printer identity"))
    }

    pub fn compact(self) -> String {
        self.0.simple().to_string()
    }
}

impl TryFrom<String> for Identity {
    type Error = Failure;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Identity> for String {
    fn from(value: Identity) -> Self {
        value.0.hyphenated().to_string()
    }
}
impl fmt::Display for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Model {
    #[serde(rename = "HL-L2375DW")]
    HlL2375dw,
    #[serde(rename = "MFC-L2860DW")]
    MfcL2860dw,
}

impl Model {
    pub fn parse(input: &str) -> Result<Self, Failure> {
        let input = input.strip_prefix("Brother ").unwrap_or(input).trim();
        match input.strip_suffix(" series").unwrap_or(input) {
            "HL-L2375DW" => Ok(Self::HlL2375dw),
            "MFC-L2860DW" => Ok(Self::MfcL2860dw),
            _ => Err(Failure::new(
                FailureKind::Unsupported,
                "printer model is outside the supported scope",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    NativeCredentialStore,
    DnsSd,
    Ipp,
    Snmp,
    WebManagement,
    OfficialFirmware,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reading<T> {
    Known { value: T },
    Missing { reason: String },
    Unknown { reason: String },
    Unsupported { reason: String },
    Failed { failure: Failure },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation<T> {
    pub source: Source,
    pub observed_at_unix_seconds: u64,
    pub reading: Reading<T>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Printer {
    pub identity: Identity,
    pub model: Model,
    pub host: Host,
}
