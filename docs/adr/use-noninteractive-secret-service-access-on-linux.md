---
date: 2026-11-15
status: proposed
---

# Linux credential reads without prompts

## Context

Normal CLI and MCP credential reads must never open permission dialogs.
The default Linux keyring provider depends on [unmaintained zvariant](https://rustsec.org/advisories/RUSTSEC-2026-0370.html).

## Decision

Use the concrete `dbus-secret-service` adapter on Linux with encrypted sessions.
Its [zero prompt timeout](https://docs.rs/dbus-secret-service/4.1.0/dbus_secret_service/struct.SecretService.html#method.connect_with_max_prompt_timeout) prevents interactive access during normal reads.
Require exactly one unlocked item matching the service and device account.
Only setup and explicit credential access may unlock an item, with a 60-second prompt limit.
Keep the working macOS and Windows providers target-specific.
Audit each supported target graph.

## Alternatives

Retaining the default provider keeps the unmaintained dependency graph.
A prerelease provider update adds compatibility uncertainty.
Environment references remain available for headless hosts without a Secret Service.

## Consequences

Linux builds include the vendored D-Bus library.
Dependency internals use dynamic D-Bus values.
The app retains concrete sessions and borrowed items.
Ambiguous and locked entries fail without guessing.
Reconsider the provider when a maintained alternative preserves explicit prompt control.

## Confirmation

Keep the record proposed pending native Linux adapter checks and clean advisory checks for all supported targets.
