# Current Remediation Plan

Date: 2026-06-14

## Goal

Improve the public-facing quality of `dcmview` by making install paths,
configuration, troubleshooting, API expectations, and contributor/project
hygiene clear enough for first-time users and future contributors.

This plan covers remaining agent-resolvable work after the initial public review,
security policy, distribution clarification, and `debug-api` feature were
completed.

## Guiding Decisions

- `dcmview` is for research and development inspection on secure networks, not
  clinical use.
- Public-facing server binds should be discouraged across distribution channels.
- The HTTP API is internal to the viewer. It may be documented for debugging and
  test automation, but should not be positioned as a stable public integration
  API.
- Cross-origin browser access to the viewer API is available only through the
  `debug-api` Cargo feature and must remain clearly marked as debugging-only.
- PyPI wheels are distributed through `dcmview-py`.
- The VS Code extension is available through the VS Code Marketplace.
- Homebrew is planned for `v0.2.7`, but tap configuration is not complete yet.
- Public issue reports must not include PHI, sensitive DICOM data, or
  non-redacted logs.
- Screenshots and GIFs are desirable but blocked until an appropriate
  non-sensitive dataset is selected.

## Phase 1: Command-Line Self-Service

Objective: make `dcmview --help` and `python -m dcmview_py --help`
self-explanatory without requiring users to read the README first.

Tasks:

- Add descriptive `help` and `value_name` text for every Rust CLI argument in
  `src/main.rs`.
- Add long help examples for common workflows: single file, recursive directory,
  remote `--no-browser` with SSH forwarding, annotation CSV, and filters.
- Hide `--startup-json` from normal help or label it as integration-only.
- Add equivalent descriptions and examples to `python/dcmview_py/__main__.py`.
- Keep the README CLI table aligned with the actual help text.

Acceptance criteria:

- `target/debug/dcmview --help` explains each option clearly.
- `target/debug/dcmview --help` includes at least one remote workflow example.
- `PYTHONPATH=python python -m dcmview_py --help` explains each option clearly.
- Existing CLI parsing tests pass.

Suggested verification:

```bash
cargo fmt --all
DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo test --locked cli_definition_satisfies_clap_debug_assertions
PYTHONPATH=python python -m dcmview_py --help
target/debug/dcmview --help
```

## Phase 2: Troubleshooting Guide

Objective: give users a direct path from common symptoms to fixes.

Tasks:

- Add `docs/troubleshooting.md`.
- Cover install failures, missing binary resolution, missing Node/npm during
  source builds, no valid DICOM files found, skipped files, unsupported transfer
  syntaxes, port conflicts, browser launch failures, SSH tunnel failures, VS Code
  interception confusion, and annotation CSV validation errors.
- Include safe issue-reporting guidance that excludes PHI and sensitive data.
- Link the guide from the README install, quick start, and reporting sections.
- Link the guide from `vscode/README.md`.

Acceptance criteria:

- Each troubleshooting entry has symptom, likely cause, and fix.
- The guide explicitly repeats that DICOM files, screenshots, and logs may
  contain sensitive data.
- The README points users to troubleshooting before they need to file an issue.

Suggested verification:

```bash
grep -n "troubleshooting" README.md vscode/README.md
grep -n "PHI" docs/troubleshooting.md
```

## Phase 3: Configuration Reference

Objective: centralize all user-facing configuration surfaces and precedence
rules.

Tasks:

- Add `docs/configuration.md`.
- Document Rust CLI flags, Python `view()` parameters, Python module CLI flags,
  VS Code settings, and environment variables.
- Separate runtime variables from build/development variables.
- Document binary resolution order for Python and VS Code.
- Document VS Code bridge bypass/debug variables.
- Document `DCMVIEW_SKIP_FRONTEND_BUILD`, `DCMVIEW_NODE_PATH`, and
  `DCMVIEW_NPM_PATH` as build-only variables.
- Link from README, VS Code README, and any new docs index.

Acceptance criteria:

- A user can find every documented config knob from one page.
- Runtime and build-time variables are clearly separated.
- Binary override behavior is explicit for both Python and VS Code.

Suggested verification:

```bash
grep -n "DCMVIEW_BINARY" docs/configuration.md
grep -n "dcmview.binaryPath" docs/configuration.md
grep -n "DCMVIEW_SKIP_FRONTEND_BUILD" docs/configuration.md
```

## Phase 4: Python Wrapper Reference

Objective: make notebook/script usage clear for users who interact primarily
through `dcmview-py`.

Tasks:

- Add `docs/python.md`.
- Expand the `view()` docstring in `python/dcmview_py/wrapper.py`.
- Document parameters, return values, blocking behavior, handle lifecycle,
  context-manager usage, exceptions, binary resolution, VS Code bridge behavior,
  and bypass options.
- Add examples for blocking use, non-blocking use, context-manager use,
  annotation loading, filters, and remote/no-browser workflows.
- Link from README and PyPI metadata where appropriate.

Acceptance criteria:

- `help(dcmview_py.view)` is useful in a Python REPL.
- `docs/python.md` covers every public `view()` parameter.
- Python wrapper tests continue to pass.

Suggested verification:

```bash
PYTHONPATH=python python - <<'PY'
from dcmview_py import view
help(view)
PY
python -m unittest discover -s python/tests
```

## Phase 5: Internal API Documentation

Objective: document the viewer-internal API accurately without presenting it as
a supported external integration contract.

Tasks:

- Add `docs/api.md`.
- Move or mirror the detailed HTTP API reference from README into this file.
- State clearly that the API is internal to the viewer and intended for
  debugging/test automation only.
