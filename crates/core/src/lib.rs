#![no_std]

mod settings;
pub use settings::{
    Change, ChangeError, Duplex, PaperSize, PaperType, Setting, SettingKey, Switch, VerifiedChange,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    OutOfRange,
    EmptyIdentity,
}

impl core::fmt::Display for InputError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("value outside the supported range"),
            Self::EmptyIdentity => f.write_str("device identity must not be empty"),
        }
    }
}

impl core::error::Error for InputError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId([u8; 16]);

impl DeviceId {
    pub fn new(bytes: [u8; 16]) -> Result<Self, InputError> {
        if bytes == [0; 16] {
            Err(InputError::EmptyIdentity)
        } else {
            Ok(Self(bytes))
        }
    }

    pub const fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Minutes(u8);

impl Minutes {
    pub const fn new(value: u8) -> Result<Self, InputError> {
        if value <= 50 {
            Ok(Self(value))
        } else {
            Err(InputError::OutOfRange)
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplyLevel {
    Percent(u8),
    Other,
    Unknown,
    SomeRemaining,
    Invalid,
}

impl SupplyLevel {
    pub fn from_snmp(level: i64, capacity: i64) -> Self {
        match level {
            -1 => Self::Other,
            -2 => Self::Unknown,
            -3 => Self::SomeRemaining,
            0.. if capacity > 0 && level <= capacity => {
                let percent = (i128::from(level) * 100) / i128::from(capacity);
                match u8::try_from(percent) {
                    Ok(value) => Self::Percent(value),
                    Err(_) => Self::Invalid,
                }
            }
            0.. if capacity <= 0 => Self::Unknown,
            _ => Self::Invalid,
        }
    }
}
