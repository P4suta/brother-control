---
date: 2026-11-15
status: proposed
---

# Prevent Keychain dialogs during normal operations

## Context

The macOS legacy Keychain displayed permission dialogs after executable changes despite a SecItem query requesting authentication-free results.
Normal CLI and MCP operations must remain noninteractive.

## Decision

Before the first credential read, turn off optional Keychain interaction through [SecKeychainSetUserInteractionAllowed](https://github.com/apple-oss-distributions/Security/blob/main/OSX/libsecurity_keychain/lib/SecKeychain.h).
Keep the safe Security Framework guard in one process-lifetime `OnceLock`.
Never drop a per-read guard that could re-enable interaction while another MCP request is reading.
Return an authentication failure when access needs permission.
Interactive setup runs in a separate CLI process and may store a password.
Use a stable signing identifier and Developer ID certificate under [Apple's code identity rules](https://developer.apple.com/library/archive/technotes/tn2206/_index.html).
Build and sign a staged executable, verify its signature, then replace the installed file.

## Alternatives

The SecItem authentication-UI query flag alone didn't suppress observed legacy access-control prompts.
Removing item access controls would weaken credential protection.

## Consequences

The shared initialization stores the process interaction policy.
A changed code identity needs explicit `credentials authorize` permission.
Keep the certificate selector in machine-local configuration.
Reconsider the legacy interface when credential storage moves to a supported app identity.

## Confirmation

Require a native test of the interaction flag, concurrent initialization, and credential reads after signed executable replacement.