- Document `/api/files` progressive scan fields: `scan_complete`, `scanned`,
  `skipped`, and `filtered`.
- Document polling behavior for scripts and tests.
- Document cache headers, raw frame metadata headers, and error semantics.
- Document the `debug-api` feature and its build warning.
- Reduce README API content to a short summary plus a link, if Phase 7 is done
  in the same pass.

Acceptance criteria:

- API examples match current response structs.
- `debug-api` is documented as debugging-only.
- README no longer implies the API is a public stable integration surface.

Suggested verification:

```bash
grep -n "internal to the viewer" docs/api.md README.md
grep -n "scan_complete" docs/api.md
DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --features debug-api --locked
```

## Phase 6: Documentation Navigation

Objective: make the documentation set navigable as it grows.

Tasks:

- Add `docs/index.md`.
- Group docs into user, Python, VS Code, API/debugging, development, release, and
  internal design/planning sections.
- Decide whether to move feasibility and remediation plans under
  `docs/internal/`. If moving files, update links and keep the commit focused.
- Add links from README to `docs/index.md`.
- Add links from VS Code README to relevant user/config/troubleshooting pages.

Acceptance criteria:

- A user can start at README and find install, configuration, troubleshooting,
  Python, VS Code, and API/debugging docs.
- Internal planning docs are clearly marked as internal notes or moved under an
  internal folder.
- Links are relative and work on GitHub.

Suggested verification:

```bash
rg "\\]\\(" README.md docs vscode/README.md
```

## Phase 7: README Landing Page Cleanup

Objective: make the README concise and user-focused while preserving access to
technical detail through linked docs.

Tasks:

- Keep the README focused on product premise, safety note, install matrix, quick
  start, remote workflow, Python quick example, VS Code pointer, annotations
  summary, troubleshooting link, and reporting guidance.
- Move long API details, full annotation schema, transfer syntax matrix, and
  development architecture details to dedicated docs.
- Preserve important warnings about non-clinical use, secure networks, and
  unauthenticated local server access.
- Keep PyPI long-description readability in mind because `pyproject.toml` uses
  `README.md`.

Acceptance criteria:

- README is shorter and task-oriented.
- Detailed material is still available through linked docs.
- No install, safety, or reporting guidance is lost.

Suggested verification:

```bash
wc -l README.md
rg "SECURITY.md|troubleshooting|configuration|python|API" README.md
```

## Phase 8: Public Project Hygiene

Objective: add lightweight project governance without over-engineering for a
currently small project.

Tasks:

- Add `CONTRIBUTING.md` with setup, tests, fixture policy, docs expectations,
  and no-PHI issue guidance.
- Add top-level `CHANGELOG.md` for user-visible CLI, Python, VS Code, API/debug,
  and packaging changes.
- Add `.github/ISSUE_TEMPLATE/bug_report.md`.
- Add `.github/ISSUE_TEMPLATE/dicom_compatibility.md` with strong no-PHI
  guidance.
- Add `.github/ISSUE_TEMPLATE/feature_request.md`.
- Add `.github/pull_request_template.md` with tests/docs/security checklist.

Acceptance criteria:

- New issue templates route users away from posting sensitive data.
- Contributor instructions are realistic for the current project size.
- Changelog has an unreleased section and references existing VS Code changelog
  where appropriate.

Suggested verification:

```bash
rg "PHI|sensitive|DICOM" CONTRIBUTING.md .github/ISSUE_TEMPLATE
rg "Unreleased" CHANGELOG.md
```

## Phase 9: Homebrew v0.2.7 Preparation

Objective: prepare repository docs and release checklist for Homebrew without
claiming it is currently available.

Tasks:

- Keep README wording clear that Homebrew is planned for `v0.2.7`.
- Add a `v0.2.7` Homebrew checklist to `docs/releasing.md`.
- Document what owner-side configuration is still required:
  `HOMEBREW_TAP_REPOSITORY`, `HOMEBREW_TAP_TOKEN`, and tap repository setup.
- Do not add public Homebrew install commands until the tap exists.

Acceptance criteria:

- Maintainer release docs explain the Homebrew steps.
- Public user docs do not imply Homebrew is currently available.

Suggested verification:

```bash
rg "Homebrew" README.md docs/releasing.md
```

## Phase 10: Visual Onboarding

Objective: add screenshots/GIFs once a suitable non-sensitive dataset is
available.

Status: blocked on owner dataset selection.

Tasks after unblocking:

- Capture a synthetic or otherwise approved screenshot showing viewport, file
  navigation, tags, and ROI tools.
- Add screenshot to README.
- Add screenshot or GIF to VS Code Marketplace README.
- Consider using the same assets in PyPI long description and GitHub Release
  notes.
- Ensure all visual assets are free of PHI, patient identifiers, sensitive paths,
  and institution identifiers.

Acceptance criteria:

- Screenshot dataset is explicitly approved for public use.
- Distribution pages show the UI without exposing sensitive data.
- Visual assets are committed in a predictable assets/docs location.

## Suggested Execution Order

1. Phase 1: CLI help.
2. Phase 2: Troubleshooting.
3. Phase 3: Configuration.
4. Phase 4: Python wrapper reference.
5. Phase 5: Internal API documentation.
6. Phase 6: Documentation navigation.
7. Phase 7: README cleanup.
8. Phase 8: Public project hygiene.
9. Phase 9: Homebrew release prep.
10. Phase 10: Visual onboarding after dataset approval.

Phases 2 through 7 may be combined if docs are edited holistically, but commits
should remain split by logical unit. Phase 10 should not start until an approved
non-sensitive dataset is available.
