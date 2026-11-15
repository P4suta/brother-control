---
date: 2026-11-15
status: proposed
---

# Static dispatch and verified device changes

## Context

The approved CLI and Model Context Protocol (MCP) server manage two Brother models.
Observations can be missing, stale, unsupported, or ambiguous after a write.
The [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/type-safety.html) recommend distinct identity and unit types.

## Decision

Keep pure decisions in a dependency-free core and effects in concrete adapters.
Use exhaustive operations and the official [SDK handler](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/handler/server.rs) without its dynamic tool router.
Match identity and current values before writing, then read back the result.
Never retry a mutation automatically.
Trust each certificate explicitly and store credential references.
Enforce type restrictions with Clippy and [Dylint](https://github.com/trailofbits/dylint).

## Alternatives

A dynamic registry conflicts with the approved static dispatch constraint.
Platform command wrappers expose different contracts across operating systems.

## Consequences

Audit dependency internals at adoption.
The current source uses no type-policy exceptions.
Unregistered, broadened, and stale suppressions fail.
An exception needs an API constraint, affected item, test, removal trigger, and targeted gate change.
Unknown management forms fail closed instead of guessing field names.
Reconsider static dispatch if the product needs runtime plugins.

## Confirmation

Keep the record proposed until protocol fixtures, resolved-type rejection tests, native platform checks, and both device round trips pass.
