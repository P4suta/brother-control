use super::{CheckError, run};
use std::{path::Path, process::Command};

const ORIGIN: &str = "https://github.com/RustSec/advisory-db";
const CACHE: &str = ".local/advisories/advisory-db-3157b0e258782691";

fn git_text(path: &Path, arguments: &[&str]) -> Result<String, CheckError> {
    let output = Command::new("git")
        .current_dir(path)
        .args(arguments)
        .output()
        .map_err(|error| CheckError(format!("advisory Git command: {error}")))?;
    if !output.status.success() {
        return Err(CheckError(format!(
            "advisory Git command failed: {}",
            output.status
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| CheckError(error.to_string()))
}

fn check_cache(path: &Path, origin: &str) -> Result<(), CheckError> {
    if !std::fs::symlink_metadata(path)
        .map_err(|error| CheckError(error.to_string()))?
        .is_dir()
        || git_text(path, &["remote", "get-url", "origin"])?.trim() != origin
        || !git_text(path, &["status", "--porcelain", "--untracked-files=all"])?
            .trim()
            .is_empty()
    {
        return Err(CheckError(
            "advisory cache must be a clean checkout of the official database".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn check() -> Result<(), CheckError> {
    std::fs::create_dir_all(".local/advisories").map_err(|error| CheckError(error.to_string()))?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(".local/advisories.lock")
        .map_err(|error| CheckError(error.to_string()))?;
    lock.try_lock()
        .map_err(|error| CheckError(format!("advisory check is already running: {error}")))?;
    let path = Path::new(CACHE);
    if !path
        .try_exists()
        .map_err(|error| CheckError(error.to_string()))?
    {
        run(
            "git",
            &[
                "clone",
                "--filter=blob:none",
                "--single-branch",
                "--branch",
                "main",
                ORIGIN,
                CACHE,
            ],
        )?;
    }
    check_cache(path, ORIGIN)?;
    git_text(path, &["fetch", "origin", "main"])?;
    git_text(path, &["merge", "--ff-only", "FETCH_HEAD"])?;
    check_cache(path, ORIGIN)?;
    println!(
        "RustSec advisory revision: {}",
        git_text(path, &["rev-parse", "HEAD"])?.trim()
    );
    run("cargo", &["deny", "fetch", "index", "std-replacement"])?;
    for target in [
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        println!("Dependency target: {target}");
        run(
            "cargo",
            &[
                "deny",
                "--offline",
                "--locked",
                "--target",
                target,
                "--log-level",
                "error",
                "check",
            ],
        )?;
    }
    run("cargo-machete", &["."])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_updates_refuse_dirty_and_unrelated_checkouts() -> Result<(), String> {
        let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
        let git = |arguments: &[&str]| -> Result<(), String> {
            let output = Command::new("git")
                .current_dir(directory.path())
                .args(arguments)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        };
        git(&["init"])?;
        git(&[
            "remote",
            "add",
            "origin",
            "https://github.com/RustSec/advisory-db",
        ])?;
        assert!(check_cache(directory.path(), "https://github.com/RustSec/advisory-db").is_ok());
        assert!(check_cache(directory.path(), "https://example.invalid/other").is_err());
        std::fs::write(directory.path().join("untracked"), "local edit")
            .map_err(|error| error.to_string())?;
        assert!(check_cache(directory.path(), "https://github.com/RustSec/advisory-db").is_err());
        Ok(())
    }
}
