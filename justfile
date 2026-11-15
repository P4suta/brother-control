set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

check_runner := env("CARGO_TARGET_DIR", "target") / "check-runner"

default:
    @just --list

dependencies:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- dependencies

secrets:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- secrets

install root:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- install {{ quote(root) }}

check:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- check

native-check:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- native-check

static-types:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- static-types

compile-fail:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- compile-fail

codex-mcp:
    cargo run --locked --target-dir {{ quote(check_runner) }} -p xtask -- codex-mcp

fmt:
    cargo fmt --all
