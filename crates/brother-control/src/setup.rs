use crate::{
    config::{ConfigStore, CredentialRef, Fingerprint, Profile, Secret},
    ipp_adapter::{self, Endpoint},
    service::{OperationName, Output, Report},
    types::{Failure, FailureKind, Identity, Observation, Printer, Reading, Source},
    web::{self, WebSession},
};
use serde::Serialize;
use std::{path::PathBuf, time::Duration};

pub enum Registration {
    Native(Secret),
    Environment,
    Reuse,
}

pub fn password_prompt(printer: &Printer) -> String {
    let model = match printer.model {
        crate::types::Model::HlL2375dw => "HL-L2375DW",
        crate::types::Model::MfcL2860dw => "MFC-L2860DW",
    };
    format!("{model} administrator password ({}): ", printer.host)
}

pub struct PreparedSetup {
    profile: Profile,
    timeout: Duration,
}

#[derive(Serialize)]
pub struct SetupReport {
    pub printer: Printer,
    pub certificate_sha256: Fingerprint,
    pub credential: CredentialRef,
    pub profile_path: PathBuf,
    pub credential_saved: bool,
    pub profile_saved: bool,
    pub authentication: Observation<bool>,
}

impl Report for SetupReport {
    fn succeeded(&self) -> bool {
        self.profile_saved
            && match self.credential {
                CredentialRef::Native => self.credential_saved,
                CredentialRef::Environment { .. } => true,
            }
            && !matches!(self.authentication.reading, Reading::Failed { .. })
    }
}

pub fn prepare(
    endpoint: Endpoint,
    web_port: u16,
    trust: Fingerprint,
    timeout: Duration,
) -> Result<PreparedSetup, Failure> {
    let status = ipp_adapter::query(&endpoint, timeout)?;
    let certificate = web::inspect_certificate(endpoint.host(), web_port, timeout)?;
    if certificate.sha256 != trust {
        return Err(Failure::new(
            FailureKind::Certificate,
            "device certificate differs from the explicitly trusted fingerprint",
        ));
    }
    let profile = Profile {
        schema: 1,
        printer: status.printer,
        tls_host: certificate
            .tls_host
            .unwrap_or_else(|| endpoint.host().clone()),
        endpoint,
        web_port,
        certificate_der: certificate.certificate_der,
        certificate_sha256: trust,
        credential: CredentialRef::Native,
    };
    profile.validate(profile.printer.identity)?;
    WebSession::connect(&profile, timeout)?.get(web::status_path(profile.printer.model))?;
    Ok(PreparedSetup { profile, timeout })
}

impl PreparedSetup {
    pub fn printer(&self) -> &Printer {
        &self.profile.printer
    }

    pub fn finish(
        self,
        store: &ConfigStore,
        registration: Registration,
    ) -> Result<Output<SetupReport>, Failure> {
        self.finish_with(store, registration, &mut NativeRegistration)
    }

