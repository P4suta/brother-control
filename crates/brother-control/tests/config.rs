use brother_control::{
    config::{CredentialRef, Fingerprint, Secret},
    types::{Failure, Identity},
};

#[test]
fn headless_credentials_load_only_the_selected_devices_environment() -> Result<(), String> {
    let device = Identity::parse("11111111-1111-4111-8111-111111111111")
        .map_err(|error| error.to_string())?;
    let reference = CredentialRef::environment(device);
    if std::env::var("BROTHERCTL_ENV_FIXTURE").as_deref() == Ok("child") {
        let password = reference.load(device).map_err(|error| error.to_string())?;
        assert!(!format!("{password:?}").contains("fixture-secret"));
        let other = Identity::parse("22222222-2222-4222-8222-222222222222")
            .map_err(|error| error.to_string())?;
        assert!(reference.load(other).is_err());
        assert!(CredentialRef::environment(other).load(other).is_err());
        return Ok(());
    }
    let name = reference
        .environment_name()
        .ok_or_else(|| "missing fixture environment name".to_owned())?;
    let output =
        std::process::Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
            .args([
                "--exact",
                "headless_credentials_load_only_the_selected_devices_environment",
            ])
            .env("BROTHERCTL_ENV_FIXTURE", "child")
            .env(name, "fixture-secret")
            .env_remove("BROTHERCTL_PASSWORD_22222222222242228222222222222222")
            .output()
            .map_err(|error| error.to_string())?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-secret"));
    Ok(())
}

#[test]
fn password_prompts_identify_the_selected_model_and_transport() -> Result<(), Failure> {
    use brother_control::types::{Host, Model, Printer};
    let printer = Printer {
        identity: Identity::parse("11111111-1111-4111-8111-111111111111")?,
        model: Model::MfcL2860dw,
        host: Host::parse("printer.local")?,
    };
    assert_eq!(
        brother_control::setup::password_prompt(&printer),
        "MFC-L2860DW administrator password (printer.local): "
    );
    Ok(())
}

#[test]
fn credentials_are_redacted_and_environment_names_are_device_specific() -> Result<(), Failure> {
    let id = Identity::parse("11111111-1111-4111-8111-111111111111")?;
    assert_eq!(
        CredentialRef::environment(id).environment_name().as_deref(),
        Some("BROTHERCTL_PASSWORD_11111111111141118111111111111111")
    );
    let password = Secret::new("fixture-secret".to_owned())?;
    assert!(!format!("{password:?}").contains("fixture-secret"));
    assert!(Secret::new(String::new()).is_err());
    assert!(Secret::new("line\nbreak".to_owned()).is_err());
    Ok(())
}

#[test]
fn fingerprints_are_exact_sha256_values() -> Result<(), Failure> {
    let fingerprint = Fingerprint::of(b"abc");
    let canonical = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(fingerprint.to_string(), canonical);
    assert_eq!(Fingerprint::parse(canonical)?, fingerprint);
    assert!(Fingerprint::parse("00").is_err());
    assert!(Fingerprint::parse(&"g".repeat(64)).is_err());
    Ok(())
}

#[test]
fn profiles_refuse_overwrite_tampered_certificates_and_other_devices() -> Result<(), Failure> {
    use brother_control::{
        config::{ConfigStore, Profile},
        ipp_adapter::Endpoint,
        types::{Host, Model, Printer},
    };
    let directory = tempfile::tempdir().map_err(|_| {
        Failure::new(
            brother_control::types::FailureKind::Storage,
            "temporary directory failed",
        )
    })?;
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
        certificate_der: b"fixture-public-certificate".to_vec(),
        certificate_sha256: Fingerprint::of(b"fixture-public-certificate"),
        credential: CredentialRef::environment(identity),
    };
    store.save(&profile)?;
    assert_eq!(store.registered()?.len(), 1);
    assert_eq!(store.load(identity)?.printer.identity, identity);
    assert!(store.save(&profile).is_err());
    store.refresh(&profile)?;
    assert_eq!(store.load(identity)?.printer.identity, identity);
    let mut invalid = profile;
    invalid.certificate_der.push(0);
    assert!(invalid.validate(identity).is_err());
    assert!(
        store
            .load(Identity::parse("22222222-2222-4222-8222-222222222222")?)
            .is_err()
    );
    Ok(())
}

#[test]
fn device_leases_exclude_concurrent_sessions_and_release_on_drop() -> Result<(), Failure> {
    use brother_control::{config::ConfigStore, types::FailureKind};
    let directory = tempfile::tempdir()
        .map_err(|_| Failure::new(FailureKind::Storage, "fixture directory failed"))?;
    let store = ConfigStore::new(directory.path().to_path_buf());
    let first = Identity::parse("11111111-1111-4111-8111-111111111111")?;
    let second = Identity::parse("22222222-2222-4222-8222-222222222222")?;
    let lease = store.lease(first)?;
    assert!(matches!(
        store.lease(first),
        Err(Failure {
            kind: FailureKind::Busy,
            ..
        })
    ));
    let _independent = store.lease(second)?;
    drop(lease);
    let _recovered = store.lease(first)?;
    Ok(())
}
