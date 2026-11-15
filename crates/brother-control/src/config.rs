use crate::{
    ipp_adapter::Endpoint,
    types::{Failure, FailureKind, Host, Identity, Printer},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[cfg(any(target_os = "linux", test))]
mod linux_credentials;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: u16,
    pub printer: Printer,
    pub endpoint: Endpoint,
    pub tls_host: Host,
    pub web_port: u16,
    pub certificate_der: Vec<u8>,
    pub certificate_sha256: Fingerprint,
    pub credential: CredentialRef,
}

impl Profile {
    pub fn validate(&self, device: Identity) -> Result<(), Failure> {
        if self.schema != 1
            || self.web_port == 0
            || self.certificate_der.is_empty()
            || self.certificate_der.len() > 65_536
        {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "invalid device profile",
            ));
        }
        if self.printer.identity != device || self.printer.host != *self.endpoint.host() {
            return Err(Failure::new(
                FailureKind::WrongDevice,
                "profile identifies another device or endpoint",
            ));
        }
        if self.certificate_sha256 != Fingerprint::of(&self.certificate_der) {
            return Err(Failure::new(
                FailureKind::Certificate,
                "profile certificate does not match its fingerprint",
            ));
        }
        match self.credential {
            CredentialRef::Environment { device: expected } if expected != device => {
                Err(Failure::new(
                    FailureKind::WrongDevice,
                    "credential reference identifies another device",
                ))
            }
            CredentialRef::Environment { .. } | CredentialRef::Native => Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    root: PathBuf,
}

pub struct DeviceLease {
    file: fs::File,
}

impl Drop for DeviceLease {
    fn drop(&mut self) {
        if let Err(error) = self.file.unlock() {
            eprintln!("device lease unlock failed: {error}");
        }
    }
}

#[cfg(test)]
mod lease_tests {
    use super::*;

    #[test]
    fn dropping_a_lease_releases_a_duplicated_file_handle() -> Result<(), Failure> {
        let directory = tempfile::tempdir().map_err(storage_failure)?;
        let store = ConfigStore::new(directory.path().to_path_buf());
        let device = Identity::parse("11111111-1111-4111-8111-111111111111")?;
        let lease = store.lease(device)?;
        let inherited = lease.file.try_clone().map_err(storage_failure)?;
        drop(lease);
        let recovered = store.lease(device)?;
        drop(inherited);
        assert!(matches!(
            store.lease(device),
            Err(Failure {
                kind: FailureKind::Busy,
                ..
            })
        ));
        drop(recovered);
        let _available = store.lease(device)?;
        Ok(())
    }
}

fn storage_failure(_: std::io::Error) -> Failure {
    Failure::new(FailureKind::Storage, "unable to access the device profile")
}

