set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

default:
    @just --list

dependencies:
    cargo run --locked --target-dir target/check-runner -p xtask -- dependencies

secrets:
    cargo run --locked --target-dir target/check-runner -p xtask -- secrets

install root:
    cargo run --locked --target-dir target/check-runner -p xtask -- install {{ quote(root) }}

check:
    cargo run --locked --target-dir target/check-runner -p xtask -- check

native-check:
    cargo run --locked --target-dir target/check-runner -p xtask -- native-check

static-types:
    cargo run --locked --target-dir target/check-runner -p xtask -- static-types

compile-fail:
    cargo run --locked --target-dir target/check-runner -p xtask -- compile-fail

codex-mcp:
    cargo run --locked --target-dir target/check-runner -p xtask -- codex-mcp

fmt:
    cargo fmt --all
