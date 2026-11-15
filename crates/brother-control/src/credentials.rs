use crate::{
    config::{ConfigStore, CredentialRef},
    ipp_adapter,
    service::{OperationName, Output, Report},
    types::{Failure, FailureKind, Identity, Observation, Printer, Reading, Source},
};
use serde::Serialize;

#[derive(Serialize)]
pub struct Authorization {
    pub printer: Printer,
    pub access: Observation<bool>,
}

#[derive(Serialize)]
pub struct AuthorizationReport {
    pub authorizations: Vec<Authorization>,
}

impl Report for AuthorizationReport {
    fn succeeded(&self) -> bool {
        !self.authorizations.is_empty()
            && self
                .authorizations
                .iter()
                .all(|result| matches!(result.access.reading, Reading::Known { value: true }))
    }
}

pub fn authorize(
    store: &ConfigStore,
    selected: &[Identity],
) -> Result<Output<AuthorizationReport>, Failure> {
    if selected.len() > 256
        || selected
            .iter()
            .enumerate()
            .any(|(index, device)| selected[..index].contains(device))
    {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "credential selection exceeds the limit or repeats a device",
        ));
    }
    let profiles = if selected.is_empty() {
        store.registered()?
    } else {
        selected
            .iter()
            .map(|device| store.load(*device))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut authorizations = Vec::new();
    for profile in profiles {
        if matches!(profile.credential, CredentialRef::Environment { .. }) {
            continue;
        }
        let access = match profile
            .credential
            .authorize_native(profile.printer.identity)
        {
            Ok(()) => Reading::Known { value: true },
            Err(failure) => Reading::Failed { failure },
        };
        authorizations.push(Authorization {
            printer: profile.printer,
            access: Observation {
                source: Source::NativeCredentialStore,
                observed_at_unix_seconds: ipp_adapter::now()?,
                reading: access,
            },
        });
    }
    if authorizations.is_empty() {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "no registered native credentials are selected",
        ));
    }
    Ok(Output {
        operation: OperationName::AuthorizeCredentials,
        result: AuthorizationReport { authorizations },
    })
}
