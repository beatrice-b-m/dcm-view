---
name: Bug report
about: Report a reproducible dcmview problem without sharing sensitive data
title: "[Bug]: "
labels: bug
assignees: ""
---

## Safety checklist

- [ ] I have not attached DICOM files, screenshots with identifiers, PHI, or other sensitive data.
- [ ] I have redacted patient names, IDs, dates, accession numbers, institution names, hostnames, usernames, paths, and tokens from logs.
- [ ] I understand that `dcmview` is for research and development inspection, not clinical diagnosis.

## Summary

Describe what went wrong and what you expected to happen.

## Steps to reproduce

1. Opened dcmview with:
2. Clicked or requested:
3. Observed:

Use synthetic, public, or fully de-identified data only. Do not upload clinical DICOM files to a public issue.

## Environment

- dcmview version:
- Install method: Cargo / `dcmview-py` / VS Code extension / source build / other
- Operating system:
- Rust version, if built from source:
- Node.js and npm versions, if built from source:
- Python version, if using `dcmview-py`:
- VS Code extension version, if applicable:

## Command or workflow

Paste the command, Python snippet, or VS Code action that triggered the issue.
Redact sensitive paths, hostnames, usernames, tokens, and dataset identifiers.

```text

```

## Logs or errors

Paste the smallest useful error output. Redact all PHI, DICOM identifiers,
network details, local paths, and secrets before posting.

```text

```

## Additional context

Mention transfer syntax, modality, frame count, annotation CSV shape, or viewer
mode when relevant, but keep all dataset details de-identified.
