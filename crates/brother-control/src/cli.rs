use crate::{
    ipp_adapter::Endpoint,
    service,
    types::{Failure, FailureKind, Host},
};
use clap::{Args, Parser, Subcommand};
use std::{
    io::{self, Write},
    process::ExitCode,
    time::Duration,
};

#[derive(Parser)]
#[command(
    name = "brotherctl",
    about = "Manage Brother printers through verified device operations",
    version
)]
pub struct Cli {
    #[arg(long, global = true, help = "Write a machine-readable JSON result")]
    json: bool,
    #[arg(long, global = true, help = "Use a specific configuration directory")]
    config_dir: Option<std::path::PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Report remote reboot support for a selected printer")]
    Reboot {
        #[arg(long, value_parser = crate::types::Identity::parse)]
        device: crate::types::Identity,
    },
    #[command(about = "Manage explicit native credential authorization")]
    Credentials {
        #[command(subcommand)]
        command: CredentialsCommand,
    },
    #[command(about = "Compare component versions with the official Brother catalog")]
    Firmware {
        #[command(subcommand)]
        command: FirmwareCommand,
    },
    #[command(hide = true)]
    InspectManagement {
        #[command(flatten)]
        target: DeviceArgs,
        #[arg(long, default_value = "/home/status.html")]
        page: String,
    },
    #[command(about = "Diagnose printer protocols, certificate trust, and authentication")]
    Diagnose(ManagementArgs),
    #[command(about = "Serve the shared printer operations through MCP")]
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    #[command(about = "Read, compare, and apply ordinary printer settings")]
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    #[command(about = "Register device certificate trust and a credential reference")]
    Setup {
        #[command(flatten)]
        target: ManagementArgs,
        #[arg(long, default_value_t = 443)]
        web_port: u16,
        #[arg(long, conflicts_with = "trust_sha256")]
        inspect_certificate: bool,
        #[arg(long, required_unless_present = "inspect_certificate")]
        trust_sha256: Option<crate::config::Fingerprint>,
        #[arg(
            long,
            conflicts_with = "inspect_certificate",
            help = "Reference a device-specific password environment variable"
        )]
        credential_env: bool,
        #[arg(long, conflicts_with_all = ["inspect_certificate", "credential_env"], help = "Reuse the registered credential when changing the transport address")]
        reuse_credential: bool,
    },
    #[command(about = "Read printer status and observation sources")]
    Status(ReadArgs),
    #[command(about = "Discover supported printers on the local network through DNS-SD")]
    Discover {
        #[arg(long, help = "Probe a private IPv4 subnet with prefix 24 through 32")]
        subnet: Option<crate::discovery::PrivateSubnet>,
        #[arg(
            long,
            requires = "subnet",
            help = "Use the Tailscale 4via6 site identifier"
        )]
        via_site: Option<u16>,
        #[arg(long, help = "Probe an explicit printer hostname or IP address")]
        host: Vec<Host>,
        #[arg(long, default_value_t = 3000, value_parser = clap::value_parser!(u64).range(1..=30000))]
        timeout_ms: u64,
    },
}

#[derive(Subcommand)]
enum McpCommand {
    Serve,
}

#[derive(Subcommand)]
enum CredentialsCommand {
    #[command(
        about = "Authorize saved credentials interactively; the OS may show permission dialogs"
    )]
    Authorize {
        #[arg(
            long,
            help = "Select a registered device; defaults to all native profiles",
            value_parser = crate::types::Identity::parse
        )]
        device: Vec<crate::types::Identity>,
    },
}

#[derive(Subcommand)]
enum FirmwareCommand {
    Check(ManagementArgs),
}

#[derive(Subcommand)]
enum SettingsCommand {
    List,
    Get {
        #[command(flatten)]
        target: DeviceArgs,
        #[arg(long, value_parser = crate::settings::parse_key)]
        key: Option<brother_core::SettingKey>,
    },
    Export {
        #[command(flatten)]
        target: DeviceArgs,
    },
    Diff {
        #[command(flatten)]
        target: DeviceArgs,
        #[arg(long)]
        desired: std::path::PathBuf,
    },
    Apply {
        #[command(flatten)]
        target: DeviceArgs,
        #[arg(long)]
        diff: std::path::PathBuf,
    },
}

