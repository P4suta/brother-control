# Printer management

`brotherctl` supports Brother `HL-L2375DW` and `MFC-L2860DW` on macOS, Linux, and Windows.
It reads status through Internet Printing Protocol (IPP) and the Simple Network Management Protocol (SNMP).
Authenticated HTTPS adapters manage Sleep Time, Toner Save, Quiet Mode, default duplex, tray paper size, and tray paper type.
Observations include their source, time, and distinct missing, unknown, unsupported, or failed states.

## Installation and credentials

Build from a source checkout with `mise x rust -- cargo install --locked --path crates/brother-control`.
Linux builds need a C compiler, `pkg-config`, and OpenSSL development headers.
A macOS build without a stable signing identity may need renewed Keychain authorization after replacement.
For macOS installations that keep Keychain permissions across updates, use the signed installer.
Put an existing Developer ID Application certificate's 40-digit selector in `BROTHERCTL_SIGNING_IDENTITY` through the ignored `mise.local.toml` configuration.
Read available public selectors with `security find-identity -v -p codesigning`.
The signed installer builds a staged executable, verifies its signature, and replaces the installed file after verification.
Signing with the same identity preserves saved Keychain permissions across updates.

```sh
mise x -- just install "$HOME/.local"
```

Native credentials use macOS Keychain, Windows Credential Manager, or the Linux Secret Service.
Setup requests a printer password through hidden terminal input after validating the device and certificate.
Normal CLI and MCP reads return an authentication failure when permission requires interaction.
Run `brotherctl credentials authorize` to grant access to saved native credentials.
On macOS, select `Always Allow` and enter the Mac login password when Keychain requests permission.

## Register a printer

Discover on the printer network, or supply a known address.
Inspect the certificate before trusting its SHA-256 fingerprint.
Replace `printer.local` and `FINGERPRINT` with the intended printer's address and verified fingerprint.

```sh
brotherctl discover
brotherctl status --host printer.local
brotherctl setup --host printer.local --inspect-certificate
brotherctl setup --host printer.local --trust-sha256 FINGERPRINT
```

For a headless host, add `--credential-env` during setup and supply the device-specific environment variable named by the saved credential reference.
The variable name is `BROTHERCTL_PASSWORD_` followed by the device's uppercase Universally Unique Identifier (UUID) without hyphens.
Setup saves the reference and reports authentication as missing until a management operation reads the environment.
Passwords stay outside profiles, exports, logs, and operation results.

Use `setup --reuse-credential` with the trusted fingerprint to change an existing device's transport address.
The device identity, model, and registered certificate must match.
The command reuses the saved credential without rewriting its password.

## Inspect and change settings

Use the UUID returned by discovery or status as `DEVICE_UUID`.
Export the current values, edit the desired values, and create a device-bound difference.
The desired document accepts the unchanged export envelope.

```sh
brotherctl settings get --device DEVICE_UUID
brotherctl settings export --device DEVICE_UUID > desired.json
brotherctl settings diff --device DEVICE_UUID --desired desired.json > difference.json
brotherctl settings apply --device DEVICE_UUID --diff difference.json
```

Each write checks the current device and value, sends one request, and reads the value back.
A final read checks earlier changes after the whole batch.
The adapter omits the Eco Mode preset when changing ordinary settings.
Partial failures stop remaining changes.
An ambiguous write stays uncertain and is never resent automatically.
Ctrl-C requests cancellation between requests and preserves completed outcomes.

`diagnose --host ADDRESS` checks protocols, certificate trust, and authentication.
`firmware check --host ADDRESS` compares matching components with Brother's official catalog and includes reference URLs.
The command reports unknown when it can't compare components.
`reboot --device DEVICE_UUID` returns unsupported because the current adapters have no verified restart operation that preserves configuration.
Firmware installation, network resets, printing, and scanning remain outside the command surface.

## Tailscale and MCP

A subnet router needs advertised routes, administration approval, client route acceptance, and applicable access grants.
Overlapping networks can use Tailscale 4via6 addresses with the router's site ID.
Discover a bounded private subnet through a configured router with `brotherctl discover --subnet 10.1.1.0/24 --via-site 7`.
Subnet routes carry unicast traffic and don't relay multicast discovery.
Registered routed profiles work without local multicast name resolution.

Register `brotherctl mcp serve` as a stdio Model Context Protocol (MCP) server.
The `brother_control` tool uses the same typed operations and result contracts as the CLI's `--json` output.
Setup and interactive credential permission stay in the terminal.

```sh
codex mcp add brother-control -- "$HOME/.local/bin/brotherctl" mcp serve
mise x -- just codex-mcp
```

## Development

`mise x -- just check` runs formatting, compiler checks, Clippy, tests, compile-fail fixtures, resolved-type checks, dependency policy, secret scanning, and workflow linting.
The dependency check uses a clean project-local RustSec checkout and fast-forward updates.
It rejects dirty or unrelated advisory caches and audits each supported target graph.
The check prints the advisory revision used for the run.
`mise x -- just native-check` runs formatting, compilation, Clippy, and adapter and MCP tests on a native platform.
Git hooks and pull-request CI call the project commands.
The manual CI workflow includes macOS and Windows checks for a candidate.
Native adapters need each platform's credential and TLS services.
