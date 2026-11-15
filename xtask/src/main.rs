use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
mod codex;
mod dependencies;
mod install;
mod secrets;

#[derive(Debug)]
struct CheckError(String);

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CheckError {}

fn run(program: &str, arguments: &[&str]) -> Result<(), CheckError> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| CheckError(format!("{program}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CheckError(format!(
            "{program} {} failed: {status}",
            arguments.join(" ")
        )))
    }
}

fn fixtures() -> Result<(), CheckError> {
    for (name, accepted) in [
        ("concrete", true),
        ("direct", false),
        ("alias", false),
        ("inferred", false),
        ("trait_object", false),
        ("authored_macro", false),
        ("shared_mutation", false),
        ("unused_alias", false),
        ("trait_declaration", false),
        ("broadened_exception", false),
        ("stale_exception", false),
    ] {
        let output = Command::new("cargo")
            .args([
                "dylint",
                "--path",
                "verification/static-types",
                "--all",
                "--fail-on-no-libraries",
                "--",
                "--manifest-path",
                "verification/type-fixtures/Cargo.toml",
                "--bin",
                name,
            ])
            .output()
            .map_err(|error| CheckError(error.to_string()))?;
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let marker = if matches!(name, "broadened_exception" | "stale_exception") {
            "error[E0453]"
        } else {
            "error: static_types:"
        };
        if output.status.success() != accepted || (!accepted && !diagnostic.contains(marker)) {
            return Err(CheckError(format!(
                "fixture {name} failed its contract:\n{diagnostic}"
            )));
        }
        println!("static type fixture {name}: passed");
    }
    Ok(())
}

fn source_policy(directory: &Path) -> Result<(), CheckError> {
    for entry in fs::read_dir(directory).map_err(|error| CheckError(error.to_string()))? {
        let entry = entry.map_err(|error| CheckError(error.to_string()))?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == "target") {
            continue;
        }
        let metadata = entry
            .file_type()
            .map_err(|error| CheckError(error.to_string()))?;
        if metadata.is_symlink() {
            return Err(CheckError(format!("source symlink: {}", path.display())));
        }
        if metadata.is_dir() {
            source_policy(&path)?;
        }
    }
    Ok(())
}

fn compile_fail() -> Result<(), CheckError> {
    let target = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| PathBuf::from("target"), PathBuf::from)
        .join("compile-fail");
    for (name, diagnostic) in [
        ("valid", None),
        ("forge_identity", Some("error[E0603]")),
        ("forge_minutes", Some("error[E0603]")),
        ("forge_verified", Some("error[E0451]")),
        ("reuse_verified", Some("error[E0382]")),
        ("clone_verified", Some("error[E0599]")),
    ] {
        let output = Command::new("cargo")
            .args([
                "check",
                "--locked",
                "--offline",
                "--manifest-path",
                "verification/compile-fail/Cargo.toml",
                "--target-dir",
            ])
            .arg(&target)
            .args(["--bin", name])
            .output()
            .map_err(|error| CheckError(error.to_string()))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() != diagnostic.is_none()
            || diagnostic.is_some_and(|marker| !stderr.contains(marker))
        {
            return Err(CheckError(format!(
                "domain fixture {name} failed its contract:\n{stderr}"
            )));
        }
        println!("domain fixture {name}: passed");
    }
    Ok(())
}

fn execute(argument: &str) -> Result<(), CheckError> {
    match argument {
        "secrets" => secrets::check(),
        "dependencies" => dependencies::check(),
        "compile-fail" => compile_fail(),
        "codex-mcp" => codex::run(),
        "static-types" => {
            fixtures()?;
            run(
                "cargo",
                &[
                    "dylint",
                    "--path",
                    "verification/static-types",
                    "--all",
                    "--fail-on-no-libraries",
                    "--",
                    "--workspace",
                    "--all-targets",
                ],
            )
        }
        "native-check" => {
            source_policy(Path::new("crates"))?;
            run("cargo", &["fmt", "--all", "--", "--check"])?;
            run(
                "cargo",
                &["check", "--locked", "--workspace", "--all-targets"],
            )?;
            run(
                "cargo",
                &[
                    "clippy",
                    "--locked",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
            run("cargo", &["test", "--locked", "--workspace"])?;
            Ok(())
        }
        "check" => {
            execute("native-check")?;
            execute("static-types")?;
            execute("dependencies")?;
            execute("secrets")?;
            run("actionlint", &[])
        }
        _ => Err(CheckError(format!(
            "unknown check: {argument}; expected check or static-types"
        ))),
    }
}

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let result = match (arguments.next(), arguments.next(), arguments.next()) {
        (Some(argument), Some(root), None) if argument == "install" => {
            install::run(Path::new(&root))
        }
        (Some(argument), None, None) => execute(&argument),
        _ => Err(CheckError("expected exactly one check name".to_owned())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
