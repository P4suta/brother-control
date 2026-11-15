use crate::{
    config::{ConfigStore, Fingerprint},
    ipp_adapter::{self, Endpoint},
    service::{self, OperationName, Output, Report, StatusReport},
    types::{Failure, Observation, Reading, Source},
    web::{self, WebSession},
};
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
pub struct DiagnosticReport {
    pub status: StatusReport,
    pub certificate: Observation<Fingerprint>,
    pub authentication: Observation<bool>,
}

impl Report for DiagnosticReport {
    fn succeeded(&self) -> bool {
        self.status.succeeded()
            && matches!(self.certificate.reading, Reading::Known { .. })
            && !matches!(self.authentication.reading, Reading::Failed { .. })
    }
}

pub fn run(
    store: &ConfigStore,
    endpoint: &Endpoint,
    timeout: Duration,
) -> Result<Output<DiagnosticReport>, Failure> {
    let status = service::status(endpoint, timeout)?.result;
    let certificate = match web::inspect_certificate(endpoint.host(), 443, timeout) {
        Ok(certificate) => Reading::Known {
            value: certificate.sha256,
        },
        Err(failure) => Reading::Failed { failure },
    };
    let authentication = if let Reading::Known { value } = &status.ipp.reading {
        if store
            .path(value.printer.identity)
            .try_exists()
            .map_err(|_| {
                Failure::new(
                    crate::types::FailureKind::Storage,
                    "unable to inspect the device profile",
                )
            })?
        {
            let login = store.load(value.printer.identity).and_then(|profile| {
                let _lease = store.lease(profile.printer.identity)?;
                let password = profile.credential.load(profile.printer.identity)?;
                WebSession::connect(&profile, timeout)?.login(&password)?;
                Ok(true)
            });
            match login {
                Ok(value) => Reading::Known { value },
                Err(failure) => Reading::Failed { failure },
            }
        } else {
            Reading::Missing {
                reason:
                    "device trust and credential reference are not registered; run brotherctl setup"
                        .to_owned(),
            }
        }
    } else {
        Reading::Missing {
            reason: "printer identity is unavailable; authentication was not attempted".to_owned(),
        }
    };
    let at = ipp_adapter::now()?;
    Ok(Output {
        operation: OperationName::Diagnose,
        result: DiagnosticReport {
            status,
            certificate: Observation {
                source: Source::WebManagement,
                observed_at_unix_seconds: at,
                reading: certificate,
            },
            authentication: Observation {
                source: Source::WebManagement,
                observed_at_unix_seconds: at,
                reading: authentication,
            },
        },
    })
}
