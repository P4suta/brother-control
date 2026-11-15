---
date: 2026-11-15
status: proposed
---

# Trust certificates without subject alternative names

## Context

The HL-L2375DW presents an X.509v1 certificate without a subject alternative name.
The [transport decision](pin-device-certificates-and-resolve-transport-addresses.md) prevents setup.
[RFC 9525](https://www.rfc-editor.org/rfc/rfc9525.html#section-6.6) permits prior certificate configuration and prohibits Common Name fallback.

## Decision

Require an explicit SHA-256 fingerprint during setup.
Without a Domain Name System (DNS) name, use the transport hostname and registered certificate as the connection's only trust anchor.
Turn off hostname matching for that connection while checking certificate paths, validity, and TLS signatures.
Check each response's fingerprint and the registered Internet Printing Protocol (IPP) identity and model.
Preserve DNS verification when available.
Use native TLS for legacy X.509v1 certificates, which Rustls rejects.

## Alternatives

Managed certificates preserve DNS verification but require changing printer configuration.
A custom Rustls verifier rejects altered certificates during the handshake but requires shared trait objects.
Reqwest's native TLS implementation keeps cryptographic checks without that exception.

## Consequences

Legacy authentication relies on the registered private key.
Another certificate signed by that key can complete TLS before response rejection.
Reconsider the choice if delegated certificates become a supported printer feature.

## Confirmation

Keep the record proposed until unrelated-certificate rejection, expiry, hostname-bearing certificates, and both physical printers pass verification.
