use super::CheckError;
use std::path::Path;

#[cfg(any(target_os = "macos", test))]
struct SigningIdentity(String);

#[cfg(any(target_os = "macos", test))]
impl SigningIdentity {
    fn parse(value: &str) -> Result<Self, CheckError> {
        if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CheckError(
                "BROTHERCTL_SIGNING_IDENTITY must be the exact 40-digit certificate selector"
                    .to_owned(),
            ));
        }
        Ok(Self(value.to_ascii_uppercase()))
    }
}

#[cfg(any(target_os = "macos", test))]
fn publish<F: FnOnce(&Path, &SigningIdentity) -> Result<(), CheckError>>(
    source: &Path,
    destination: &Path,
    identity: &SigningIdentity,
    sign: F,
) -> Result<(), CheckError> {
    if std::fs::symlink_metadata(destination).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(CheckError(
            "installation target must be a regular file".to_owned(),
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| CheckError("installation target needs a parent directory".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(|error| CheckError(error.to_string()))?;
    let staged =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| CheckError(error.to_string()))?;
    std::fs::copy(source, staged.path()).map_err(|error| CheckError(error.to_string()))?;
    sign(staged.path(), identity)?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| CheckError(error.to_string()))?;
    staged
        .persist(destination)
        .map_err(|error| CheckError(error.to_string()))?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn run(root: &Path) -> Result<(), CheckError> {
    use std::process::Command;
    let identity = SigningIdentity::parse(&std::env::var("BROTHERCTL_SIGNING_IDENTITY")
        .map_err(|_| CheckError("set BROTHERCTL_SIGNING_IDENTITY in machine-local configuration before installation".to_owned()))?)?;
    let build = tempfile::tempdir().map_err(|error| CheckError(error.to_string()))?;
    let status = Command::new("cargo")
        .args([
            "install",
            "--path",
            "crates/brother-control",
            "--locked",
            "--root",
        ])
        .arg(build.path())
        .status()
        .map_err(|error| CheckError(error.to_string()))?;
    if !status.success() {
        return Err(CheckError(format!("candidate build failed: {status}")));
    }
    let destination = root.join("bin/brotherctl");
    publish(
        &build.path().join("bin/brotherctl"),
        &destination,
        &identity,
        |path, identity| {
            let signed = Command::new("codesign")
                .args([
                    "--force",
                    "--sign",
                    &identity.0,
                    "--identifier",
                    "com.P4suta.brother-control",
                    "--options",
                    "runtime",
                    "--timestamp",
                ])
                .arg(path)
                .status()
                .map_err(|error| CheckError(error.to_string()))?;
            if !signed.success() {
                return Err(CheckError(format!("candidate signing failed: {signed}")));
            }
            let verified = Command::new("codesign")
                .args(["--verify", "--strict"])
                .arg(path)
                .status()
                .map_err(|error| CheckError(error.to_string()))?;
            if !verified.success() {
                return Err(CheckError(format!(
                    "candidate signature verification failed: {verified}"
                )));
            }
            Ok(())
        },
    )?;
    println!("Installed signed CLI: {}", destination.display());
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn run(_: &Path) -> Result<(), CheckError> {
    Err(CheckError(
        "signed installation requires macOS; use cargo install on other platforms".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_failure_preserves_the_installed_binary_and_cleans_staging() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let source = directory.path().join("candidate");
        let destination = directory.path().join("brotherctl");
        std::fs::write(&source, b"candidate").map_err(|error| error.to_string())?;
        std::fs::write(&destination, b"installed").map_err(|error| error.to_string())?;
        let identity = SigningIdentity::parse("0123456789abcdef0123456789abcdef01234567")
            .map_err(|error| error.to_string())?;
        assert_eq!(identity.0, "0123456789ABCDEF0123456789ABCDEF01234567");
        let failed = publish(&source, &destination, &identity, |_, _| {
            Err(super::super::CheckError(
                "injected signing failure".to_owned(),
            ))
        });
        assert!(failed.is_err());
        assert_eq!(
            std::fs::read(&destination).map_err(|error| error.to_string())?,
            b"installed"
        );
        assert_eq!(
            std::fs::read_dir(directory.path())
                .map_err(|error| error.to_string())?
                .count(),
            2
        );
        publish(&source, &destination, &identity, |path, _| {
            assert_eq!(
                std::fs::read(path).map_err(|error| super::super::CheckError(error.to_string()))?,
                b"candidate"
            );
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        assert_eq!(
            std::fs::read(&destination).map_err(|error| error.to_string())?,
            b"candidate"
        );
        assert!(SigningIdentity::parse("Developer ID Application").is_err());
        assert!(SigningIdentity::parse(&"0".repeat(39)).is_err());
        assert!(SigningIdentity::parse(&"g".repeat(40)).is_err());
        Ok(())
    }
}