#[derive(Args)]
struct DeviceArgs {
    #[arg(long, value_parser = crate::types::Identity::parse)]
    device: crate::types::Identity,
    #[arg(long, default_value_t = 15000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    timeout_ms: u64,
}

#[derive(Args)]
struct ManagementArgs {
    #[arg(long, help = "Printer hostname or IP address")]
    host: Host,
    #[arg(long, default_value_t = 631)]
    port: u16,
    #[arg(long, default_value = "/ipp/print")]
    resource: String,
    #[arg(long, default_value_t = 15000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    timeout_ms: u64,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T, Failure> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| Failure::new(FailureKind::Storage, "unable to open the input file"))?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::new(FailureKind::Storage, "unable to read the input file"))?;
    if bytes.len() > 65_536 {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "input file exceeds the size limit",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        Failure::new(
            FailureKind::InvalidInput,
            "input file does not match the operation schema",
        )
    })
}

fn config_store(root: Option<std::path::PathBuf>) -> Result<crate::config::ConfigStore, Failure> {
    match root {
        Some(root) => Ok(crate::config::ConfigStore::new(root)),
        None => crate::config::ConfigStore::platform(),
    }
}

#[derive(Args)]
struct ReadArgs {
    #[arg(long, help = "Printer hostname or IP address")]
    host: Host,
    #[arg(long, default_value_t = 631)]
    port: u16,
    #[arg(long, default_value = "/ipp/print")]
    resource: String,
    #[arg(long, default_value_t = 3000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    timeout_ms: u64,
}

