use crate::{
    config::{DeviceLease, Profile},
    ipp_adapter::{self, IppStatus},
    settings::{KEYS, SettingObservation, SettingsPort, Snapshot},
    types::{Failure, FailureKind, Observation, Reading, Source},
    web::WebSession,
    web_settings::{SettingsPage, parse_page, settings_links},
};
use std::time::Duration;

pub struct Device {
    _lease: DeviceLease,
    profile: Profile,
    session: WebSession,
    timeout: Duration,
    home: String,
    pages: Vec<SettingsPage>,
}

impl Device {
    pub fn open(profile: Profile, timeout: Duration, lease: DeviceLease) -> Result<Self, Failure> {
        profile.validate(profile.printer.identity)?;
        let session = WebSession::connect(&profile, timeout)?;
        let mut device = Self {
            _lease: lease,
            profile,
            session,
            timeout,
            home: String::new(),
            pages: Vec::new(),
        };
        device.identify()?;
        let password = device
            .profile
            .credential
            .load(device.profile.printer.identity)?;
        device.home = device.session.login(&password)?;
        Ok(device)
    }

    pub fn identify(&self) -> Result<IppStatus, Failure> {
        let status = ipp_adapter::query(&self.profile.endpoint, self.timeout)?;
        if status.printer.identity != self.profile.printer.identity
            || status.printer.model != self.profile.printer.model
        {
            return Err(Failure::new(
                FailureKind::WrongDevice,
                "another printer answered at the configured endpoint",
            ));
        }
        Ok(status)
    }

    pub fn firmware_components(&self) -> Result<Vec<crate::firmware::ComponentVersion>, Failure> {
        crate::firmware::installed_components(&self.session.get("/general/information.html")?)
    }
}

impl SettingsPort for Device {
    fn read(&mut self) -> Result<Snapshot, Failure> {
        let status = self.identify()?;
        self.home = self
            .session
            .get(crate::web::status_path(self.profile.printer.model))?;
        let mut paths = settings_links(&self.home)?;
        let mut pages = Vec::new();
        let mut failure = None;
        let mut visited = std::collections::BTreeSet::new();
        while let Some(path) = paths.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            if visited.len() > 32 {
                return Err(Failure::new(
                    FailureKind::UnknownResponse,
                    "management navigation exceeds the page limit",
                ));
            }
            match self.session.get(&path) {
                Ok(document) => {
                    for link in settings_links(&document)? {
                        if !visited.contains(&link) {
                            paths.push(link);
                        }
                    }
                    match parse_page(self.profile.printer.model, &path, &document) {
                        Ok(page) => pages.push(page),
                        Err(error) => {
                            failure = Some(error);
                        }
                    }
                }
                Err(error) => {
                    failure = Some(error);
                }
            }
        }
        let at = ipp_adapter::now()?;
        let mut values = Vec::new();
        for key in KEYS {
            let mut matching = pages
                .iter()
                .flat_map(SettingsPage::values)
                .filter(|value| value.key() == key);
            let reading = match matching.next() {
                Some(value) if matching.next().is_none() => Reading::Known { value },
                Some(_) => Reading::Unknown {
                    reason: "setting appears in multiple management forms".to_owned(),
                },
                None => match &failure {
                    Some(failure) => Reading::Failed {
                        failure: failure.clone(),
                    },
                    None => Reading::Unsupported {
                        reason: "setting is absent from recognized management pages".to_owned(),
                    },
                },
            };
            values.push(SettingObservation {
                key,
                observation: Observation {
                    source: Source::WebManagement,
                    observed_at_unix_seconds: at,
                    reading,
                },
            });
        }
        self.pages = pages;
        Ok(Snapshot {
            device: status.printer.identity,
            values,
        })
    }

    fn write(&mut self, change: brother_core::VerifiedChange) -> Result<(), Failure> {
        if change.device() != self.profile.printer.identity.core()? {
            return Err(Failure::new(
                FailureKind::WrongDevice,
                "verified change identifies another printer",
            ));
        }
        let desired = change.desired().into();
        let page = self
            .pages
            .iter()
            .find(|page| {
                page.values()
                    .iter()
                    .any(|value| value.key() == change.desired().key())
            })
            .ok_or_else(|| {
                Failure::new(
                    FailureKind::Unsupported,
                    "no recognized form exists for the verified setting",
                )
            })?;
        let submission = page.submission(desired)?;
        self.session
            .submit(submission.path(), submission.fields())?;
        Ok(())
    }
}
