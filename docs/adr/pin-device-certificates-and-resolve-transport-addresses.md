---
date: 2026-11-15
status: proposed
---

# Pin device certificates and resolve transport addresses

## Context

Brother printers use self-signed certificates whose Domain Name System (DNS) names can differ from routed transport addresses.
The approved setup requires explicit certificate trust and per-device credentials.
[Reqwest](https://docs.rs/reqwest/0.13.5/reqwest/blocking/struct.ClientBuilder.html#method.tls_certs_only) supports isolated trust roots and address resolution overrides.

## Decision

Inspect certificates through an HTTPS request without credentials or redirects.
Require an explicitly supplied SHA-256 fingerprint before persisting trust.
Use the certificate DNS name for authenticated HTTPS and resolve that name to the selected transport address.
Keep certificate and hostname validation enabled for authenticated requests.
Store passwords in the native credential store or reference a device-specific environment variable.

## Alternatives

A private certificate authority avoids manual fingerprints but requires changing printer authentication infrastructure outside the approved scope.
Disabling certificate validation during authenticated requests would leave credentials exposed to interception.

## Consequences

Certificate replacement requires setup again.
Configuration exports contain references and public certificates, never passwords.
Reconsider the trust model when managed certificate enrollment becomes available.

## Confirmation

Keep the record proposed until certificate mismatch, redirect, credential redaction, routed transport, and native platform tests pass.
