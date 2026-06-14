---
name: Feature request
about: Suggest a dcmview improvement without sharing sensitive data
title: "[Feature]: "
labels: enhancement
assignees: ""
---

## Safety checklist

- [ ] I have not attached DICOM files, screenshots with identifiers, PHI, or other sensitive data.
- [ ] I have redacted patient names, IDs, dates, accession numbers, institution names, hostnames, usernames, paths, and tokens from examples.
- [ ] I understand that `dcmview` is for research and development inspection, not clinical diagnosis.

Do not upload clinical DICOM files, private research datasets, or screenshots of
identifiable studies to a public issue. Use synthetic, public, or fully
de-identified examples only.

## Problem

What workflow is hard, slow, confusing, or currently impossible?

## Proposed behavior

Describe the smallest useful behavior change. Include the user-facing command,
Python call, VS Code action, viewer control, API/debugging behavior, or
documentation change you expect when relevant.

## Alternatives considered

Describe any workaround you currently use, or why existing CLI flags, Python
options, VS Code settings, or documentation do not address the workflow.

## Scope

- Interface affected: CLI / Python / VS Code / viewer UI / API-debugging / docs / packaging / other
- Data type involved: single file / directory / multi-frame / compressed pixels / annotations / metadata / other
- Remote workflow involved: yes / no

Keep dataset details technical and non-sensitive. Avoid patient, study,
institution, accession, hostname, username, and exact path values unless they
are synthetic or explicitly approved for public sharing.

## Additional context

Add non-sensitive context, links to public examples, or synthetic reproduction
notes that would help evaluate the request.
