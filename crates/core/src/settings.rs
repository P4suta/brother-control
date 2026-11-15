use crate::{DeviceId, Minutes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Off,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplex {
    Off,
    LongEdge,
    ShortEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperSize {
    A4,
    A5,
    A6,
    Letter,
    Legal,
    Executive,
    B5,
    B6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperType {
    Plain,
    Thin,
    Thick,
    Thicker,
    Recycled,
    Bond,
    Label,
    Envelope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKey {
    Sleep,
    TonerSave,
    QuietMode,
    Duplex,
    PaperSize,
    PaperType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Sleep(Minutes),
    TonerSave(Switch),
    QuietMode(Switch),
    Duplex(Duplex),
    PaperSize(PaperSize),
    PaperType(PaperType),
}

impl Setting {
    pub const fn key(self) -> SettingKey {
        match self {
            Self::Sleep(_) => SettingKey::Sleep,
            Self::TonerSave(_) => SettingKey::TonerSave,
            Self::QuietMode(_) => SettingKey::QuietMode,
            Self::Duplex(_) => SettingKey::Duplex,
            Self::PaperSize(_) => SettingKey::PaperSize,
            Self::PaperType(_) => SettingKey::PaperType,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeError {
    DifferentSetting,
    WrongDevice,
    Stale,
}

impl core::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DifferentSetting => {
                f.write_str("before and desired settings must have the same key")
            }
            Self::WrongDevice => f.write_str("device identity changed"),
            Self::Stale => f.write_str("current value differs from the change's expected value"),
        }
    }
}

impl core::error::Error for ChangeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    device: DeviceId,
    before: Setting,
    desired: Setting,
}

impl Change {
    pub fn new(device: DeviceId, before: Setting, desired: Setting) -> Result<Self, ChangeError> {
        if before.key() != desired.key() {
            return Err(ChangeError::DifferentSetting);
        }
        Ok(Self {
            device,
            before,
            desired,
        })
    }

    pub fn verify(
        &self,
        device: DeviceId,
        current: Setting,
    ) -> Result<VerifiedChange, ChangeError> {
        if self.device != device {
            return Err(ChangeError::WrongDevice);
        }
        if self.before != current {
            return Err(ChangeError::Stale);
        }
        Ok(VerifiedChange {
            device,
            desired: self.desired,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedChange {
    device: DeviceId,
    desired: Setting,
}

impl VerifiedChange {
    pub const fn device(&self) -> DeviceId {
        self.device
    }

    pub const fn desired(&self) -> Setting {
        self.desired
    }
}
