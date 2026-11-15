use brother_control::{
    config::ConfigStore,
    credentials,
    types::{Failure, FailureKind, Identity},
};

#[test]
fn authorization_rejects_empty_and_duplicate_selections_before_native_access() -> Result<(), Failure>
{
    let directory = tempfile::tempdir()
        .map_err(|_| Failure::new(FailureKind::Storage, "fixture directory failed"))?;
    let store = ConfigStore::new(directory.path().to_path_buf());
    assert!(matches!(
        credentials::authorize(&store, &[]),
        Err(Failure {
            kind: FailureKind::InvalidInput,
            ..
        })
    ));
    let device = Identity::parse("11111111-1111-4111-8111-111111111111")?;
    assert!(matches!(
        credentials::authorize(&store, &[device, device]),
        Err(Failure {
            kind: FailureKind::InvalidInput,
            ..
        })
    ));
    Ok(())
}
