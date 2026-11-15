use brother_control::types::{Failure, FailureKind};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

fn failed(_: impl std::fmt::Display) -> Failure {
    Failure::new(FailureKind::UnknownResponse, "MCP fixture transport failed")
}

#[test]
fn stdio_negotiates_lists_tools_and_matches_cli_json() -> Result<(), Failure> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(failed)?;
    let mut input = child.stdin.take().ok_or_else(|| failed("missing stdin"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| failed("missing stdout"))?;
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::scope(|scope| {
        let _reader = scope.spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let result = exercise(&mut input, &receiver);
        drop(input);
        drop(receiver);
        let kill_result = child.kill();
        let wait_result = child.wait();
        result?;
        kill_result.map_err(failed)?;
        wait_result.map_err(failed)?;
        Ok(())
    })
}

fn receive(
    receiver: &mpsc::Receiver<std::io::Result<String>>,
) -> Result<serde_json::Value, Failure> {
    let line = receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(failed)?
        .map_err(failed)?;
    serde_json::from_str(&line).map_err(failed)
}

fn exercise(
    input: &mut std::process::ChildStdin,
    receiver: &mpsc::Receiver<std::io::Result<String>>,
) -> Result<(), Failure> {
    send(
        input,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"brother-fixture","version":"0.0.0"}}}"#,
    )?;
    let initialized = receive(receiver)?;
    if !initialized["result"]["capabilities"]["tools"].is_object() {
        return Err(failed("missing tool capability"));
    }
    send(
        input,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    )?;
    send(input, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)?;
    let listed = receive(receiver)?;
    if listed["result"]["tools"][0]["name"] != "brother_control" {
        return Err(failed("missing printer tool"));
    }
    send(
        input,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"brother_control","arguments":{"operation":"settings_list"}}}"#,
    )?;
    let result = receive(receiver)?;
    if result["result"]["isError"] != false {
        return Err(failed("tool execution failed"));
    }
    let cli = Command::new(env!("CARGO_BIN_EXE_brotherctl"))
        .args(["--json", "settings", "list"])
        .output()
        .map_err(failed)?;
    if !cli.status.success() {
        return Err(failed("CLI execution failed"));
    }
    let cli: serde_json::Value = serde_json::from_slice(&cli.stdout).map_err(failed)?;
    if result["result"]["structuredContent"] != cli {
        return Err(failed("CLI and MCP results differ"));
    }
    Ok(())
}

fn send(input: &mut std::process::ChildStdin, document: &str) -> Result<(), Failure> {
    input
        .write_all(document.as_bytes())
        .and_then(|()| input.write_all(b"\n"))
        .map_err(failed)
}