impl ConfigStore {
    pub fn registered(&self) -> Result<Vec<Profile>, Failure> {
        if !self.root.try_exists().map_err(storage_failure)? {
            return Ok(Vec::new());
        }
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(storage_failure)? {
            let path = entry.map_err(storage_failure)?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            if profiles.len() >= 256 {
                return Err(Failure::new(
                    FailureKind::Storage,
                    "registered profile count exceeds the limit",
                ));
            }
            let identity = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    Failure::new(FailureKind::Storage, "invalid registered profile name")
                })?;
            profiles.push(self.load(Identity::parse(identity)?)?);
        }
        profiles.sort_by_key(|profile| profile.printer.identity.to_string());
        Ok(profiles)
    }
    pub fn lease(&self, device: Identity) -> Result<DeviceLease, Failure> {
        let path = self.root.join(format!("{}.lock", device.compact()));
        if fs::symlink_metadata(&path).is_ok_and(|metadata| !metadata.is_file()) {
            return Err(Failure::new(
                FailureKind::Storage,
                "device lock must be a regular file",
            ));
        }
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(storage_failure)?;
        match file.try_lock() {
            Ok(()) => Ok(DeviceLease { file }),
            Err(fs::TryLockError::WouldBlock) => Err(Failure::new(
                FailureKind::Busy,
                "another management operation is using the device",
            )),
            Err(fs::TryLockError::Error(error)) => Err(storage_failure(error)),
        }
    }
    pub fn refresh(&self, profile: &Profile) -> Result<(), Failure> {
        profile.validate(profile.printer.identity)?;
        let existing = self.load(profile.printer.identity)?;
        if existing.certificate_sha256 != profile.certificate_sha256
            || existing.printer.model != profile.printer.model
        {
            return Err(Failure::new(
                FailureKind::Certificate,
                "credential refresh cannot replace the registered model or certificate trust",
            ));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root).map_err(storage_failure)?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), profile).map_err(|_| {
            Failure::new(
                FailureKind::Storage,
                "unable to encode the refreshed profile",
            )
        })?;
        temporary.as_file().sync_all().map_err(storage_failure)?;
        temporary
            .persist(self.path(profile.printer.identity))
            .map_err(|_| {
                Failure::new(
                    FailureKind::Storage,
                    "unable to replace the refreshed profile",
                )
            })?;
        Ok(())
    }
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn platform() -> Result<Self, Failure> {
        let directories =
            directories::ProjectDirs::from("", "", "brother-control").ok_or_else(|| {
                Failure::new(
                    FailureKind::Storage,
                    "platform configuration directory is unavailable",
                )
            })?;
        Ok(Self::new(directories.config_dir().to_path_buf()))
    }

    pub fn path(&self, device: Identity) -> PathBuf {
        self.root.join(format!("{}.json", device.compact()))
    }

    pub fn load(&self, device: Identity) -> Result<Profile, Failure> {
        let path = self.path(device);
        if !fs::symlink_metadata(&path)
            .map_err(storage_failure)?
            .is_file()
        {
            return Err(Failure::new(
                FailureKind::Storage,
                "profile must be a regular file",
            ));
        }
        let mut input = Vec::new();
        fs::File::open(path)
            .map_err(storage_failure)?
            .take(262_145)
            .read_to_end(&mut input)
            .map_err(storage_failure)?;
        if input.len() > 262_144 {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "profile exceeds the size limit",
            ));
        }
        let profile: Profile = serde_json::from_slice(&input)
            .map_err(|_| Failure::new(FailureKind::InvalidInput, "invalid device profile JSON"))?;
        profile.validate(device)?;
        Ok(profile)
    }

    pub fn save(&self, profile: &Profile) -> Result<(), Failure> {
        profile.validate(profile.printer.identity)?;
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(&self.root).map_err(storage_failure)?;
        if fs::symlink_metadata(&self.root)
            .map_err(storage_failure)?
            .file_type()
            .is_symlink()
        {
            return Err(Failure::new(
                FailureKind::Storage,
                "configuration directory must not be a symlink",
            ));
        }
        let bytes = serde_json::to_vec_pretty(profile).map_err(|_| {
            Failure::new(FailureKind::Storage, "unable to encode the device profile")
        })?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options
            .open(self.path(profile.printer.identity))
            .map_err(storage_failure)?;
        output
            .write_all(&bytes)
            .and_then(|()| output.sync_all())
            .map_err(storage_failure)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn parse(input: &str) -> Result<Self, Failure> {
        let normalized: String = input
            .chars()
            .filter(|character| *character != ':')
            .collect();
        if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "expected a SHA-256 certificate fingerprint",
            ));
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte =
                u8::from_str_radix(&normalized[index * 2..index * 2 + 2], 16).map_err(|_| {
                    Failure::new(FailureKind::InvalidInput, "invalid certificate fingerprint")
                })?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Fingerprint {
    type Err = Failure;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl TryFrom<String> for Fingerprint {
    type Error = Failure;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Fingerprint> for String {
    fn from(value: Fingerprint) -> Self {
        value.to_string()
    }
}

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Result<Self, Failure> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(Failure::new(
                FailureKind::InvalidInput,
                "password must contain 1 through 256 bytes without control characters",
            ));
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret([redacted])")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialRef {
    Native,
    Environment { device: Identity },
}

impl CredentialRef {
    pub fn authorize_native(&self, device: Identity) -> Result<(), Failure> {
        match self {
            Self::Native => {
                let password = authorize_native(device)?;
                let _secret = Secret::new(password)?;
                Ok(())
            }
            Self::Environment { .. } => Err(Failure::new(
                FailureKind::Unsupported,
                "environment references do not require native credential authorization",
            )),
        }
    }
    pub fn environment(device: Identity) -> Self {
        Self::Environment { device }
    }

    pub fn environment_name(&self) -> Option<String> {
        match self {
            Self::Native => None,
            Self::Environment { device } => Some(format!(
                "BROTHERCTL_PASSWORD_{}",
                device.compact().to_ascii_uppercase()
            )),
        }
    }

    pub fn load(&self, device: Identity) -> Result<Secret, Failure> {
        let value = match self {
            Self::Native => load_native(device)?,
            Self::Environment { device: expected } => {
                if *expected != device {
                    return Err(Failure::new(
                        FailureKind::WrongDevice,
                        "credential reference belongs to another device",
                    ));
                }
                let name = self.environment_name().ok_or_else(|| {
                    Failure::new(FailureKind::InvalidInput, "missing credential reference")
                })?;
                std::env::var(name).map_err(|_| {
                    Failure::new(
                        FailureKind::Authentication,
                        "per-device password environment variable is unavailable",
                    )
                })?
            }
        };
        Secret::new(value)
    }

    pub fn store_native(device: Identity, password: &Secret) -> Result<Self, Failure> {
        store_native(device, password)?;
        Ok(Self::Native)
    }
}

fn unavailable_credential() -> Failure {
    Failure::new(
        FailureKind::Authentication,
        "native credential is unavailable without interaction; run brotherctl credentials authorize, or setup if the credential is missing",
    )
}

#[cfg(all(test, target_os = "macos"))]
mod native_interaction_tests {
    use super::*;
    #[test]
    fn native_reads_disable_keychain_dialogs_for_the_process_lifetime() -> Result<(), Failure> {
        deny_native_interaction()?;
        let allowed =
            security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed()
                .map_err(|_| unavailable_credential())?;
        assert!(!allowed);
        deny_native_interaction()?;
        assert!(
            !security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed()
                .map_err(|_| unavailable_credential())?
        );
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    assert!(deny_native_interaction().is_ok());
                    assert_eq!(security_framework::os::macos::keychain::SecKeychain::user_interaction_allowed().ok(), Some(false));
                });
            }
        });
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn deny_native_interaction() -> Result<(), Failure> {
    use security_framework::os::macos::keychain::{KeychainUserInteractionLock, SecKeychain};
    static POLICY: std::sync::OnceLock<Result<KeychainUserInteractionLock, Failure>> =
        std::sync::OnceLock::new();
    POLICY
        .get_or_init(|| {
            SecKeychain::disable_user_interaction().map_err(|_| unavailable_credential())
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
}

#[cfg(target_os = "macos")]
fn load_native(device: Identity) -> Result<String, Failure> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    deny_native_interaction()?;
    let mut search = ItemSearchOptions::new();
    search
        .class(ItemClass::generic_password())
        .service("brother-control")
        .account(&device.to_string())
        .load_data(true)
        .skip_authenticated_items(true)
        .limit(1);
    let results = search.search().map_err(|_| unavailable_credential())?;
    match results.into_iter().next() {
        Some(SearchResult::Data(bytes)) => {
            String::from_utf8(bytes).map_err(|_| unavailable_credential())
        }
        Some(SearchResult::Ref(_) | SearchResult::Dict(_) | SearchResult::Other) | None => {
            Err(unavailable_credential())
        }
    }
}

#[cfg(target_os = "windows")]
fn load_native(device: Identity) -> Result<String, Failure> {
    keyring::Entry::new("brother-control", &device.to_string())
        .and_then(|entry| entry.get_password())
        .map_err(|_| unavailable_credential())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn authorize_native(device: Identity) -> Result<String, Failure> {
    keyring::Entry::new("brother-control", &device.to_string())
        .and_then(|entry| entry.get_password())
        .map_err(|_| unavailable_credential())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn store_native(device: Identity, password: &Secret) -> Result<(), Failure> {
    keyring::Entry::new("brother-control", &device.to_string())
        .and_then(|entry| entry.set_password(password.expose()))
        .map_err(|_| {
            Failure::new(
                FailureKind::Storage,
                "unable to store the native credential",
            )
        })
}

#[cfg(target_os = "linux")]
fn load_native(device: Identity) -> Result<String, Failure> {
    linux_credentials::load(device, linux_credentials::Access::Normal)
}

#[cfg(target_os = "linux")]
fn authorize_native(device: Identity) -> Result<String, Failure> {
    linux_credentials::load(device, linux_credentials::Access::Interactive)
}

#[cfg(target_os = "linux")]
fn store_native(device: Identity, password: &Secret) -> Result<(), Failure> {
    linux_credentials::store(device, password)
}