    fn finish_with<P: RegistrationPort>(
        mut self,
        store: &ConfigStore,
        registration: Registration,
        port: &mut P,
    ) -> Result<Output<SetupReport>, Failure> {
        let exists = store
            .path(self.profile.printer.identity)
            .try_exists()
            .map_err(|_| {
                Failure::new(
                    FailureKind::Storage,
                    "unable to inspect the existing profile",
                )
            })?;
        if exists {
            let existing = store.load(self.profile.printer.identity)?;
            if existing.certificate_sha256 != self.profile.certificate_sha256
                || existing.printer.model != self.profile.printer.model
            {
                return Err(Failure::new(
                    FailureKind::Certificate,
                    "credential refresh cannot change the registered certificate or model",
                ));
            }
        }
        let credential_saved = !matches!(registration, Registration::Environment);
        let password = match registration {
            Registration::Native(password) => {
                self.profile.credential =
                    port.store_credential(self.profile.printer.identity, &password)?;
                Some(password)
            }
            Registration::Environment => {
                self.profile.credential = CredentialRef::environment(self.profile.printer.identity);
                None
            }
            Registration::Reuse => {
                self.profile.credential = store.load(self.profile.printer.identity)?.credential;
                Some(port.load_credential(&self.profile.credential, self.profile.printer.identity)?)
            }
        };
        if exists {
            store.refresh(&self.profile)?;
        } else {
            store.save(&self.profile)?;
        }
        let _lease = store.lease(self.profile.printer.identity)?;
        let authentication = match password {
            Some(password) => match port.authenticate(&self.profile, &password, self.timeout) {
                Ok(()) => Reading::Known { value: true },
                Err(failure) => Reading::Failed { failure },
            },
            None => Reading::Missing {
                reason: "environment reference was saved; authentication has not been attempted"
                    .to_owned(),
            },
        };
        Ok(Output {
            operation: OperationName::Setup,
            result: SetupReport {
                profile_path: store.path(self.profile.printer.identity),
                printer: self.profile.printer,
                certificate_sha256: self.profile.certificate_sha256,
                credential: self.profile.credential,
                credential_saved,
                profile_saved: true,
                authentication: Observation {
                    source: Source::WebManagement,
                    observed_at_unix_seconds: ipp_adapter::now()?,
                    reading: authentication,
                },
            },
        })
    }
}

trait RegistrationPort {
    fn load_credential(
        &mut self,
        reference: &CredentialRef,
        device: Identity,
    ) -> Result<Secret, Failure> {
        reference.load(device)
    }
    fn store_credential(
        &mut self,
        device: Identity,
        password: &Secret,
    ) -> Result<CredentialRef, Failure>;
    fn authenticate(
        &mut self,
        profile: &Profile,
        password: &Secret,
        timeout: Duration,
    ) -> Result<(), Failure>;
}

struct NativeRegistration;
impl RegistrationPort for NativeRegistration {
    fn store_credential(
        &mut self,
        device: Identity,
        password: &Secret,
    ) -> Result<CredentialRef, Failure> {
        CredentialRef::store_native(device, password)
    }
    fn authenticate(
        &mut self,
        profile: &Profile,
        password: &Secret,
        timeout: Duration,
    ) -> Result<(), Failure> {
        WebSession::connect(profile, timeout)?.login(password)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Host, Identity, Model, Observation, Reading, Source};