impl Cli {
    fn execute(self) -> Result<ExitCode, Failure> {
        match self.command {
            Command::Reboot { device } => Err(service::reboot(device)),
            Command::Credentials {
                command: CredentialsCommand::Authorize { device },
            } => write(
                crate::credentials::authorize(&config_store(self.config_dir)?, &device)?,
                self.json,
            ),
            Command::Firmware {
                command: FirmwareCommand::Check(target),
            } => write(
                crate::firmware::check(
                    &config_store(self.config_dir)?,
                    &Endpoint::new(target.host, target.port, &target.resource)?,
                    Duration::from_millis(target.timeout_ms),
                )?,
                self.json,
            ),
            Command::InspectManagement { target, page } => {
                let result = crate::inspection::inspect(
                    &config_store(self.config_dir)?,
                    target.device,
                    &page,
                    Duration::from_millis(target.timeout_ms),
                )?;
                write(
                    service::Output {
                        operation: service::OperationName::Diagnose,
                        result,
                    },
                    self.json,
                )
            }
            Command::Diagnose(target) => {
                let store = config_store(self.config_dir)?;
                write(
                    crate::diagnose::run(
                        &store,
                        &Endpoint::new(target.host, target.port, &target.resource)?,
                        Duration::from_millis(target.timeout_ms),
                    )?,
                    self.json,
                )
            }
            Command::Mcp {
                command: McpCommand::Serve,
            } => {
                crate::mcp::serve(config_store(self.config_dir)?)?;
                Ok(ExitCode::SUCCESS)
            }
            Command::Settings { command } => {
                let store = config_store(self.config_dir)?;
                match command {
                    SettingsCommand::List => write(
                        service::Output {
                            operation: service::OperationName::SettingsList,
                            result: crate::settings::catalog(),
                        },
                        self.json,
                    ),
                    SettingsCommand::Get { target, key } => {
                        let mut output = service::settings_get(
                            &store,
                            target.device,
                            Duration::from_millis(target.timeout_ms),
                        )?;
                        if let Some(key) = key {
                            output.result.values.retain(|value| value.key == key);
                        }
                        write(output, self.json)
                    }
                    SettingsCommand::Export { target } => {
                        let mut output = service::settings_get(
                            &store,
                            target.device,
                            Duration::from_millis(target.timeout_ms),
                        )?;
                        output.operation = service::OperationName::SettingsExport;
                        write(output, self.json)
                    }
                    SettingsCommand::Diff { target, desired } => {
                        let desired: crate::settings::DesiredDocument = read_json(&desired)?;
                        let values = desired.values_for(target.device)?;
                        write(
                            service::settings_diff(
                                &store,
                                target.device,
                                &values,
                                Duration::from_millis(target.timeout_ms),
                            )?,
                            self.json,
                        )
                    }
                    SettingsCommand::Apply { target, diff } => {
                        let difference: crate::settings::DifferenceDocument = read_json(&diff)?;
                        let difference = difference.into_difference()?;
                        let mut cancellation = crate::cancellation::Cancellation::install()?;
                        write(
                            service::settings_apply(
                                &store,
                                target.device,
                                &difference,
                                Duration::from_millis(target.timeout_ms),
                                || cancellation.poll(),
                            )?,
                            self.json,
                        )
                    }
                }
            }
            Command::Setup {
                target,
                web_port,
                inspect_certificate,
                trust_sha256,
                credential_env,
                reuse_credential,
            } => {
                let timeout = Duration::from_millis(target.timeout_ms);
                if inspect_certificate {
                    let result = crate::web::inspect_certificate(&target.host, web_port, timeout)?;
                    write(
                        service::Output {
                            operation: service::OperationName::Setup,
                            result,
                        },
                        self.json,
                    )
                } else {
                    let trust = trust_sha256.ok_or_else(|| {
                        Failure::new(
                            FailureKind::Certificate,
                            "explicit certificate trust is required",
                        )
                    })?;
                    let prepared = crate::setup::prepare(
                        Endpoint::new(target.host, target.port, &target.resource)?,
                        web_port,
                        trust,
                        timeout,
                    )?;
                    let registration = if reuse_credential {
                        crate::setup::Registration::Reuse
                    } else if credential_env {
                        crate::setup::Registration::Environment
                    } else {
                        let value = rpassword::prompt_password(crate::setup::password_prompt(
                            prepared.printer(),
                        ))
                        .map_err(|_| {
                            Failure::new(
                                FailureKind::Storage,
                                "unable to read the hidden password input",
                            )
                        })?;
                        crate::setup::Registration::Native(crate::config::Secret::new(value)?)
                    };
                    let store = match self.config_dir {
                        Some(root) => crate::config::ConfigStore::new(root),
                        None => crate::config::ConfigStore::platform()?,
                    };
                    write(prepared.finish(&store, registration)?, self.json)
                }
            }
            Command::Status(arguments) => write(
                service::status(
                    &Endpoint::new(arguments.host, arguments.port, &arguments.resource)?,
                    Duration::from_millis(arguments.timeout_ms),
                )?,
                self.json,
            ),
            Command::Discover {
                timeout_ms,
                subnet,
                via_site,
                mut host,
            } => {
                if let Some(subnet) = subnet {
                    host.extend(subnet.hosts(via_site)?);
                }
                let window = Duration::from_millis(timeout_ms);
                let output = if host.is_empty() {
                    service::discover(window)?
                } else {
                    let endpoints = host
                        .into_iter()
                        .map(|host| Endpoint::new(host, 631, "/ipp/print"))
                        .collect::<Result<Vec<_>, _>>()?;
                    service::Output {
                        operation: service::OperationName::Discover,
                        result: crate::discovery::probe(&endpoints, window)?,
                    }
                };
                write(output, self.json)
            }
        }
    }
}

fn write<T: serde::Serialize + service::Report>(
    output: service::Output<T>,
    json: bool,
) -> Result<ExitCode, Failure> {
    let success = output.result.succeeded();
    let bytes = if json {
        serde_json::to_vec(&output)
    } else {
        serde_json::to_vec_pretty(&output)
    }
    .map_err(|_| {
        Failure::new(
            FailureKind::Storage,
            "unable to encode the operation result",
        )
    })?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|_| Failure::new(FailureKind::Storage, "unable to write the operation result"))?;
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.json;
    match cli.execute() {
        Ok(code) => code,
        Err(error) => {
            if json {
                let encoded = serde_json::to_vec(&service::ErrorReport { error });
                let written = encoded.ok().is_some_and(|bytes| {
                    let mut stdout = io::stdout().lock();
                    stdout
                        .write_all(&bytes)
                        .and_then(|()| stdout.write_all(b"\n"))
                        .is_ok()
                });
                if !written {
                    eprintln!("unable to write the operation failure");
                }
            } else {
                eprintln!("{error}");
            }
            ExitCode::FAILURE
        }
    }
}
