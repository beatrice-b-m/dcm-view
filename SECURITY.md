# Security Policy

## Supported Versions

Security fixes are handled for the current released version of `dcmview`. If a
security issue affects older versions, maintainers will document the affected
range in the release notes when a fix is available.

## Reporting a Vulnerability

Please disclose suspected security vulnerabilities to the maintainers privately
before public disclosure. Do not include vulnerability details, proof-of-concept
payloads, DICOM files, PHI, credentials, or other sensitive information in public
GitHub issues.

If GitHub private vulnerability reporting is available for this repository, use
that channel. Otherwise, contact the maintainers directly and include:

- A short description of the issue.
- The affected `dcmview` version and install channel.
- Reproduction steps that do not include PHI or sensitive data.
- Any relevant logs with paths, patient identifiers, and hostnames redacted.

Maintainers will acknowledge reports, assess impact, and coordinate a fix before
public disclosure when the report describes a real vulnerability.

## Security Model

`dcmview` is intended for research and development inspection on secure
networks. It is not for clinical diagnosis or clinical decision-making.

The local HTTP server is unauthenticated by design. It binds to `127.0.0.1` by
default and should normally be accessed locally or through SSH port forwarding.
Avoid public-facing binds such as `--host 0.0.0.0` unless you provide your own
network access controls.

DICOM files often contain protected or sensitive information. Anyone who can
reach the running `dcmview` server may be able to access image pixels, metadata,
file paths, patient identifiers, study identifiers, and in-memory annotations.
