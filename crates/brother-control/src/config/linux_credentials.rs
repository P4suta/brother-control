use super::{Failure, unavailable_credential};

#[derive(Clone, Copy)]
pub(super) enum Access {
    Normal,
    Interactive,
}

impl Access {
    fn prompt_timeout(self) -> u64 {
        match self {
            Self::Normal => 0,
            Self::Interactive => 60,
        }
    }
}

fn select<T>(unlocked: Vec<T>, locked: Vec<T>, access: Access) -> Result<T, Failure> {
    if unlocked.len() + locked.len() != 1 || matches!(access, Access::Normal) && !locked.is_empty()
    {
        return Err(unavailable_credential());
    }
    unlocked
        .into_iter()
        .chain(locked)
        .next()
        .ok_or_else(unavailable_credential)
}

#[cfg(target_os = "linux")]
pub(super) fn load(device: super::Identity, access: Access) -> Result<String, Failure> {
    use dbus_secret_service::{EncryptionType, SecretService};
    let service =
        SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, access.prompt_timeout())
            .map_err(|_| unavailable_credential())?;
    let account = device.to_string();
    let found = service
        .search_items(std::collections::HashMap::from([
            ("service", "brother-control"),
            ("username", account.as_str()),
        ]))
        .map_err(|_| unavailable_credential())?;
    let item = select(found.unlocked, found.locked, access)?;
    if matches!(access, Access::Interactive) {
        item.unlock().map_err(|_| unavailable_credential())?;
    }
    String::from_utf8(item.get_secret().map_err(|_| unavailable_credential())?)
        .map_err(|_| unavailable_credential())
}

#[cfg(target_os = "linux")]
pub(super) fn store(device: super::Identity, password: &super::Secret) -> Result<(), Failure> {
    use dbus_secret_service::{EncryptionType, SecretService};
    let failed = || {
        Failure::new(
            super::FailureKind::Storage,
            "unable to store the native credential",
        )
    };
    let service = SecretService::connect_with_max_prompt_timeout(
        EncryptionType::Dh,
        Access::Interactive.prompt_timeout(),
    )
    .map_err(|_| failed())?;
    let account = device.to_string();
    let attributes = std::collections::HashMap::from([
        ("service", "brother-control"),
        ("username", account.as_str()),
    ]);
    let found = service
        .search_items(attributes.clone())
        .map_err(|_| failed())?;
    if found.unlocked.is_empty() && found.locked.is_empty() {
        let collection = service.get_default_collection().map_err(|_| failed())?;
        collection.unlock().map_err(|_| failed())?;
        collection
            .create_item(
                "Brother printer administrator",
                attributes,
                password.expose().as_bytes(),
                false,
                "text/plain",
            )
            .map_err(|_| failed())?;
    } else {
        let item = select(found.unlocked, found.locked, Access::Interactive)?;
        item.unlock().map_err(|_| failed())?;
        item.set_secret(password.expose().as_bytes(), "text/plain")
            .map_err(|_| failed())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_reads_require_one_unlocked_credential_without_prompting() {
        assert_eq!(Access::Normal.prompt_timeout(), 0);
        assert_eq!(Access::Interactive.prompt_timeout(), 60);
        assert_eq!(
            select(Vec::from([1]), Vec::new(), Access::Normal).ok(),
            Some(1)
        );
        assert!(select::<u8>(Vec::new(), Vec::new(), Access::Normal).is_err());
        assert!(select(Vec::new(), Vec::from([1]), Access::Normal).is_err());
        assert!(select(Vec::from([1]), Vec::from([2]), Access::Normal).is_err());
        assert!(select(Vec::from([1, 2]), Vec::new(), Access::Interactive).is_err());
        assert_eq!(
            select(Vec::new(), Vec::from([1]), Access::Interactive).ok(),
            Some(1)
        );
    }
}
