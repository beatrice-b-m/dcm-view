---
name: DICOM compatibility report
about: Report a DICOM compatibility issue without sharing sensitive data
title: "[DICOM]: "
labels: dicom-compatibility
assignees: ""
---

## Safety checklist

- [ ] I have not attached DICOM files, screenshots with identifiers, PHI, or other sensitive data.
- [ ] I have redacted patient names, IDs, dates, accession numbers, institution names, hostnames, usernames, paths, UIDs when sensitive, and tokens from logs.
- [ ] I can reproduce the issue with synthetic, public, or fully de-identified data, or I can describe the metadata without uploading the dataset.
- [ ] I understand that `dcmview` is for research and development inspection, not clinical diagnosis.

Do not upload clinical DICOM files, private research datasets, or screenshots of
identifiable studies to a public issue. If the problem requires a real dataset
to diagnose, describe the compatibility symptom and ask maintainers how to share
details safely before posting any files.

## Compatibility symptom

What did `dcmview` fail to load, decode, display, export, or validate?

Examples:

- File was skipped during discovery.
- Viewer reported an unsupported transfer syntax.
- Frame endpoint returned an error.
- Pixels displayed with the wrong intensity, orientation, color, or frame count.
- Tags or annotations were missing, malformed, or inconsistent with expectations.

## Dataset description

Provide only non-sensitive technical details.

- Modality:
- Transfer syntax UID:
- Photometric interpretation:
- Bits allocated / stored:
- Pixel representation:
- Samples per pixel:
- Rows x columns:
- Number of frames:
- Compressed or uncompressed:
- Annotation CSV involved: yes / no

Avoid patient, study, institution, accession, hostname, username, and exact path
values unless they are synthetic or explicitly approved for public sharing.

## Steps to reproduce

1. Opened dcmview with:
2. Selected file or frame:
3. Observed:

Use synthetic, public, or fully de-identified reproduction data only.

## Expected behavior

Describe the expected viewer behavior, HTTP status, pixel result, metadata
value, or annotation outcome.

## Actual behavior

Paste the smallest useful error output or response. Redact all PHI, DICOM
identifiers, network details, local paths, and secrets before posting.

```text

```

## Environment

- dcmview version:
- Install method: Cargo / `dcmview-py` / VS Code extension / source build / other
- Operating system:
- Rust version, if built from source:
- Python version, if using `dcmview-py`:
- VS Code extension version, if applicable:

## Public reproduction option

If a public or synthetic DICOM file can reproduce the issue, link to the source
or describe how it was generated. Confirm that the file is public, synthetic, or
fully de-identified and approved for public issue discussion.