    #[test]
    fn existing_credentials_move_only_with_the_same_device_model_and_trust() -> Result<(), Failure>
    {
        let directory = tempfile::tempdir()
            .map_err(|_| Failure::new(FailureKind::Storage, "fixture directory failed"))?;
        let store = ConfigStore::new(directory.path().to_path_buf());
        let identity = Identity::parse("11111111-1111-4111-8111-111111111111")?;
        let host = Host::parse("printer.local")?;
        let mut profile = Profile {
            schema: 1,
            printer: Printer {
                identity,
                model: Model::MfcL2860dw,
                host: host.clone(),
            },
            endpoint: Endpoint::new(host.clone(), 631, "/ipp/print")?,
            tls_host: host,
            web_port: 443,
            certificate_der: b"public-fixture".to_vec(),
            certificate_sha256: Fingerprint::of(b"public-fixture"),
            credential: CredentialRef::Native,
        };
        store.save(&profile)?;
        profile.printer.host = Host::parse("fd7a:115c:a1e0:b1a:0:7:a01:110")?;
        profile.endpoint = Endpoint::new(profile.printer.host.clone(), 631, "/ipp/print")?;
        struct Existing;
        impl RegistrationPort for Existing {
            fn store_credential(
                &mut self,
                _: Identity,
                _: &Secret,
            ) -> Result<CredentialRef, Failure> {
                Err(Failure::new(
                    FailureKind::Storage,
                    "reuse must not rewrite the credential",
                ))
            }
            fn load_credential(
                &mut self,
                _: &CredentialRef,
                _: Identity,
            ) -> Result<Secret, Failure> {
                Secret::new("fixture-secret".to_owned())
            }
            fn authenticate(
                &mut self,
                _: &Profile,
                _: &Secret,
                _: Duration,
            ) -> Result<(), Failure> {
                Ok(())
            }
        }
        let output = PreparedSetup {
            profile,
            timeout: Duration::from_secs(15),
        }
        .finish_with(&store, Registration::Reuse, &mut Existing)?;
        assert!(output.result.succeeded());
        assert_eq!(
            store.load(identity)?.printer.host.as_str(),
            "fd7a:115c:a1e0:b1a:0:7:a01:110"
        );
        let mut invalid = store.load(identity)?;
        invalid.certificate_der = b"another-certificate".to_vec();
        invalid.certificate_sha256 = Fingerprint::of(&invalid.certificate_der);
        assert!(matches!(
            PreparedSetup {
                profile: invalid,
                timeout: Duration::from_secs(15)
            }
            .finish_with(&store, Registration::Reuse, &mut Existing),
            Err(Failure {
                kind: FailureKind::Certificate,
                ..
            })
        ));
        let mut invalid = store.load(identity)?;
        invalid.printer.model = Model::HlL2375dw;
        assert!(matches!(
            PreparedSetup {
                profile: invalid,
                timeout: Duration::from_secs(15)
            }
            .finish_with(&store, Registration::Reuse, &mut Existing),
            Err(Failure {
                kind: FailureKind::Certificate,
                ..
            })
        ));
        let output = PreparedSetup {
            profile: store.load(identity)?,
            timeout: Duration::from_secs(15),
        }
        .finish_with(&store, Registration::Environment, &mut Existing)?;
        assert!(!output.result.credential_saved);
        assert!(output.result.profile_saved);
        assert!(output.result.succeeded());
        Ok(())
    }

    struct OfflineLogin {
        stored: bool,
    }
    impl RegistrationPort for OfflineLogin {
        fn store_credential(&mut self, _: Identity, _: &Secret) -> Result<CredentialRef, Failure> {
            self.stored = true;
            Ok(CredentialRef::Native)
        }
        fn authenticate(&mut self, _: &Profile, _: &Secret, _: Duration) -> Result<(), Failure> {
            assert!(self.stored);
            Err(Failure::new(
                FailureKind::Timeout,
                "login fixture timed out",
            ))
        }
    }

    #[test]
    fn failed_login_keeps_the_credential_and_profile_without_exposing_the_password()
    -> Result<(), Failure> {
        let directory = tempfile::tempdir()
            .map_err(|_| Failure::new(FailureKind::Storage, "fixture directory failed"))?;
        let store = ConfigStore::new(directory.path().to_path_buf());
        let identity = Identity::parse("11111111-1111-4111-8111-111111111111")?;
        let host = Host::parse("printer.local")?;
        let profile = Profile {
            schema: 1,
            printer: Printer {
                identity,
                model: Model::MfcL2860dw,
                host: host.clone(),
            },
            endpoint: Endpoint::new(host.clone(), 631, "/ipp/print")?,
            tls_host: host,
            web_port: 443,
            certificate_der: b"public-fixture".to_vec(),
            certificate_sha256: Fingerprint::of(b"public-fixture"),
            credential: CredentialRef::Native,
        };
        let prepared = PreparedSetup {
            profile,
            timeout: Duration::from_secs(15),
        };
        let output = prepared.finish_with(
            &store,
            Registration::Native(Secret::new("fixture-secret".to_owned())?),
            &mut OfflineLogin { stored: false },
        )?;
        assert!(store.path(identity).is_file());
        assert!(output.result.credential_saved);
        assert!(matches!(
            output.result.authentication,
            Observation {
                source: Source::WebManagement,
                reading: Reading::Failed { .. },
                ..
            }
        ));
        assert!(!output.result.succeeded());
        assert!(
            !serde_json::to_string(&output)
                .map_err(|_| Failure::new(FailureKind::Storage, "fixture encoding failed"))?
                .contains("fixture-secret")
        );
        Ok(())
    }
}
