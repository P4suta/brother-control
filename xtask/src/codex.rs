use super::CheckError;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Inventory {
    data: Vec<Server>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    name: String,
    tools: BTreeMap<String, serde_json::Value>,
    tools_error: Option<String>,
}

fn failure() -> CheckError {
    CheckError("Codex did not discover the brother-control MCP tool".to_owned())
}

fn check_catalog(input: &str) -> Result<(), CheckError> {
    let inventory: Inventory = serde_json::from_str(input).map_err(|_| failure())?;
    if inventory.data.iter().any(|server| {
        server.name == "brother-control"
            && server.tools_error.is_none()
            && server.tools.contains_key("brother_control")
    }) {
        Ok(())
    } else {
        Err(failure())
    }
}

#[derive(Deserialize)]
struct Reply {
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

fn send(input: &mut ChildStdin, document: &str) -> Result<(), CheckError> {
    input
        .write_all(document.as_bytes())
        .and_then(|()| input.write_all(b"\n"))
        .map_err(|_| CheckError("unable to send the Codex protocol request".to_owned()))
}

fn response(
    receiver: &mpsc::Receiver<std::io::Result<String>>,
    id: u64,
) -> Result<serde_json::Value, CheckError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    for _ in 0..64 {
        let line = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| CheckError("Codex protocol response deadline expired".to_owned()))?
            .map_err(|_| CheckError("unable to read the Codex protocol response".to_owned()))?;
        let reply: Reply = serde_json::from_str(&line)
            .map_err(|_| CheckError("invalid Codex protocol response".to_owned()))?;
        if reply.id == Some(id) {
            if reply.error.is_some() {
                return Err(CheckError("Codex rejected the protocol request".to_owned()));
            }
            return reply.result.ok_or_else(failure);
        }
    }
    Err(CheckError(
        "Codex protocol notification limit exceeded".to_owned(),
    ))
}

fn exchange(
    input: &mut ChildStdin,
    receiver: &mpsc::Receiver<std::io::Result<String>>,
) -> Result<(), CheckError> {
    send(
        input,
        r#"{"id":1,"method":"initialize","params":{"clientInfo":{"name":"brother-control-check","version":"0.0.0"}}}"#,
    )?;
    response(receiver, 1)?;
    send(input, r#"{"method":"initialized"}"#)?;
    send(
        input,
        r#"{"id":2,"method":"mcpServerStatus/list","params":{"serverName":"brother-control","detail":"toolsAndAuthOnly"}}"#,
    )?;
    let catalog = response(receiver, 2)?;
    check_catalog(&catalog.to_string())
}

fn finish(child: &mut Child) -> Result<(), CheckError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child
            .try_wait()
            .map_err(|_| CheckError("unable to inspect the Codex process".to_owned()))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(CheckError(format!("Codex exited unsuccessfully: {status}")));
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            None => {
                child
                    .kill()
                    .map_err(|_| CheckError("unable to stop the Codex check process".to_owned()))?;
                child
                    .wait()
                    .map_err(|_| CheckError("unable to reap the Codex check process".to_owned()))?;
                return Err(CheckError(
                    "Codex did not stop after closing its input".to_owned(),
                ));
            }
        }
    }
}

pub(super) fn run() -> Result<(), CheckError> {
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| CheckError("Codex CLI is unavailable".to_owned()))?;
    let mut input = child.stdin.take().ok_or_else(failure)?;
    let output = child.stdout.take().ok_or_else(failure)?;
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::scope(|scope| {
        let _reader = scope.spawn(move || {
            for line in BufReader::new(output).take(4 * 1024 * 1024).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let result = exchange(&mut input, &receiver);
        drop(input);
        drop(receiver);
        let cleanup = finish(&mut child);
        result?;
        cleanup?;
        println!("Codex discovered brother-control through its MCP client");
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_discovered_printer_tool_establishes_codex_connectivity() {
        assert!(super::check_catalog(r#"{"data":[{"name":"brother-control","tools":{"brother_control":{}},"toolsError":null}]}"#).is_ok());
        for input in [
            r#"{"data":[]}"#,
            r#"{"data":[{"name":"brother-control","tools":{},"toolsError":"connection failed"}]}"#,
            "malformed",
        ] {
            assert!(super::check_catalog(input).is_err());
        }
    }
}
