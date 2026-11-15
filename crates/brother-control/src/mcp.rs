use crate::{
    config::ConfigStore,
    ipp_adapter::Endpoint,
    service::{self, OperationName, Output, Report},
    settings::{Desired, SettingsDiff},
    types::{Failure, FailureKind, Host, Identity},
};
use rmcp::{ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde::{Deserialize, Serialize};
use std::time::Duration;

fn default_timeout() -> u64 {
    3000
}
fn management_timeout() -> u64 {
    15000
}
fn default_port() -> u16 {
    631
}
fn default_resource() -> String {
    "/ipp/print".to_owned()
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Reboot {
        device: Identity,
    },
    FirmwareCheck {
        host: Host,
        #[serde(default = "default_port")]
        port: u16,
        #[serde(default = "default_resource")]
        resource: String,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
    Diagnose {
        host: Host,
        #[serde(default = "default_port")]
        port: u16,
        #[serde(default = "default_resource")]
        resource: String,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
    Status {
        host: Host,
        #[serde(default = "default_port")]
        port: u16,
        #[serde(default = "default_resource")]
        resource: String,
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
    },
    Discover {
        #[serde(default = "default_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        hosts: Vec<Host>,
    },
    SettingsList {},
    SettingsGet {
        device: Identity,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
    SettingsExport {
        device: Identity,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
    SettingsDiff {
        device: Identity,
        desired: Desired,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
    SettingsApply {
        device: Identity,
        difference: SettingsDiff,
        #[serde(default = "management_timeout")]
        timeout_ms: u64,
    },
}

fn timeout(milliseconds: u64) -> Result<Duration, Failure> {
    if !(1..=60_000).contains(&milliseconds) {
        return Err(Failure::new(
            FailureKind::InvalidInput,
            "timeout must be 1 through 60000 milliseconds",
        ));
    }
    Ok(Duration::from_millis(milliseconds))
}

fn encoded<T: Serialize + Report>(output: Output<T>) -> Result<CallToolResult, Failure> {
    let success = output.result.succeeded();
    let value = serde_json::to_value(output).map_err(|_| {
        Failure::new(
            FailureKind::Storage,
            "unable to encode the operation result",
        )
    })?;
    Ok(if success {
        CallToolResult::structured(value)
    } else {
        CallToolResult::structured_error(value)
    })
}

pub fn execute<C: FnMut() -> bool>(
    store: &ConfigStore,
    request: Request,
    cancelled: C,
) -> Result<CallToolResult, Failure> {
    match request {
        Request::Reboot { device } => Err(service::reboot(device)),
        Request::FirmwareCheck {
            host,
            port,
            resource,
            timeout_ms,
        } => encoded(crate::firmware::check(
            store,
            &Endpoint::new(host, port, &resource)?,
            timeout(timeout_ms)?,
        )?),
        Request::Diagnose {
            host,
            port,
            resource,
            timeout_ms,
        } => encoded(crate::diagnose::run(
            store,
            &Endpoint::new(host, port, &resource)?,
            timeout(timeout_ms)?,
        )?),
        Request::Status {
            host,
            port,
            resource,
            timeout_ms,
        } => encoded(service::status(
            &Endpoint::new(host, port, &resource)?,
            timeout(timeout_ms)?,
        )?),
        Request::Discover { timeout_ms, hosts } => {
            let window = timeout(timeout_ms)?;
            if hosts.is_empty() {
                encoded(service::discover(window)?)
            } else {
                let endpoints = hosts
                    .into_iter()
                    .map(|host| Endpoint::new(host, 631, "/ipp/print"))
                    .collect::<Result<Vec<_>, _>>()?;
                encoded(Output {
                    operation: OperationName::Discover,
                    result: crate::discovery::probe(&endpoints, window)?,
                })
            }
        }
        Request::SettingsList {} => encoded(Output {
            operation: OperationName::SettingsList,
            result: crate::settings::catalog(),
        }),
        Request::SettingsGet { device, timeout_ms } => {
            encoded(service::settings_get(store, device, timeout(timeout_ms)?)?)
        }
        Request::SettingsExport { device, timeout_ms } => {
            let mut result = service::settings_get(store, device, timeout(timeout_ms)?)?;
            result.operation = OperationName::SettingsExport;
            encoded(result)
        }
        Request::SettingsDiff {
            device,
            desired,
            timeout_ms,
        } => encoded(service::settings_diff(
            store,
            device,
            &desired.values,
            timeout(timeout_ms)?,
        )?),
        Request::SettingsApply {
            device,
            difference,
            timeout_ms,
        } => encoded(service::settings_apply(
            store,
            device,
            &difference,
            timeout(timeout_ms)?,
            cancelled,
        )?),
    }
}

fn error_result(failure: Failure) -> CallToolResult {
    match serde_json::to_value(service::ErrorReport { error: failure }) {
        Ok(value) => CallToolResult::structured_error(value),
        Err(_) => CallToolResult::error(Vec::from([ContentBlock::text(
            "unable to encode the operation failure",
        )])),
    }
}

fn tool() -> Result<Tool, Failure> {
    let schema: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../schema/tool-input.json")).map_err(|_| {
            Failure::new(
                FailureKind::InvalidInput,
                "invalid bundled MCP input schema",
            )
        })?;
    Ok(Tool::new("brother_control", "Read printer observations and apply device-bound settings differences; credentials must be registered with brotherctl setup.", schema)
        .annotate(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true)))
}

pub struct Handler {
    store: ConfigStore,
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("brother-control", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::with_all_items(Vec::from([tool()
            .map_err(|_| {
                rmcp::ErrorData::internal_error("invalid printer tool schema", None)
            })?])))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        if request.name != "brother_control" {
            return Err(rmcp::ErrorData::invalid_params(
                "unknown printer tool",
                None,
            ));
        }
        let input = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let request = match serde_json::from_value::<Request>(input) {
            Ok(request) => request,
            Err(_) => {
                return Ok(error_result(Failure::new(
                    FailureKind::InvalidInput,
                    "tool arguments do not match a supported operation schema",
                ))
                .into());
            }
        };
        let store = self.store.clone();
        let response = tokio::task::spawn_blocking(move || {
            execute(&store, request, || context.ct.is_cancelled())
        })
        .await
        .map_err(|_| rmcp::ErrorData::internal_error("printer operation task failed", None))?;
        Ok(match response {
            Ok(result) => result,
            Err(failure) => error_result(failure),
        }
        .into())
    }
}

pub fn serve(store: ConfigStore) -> Result<(), Failure> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::new(FailureKind::Storage, "unable to initialize the MCP runtime"))?
        .block_on(async move {
            let service = Handler { store }
                .serve(rmcp::transport::stdio())
                .await
                .map_err(|_| {
                    Failure::new(FailureKind::UnknownResponse, "MCP initialization failed")
                })?;
            service
                .waiting()
                .await
                .map_err(|_| Failure::new(FailureKind::UnknownResponse, "MCP transport failed"))?;
            Ok(())
        })
}
