use super::CheckError;
use std::{fs, path::Path, process::Command};

fn scan(directory: &Path) -> Result<bool, CheckError> {
    let output = Command::new("gitleaks")
        .args([
            "dir",
            "--redact",
            "--no-banner",
            "--log-level",
            "error",
            "--ignore-gitleaks-allow",
        ])
        .arg(directory)
        .output()
        .map_err(|error| CheckError(format!("secret scanner: {error}")))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(CheckError(format!(
            "secret scanner failed: {}",
            output.status
        ))),
    }
}

pub(super) fn check() -> Result<(), CheckError> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| CheckError(error.to_string()))?;
    if !output.status.success() {
        return Err(CheckError(
            "unable to enumerate source files for secret scanning".to_owned(),
        ));
    }
    let files =
        std::str::from_utf8(&output.stdout).map_err(|error| CheckError(error.to_string()))?;
    let snapshot = tempfile::tempdir().map_err(|error| CheckError(error.to_string()))?;
    let mut copied = 0;
    for filename in files.split('\0').filter(|filename| !filename.is_empty()) {
        let path = Path::new(filename);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(CheckError(
                "source filename is outside the repository".to_owned(),
            ));
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CheckError(error.to_string())),
        };
        if !metadata.is_file() {
            return Err(CheckError(format!(
                "source must be a regular file: {filename}"
            )));
        }
        let target = snapshot.path().join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| CheckError(error.to_string()))?;
        }
        fs::copy(path, &target).map_err(|error| CheckError(error.to_string()))?;
        copied += 1;
    }
    if copied == 0 || !scan(snapshot.path())? {
        return Err(CheckError(
            "secret scan requires nonempty source without detected credentials".to_owned(),
        ));
    }
    println!("Secret scan: {copied} source files passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_scan_rejects_tokens_without_printing_them() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let file = directory.path().join("settings.txt");
        std::fs::write(&file, "public configuration").map_err(|error| error.to_string())?;
        assert!(scan(directory.path()).map_err(|error| error.to_string())?);
        let token = ["ghp_", "AbC7deF2GhI8jkL3MnO9pqR4StU0vwX5YzA6"].concat();
        std::fs::write(&file, format!("token = {token}\n")).map_err(|error| error.to_string())?;
        assert!(!scan(directory.path()).map_err(|error| error.to_string())?);
        Ok(())
    }
}
