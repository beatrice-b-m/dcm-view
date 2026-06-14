# Current Progress

Date: 2026-06-14

## Durable State

- Recreated tracker from `CURRENT_PLAN.md`; prior tracker was absent.
- Current plan phases are treated as the source of truth.
- Working tree was clean before slice selection except for the absent tracker.

## Phase Status

- Phase 1: Command-Line Self-Service - complete.
- Phase 2: Troubleshooting Guide - complete.
- Phase 3: Configuration Reference - complete.
- Phase 4: Python Wrapper Reference - complete.
- Phase 5: Internal API Documentation - complete.
- Phase 6: Documentation Navigation - complete.
- Phase 7: README Landing Page Cleanup - complete.
- Phase 8: Public Project Hygiene - complete.
- Phase 9: Homebrew v0.2.7 Preparation - pending.
- Phase 10: Visual Onboarding - blocked on approved non-sensitive dataset.

## Completed Work

- Recreated this progress tracker because `CURRENT_PROGRESS.md` was missing.
- Improved Rust CLI help in `src/main.rs` with descriptive help text, value
  names, long examples for local, recursive, remote/no-browser, annotation, and
  filter workflows, plus a non-clinical-use note.
- Hid the integration-only `--startup-json` flag from normal Rust CLI help while
  keeping the flag available for wrappers and launch integrations.
- Improved Python module CLI help in `python/dcmview_py/__main__.py` with
  descriptive option help, value names, non-clinical-use guidance, and examples
  aligned with the Rust CLI including remote SSH forwarding.
- Added Python help-output coverage in `python/tests/test_wrapper.py` to keep
  the module CLI self-service and ensure `--startup-json` remains hidden from
  user-facing help.
- Aligned the README CLI reference with the Rust and Python help text, including
  value names, remote SSH forwarding, and the `python -m dcmview_py` invocation.
- Added `docs/troubleshooting.md` with symptom, likely cause, and fix guidance
  for install failures, binary resolution, source build prerequisites, discovery
  failures, skipped files, unsupported transfer syntaxes, port conflicts,
  browser launch failures, SSH tunnel issues, VS Code interception confusion,
  VS Code binary launch failures, and annotation CSV validation errors.
- Linked troubleshooting from the README install, quick start, and reporting
  sections.
- Linked troubleshooting from `vscode/README.md` for extension users.
- Added `docs/configuration.md` as a centralized reference for Rust CLI flags,
  Python module CLI flags, Python `view()` parameters, VS Code settings,
  runtime environment variables, build/development variables, binary resolution
  order, VS Code bridge bypass/debug variables, and the `debug-api` feature.
- Linked the configuration reference from the README install and CLI reference
  sections.
- Linked the configuration reference from `vscode/README.md` for extension
  settings, binary resolution, and bridge environment variables.
- Added `docs/python.md` with Python wrapper install notes, blocking and
  non-blocking examples, context-manager usage, parameter details, annotation
  loading, filters, remote/no-browser workflow, return values, exceptions,
  binary resolution, VS Code bridge behavior, bypass options, and related links.
- Expanded the `dcmview_py.view()` docstring so `help(dcmview_py.view)` exposes
  parameters, return values, handle lifecycle, exceptions, binary resolution,
  VS Code bridge behavior, and the non-clinical-use note.
- Added Python test coverage that keeps the public `view()` docstring
  informative.
- Linked the Python reference from the README Python and CLI sections, the VS
  Code README bridge guidance, and PyPI project metadata.
- Added `docs/api.md` documenting the viewer-internal HTTP API as a debugging
  and test-automation surface, including endpoint summaries, progressive
  `/api/files` scan fields, polling guidance, display-frame cache behavior,
  raw-frame metadata headers, tag value shapes, annotation semantics, error
  statuses, and the debugging-only `debug-api` feature.
- Linked the README HTTP API section to the dedicated internal API reference
  without doing the broader Phase 7 README cleanup.
- Added `docs/index.md` to group the documentation set into user guides,
  Python, VS Code, API/debugging, development/release, and internal planning
  notes.
- Linked the documentation index from the README install guidance and VS Code
  README so users can navigate to configuration, troubleshooting, Python,
  VS Code, API/debugging, and release references.
- Kept internal planning documents in place for this slice and labeled them as
  maintainer-oriented notes from the docs index; no file moves were needed for
  the navigation outcome.
- Tightened the README landing page from 502 lines to 311 lines while keeping
  the public overview, install matrix, quick start, remote workflow, Python
  pointer, VS Code pointer, viewer features, annotations summary,
  troubleshooting link, reporting guidance, and non-clinical-use warnings.
- Replaced the long README CLI option table with direct help-command guidance
  and links to the configuration and Python references.
- Reduced the README HTTP API section to a viewer-internal stability warning,
  `debug-api` caution, and link to the dedicated internal API reference.
- Added `docs/annotations.md` with the EMBED-style ROI CSV contract, required
  and optional columns, coordinate and frame formats, examples, validation
  failures, export behavior, and API shape.
- Added `docs/development.md` with source-build prerequisites, build script
  behavior, frontend proxy workflow, Rust and frontend checks, Python wrapper
  tests, fixture policy, architecture summary, and cache budget guidance.
- Linked the annotation and development references from `docs/index.md`.
- Added `CONTRIBUTING.md` with contributor setup, relevant test commands,
  fixture policy, documentation expectations, pull request expectations, and
  explicit no-PHI/no-sensitive-DICOM guidance.
- Linked the contributor guide from `docs/index.md`.
- Added a top-level `CHANGELOG.md` with an Unreleased section covering
  user-visible CLI, Python, VS Code, API/debugging, documentation, and packaging
  changes.
- Referenced the existing VS Code Marketplace-focused changelog from the new
  root changelog.
- Linked the root changelog from `docs/index.md`.
- Added `.github/ISSUE_TEMPLATE/bug_report.md` with a structured reproducible
  bug report flow and explicit no-PHI, no-sensitive-DICOM, redaction, and
  non-clinical-use guidance.
- Added `.github/ISSUE_TEMPLATE/dicom_compatibility.md` with DICOM-specific
  compatibility prompts, non-sensitive metadata fields, public/synthetic
  reproduction guidance, and explicit no-PHI/no-sensitive-DICOM guardrails.
- Added `.github/ISSUE_TEMPLATE/feature_request.md` with enhancement-request
  prompts for problem, proposed behavior, alternatives, scope, and explicit
  no-PHI/no-sensitive-DICOM guardrails.
- Added `.github/pull_request_template.md` with summary, verification,
  documentation, safety/data-hygiene, area-specific checks, and follow-up
  prompts.

## Blockers

- Phase 10 must not start until an approved public, non-sensitive dataset is selected.

## Open Decisions

- None for the current slice.

## Verification Results

- `cargo fmt --all` - passed.
- `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo test --locked cli_definition_satisfies_clap_debug_assertions` - passed; build emitted the existing macOS `xcrun` temp-dir warning.
- `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --locked` - passed; build emitted the existing macOS `xcrun` temp-dir warning.
- `target/debug/dcmview --help` - passed by inspection; help includes remote workflow examples and does not show `--startup-json`.
- `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo test --locked launcher_cli_flags_exist_on_clap_contract` - passed; hidden integration flag remains part of the clap contract.
- `PYTHONPATH=python python -m dcmview_py --help` - passed by inspection; help explains each option, includes remote workflow examples, and does not show `--startup-json`.
- `python -m unittest discover -s python/tests` - passed; 51 tests run, 1 skipped.
- `grep -n "troubleshooting" README.md vscode/README.md` - passed; both user
  entry points link to the guide.
- `grep -n "PHI" docs/troubleshooting.md` - passed; guide explicitly warns that
  public reports must not include PHI or sensitive DICOM content.
- `rg -n "Symptom:|Likely cause:|Fix:" docs/troubleshooting.md` - passed; each
  troubleshooting entry uses the planned structure.
- `rg -n "SECURITY.md|dcmview --no-browser" docs/troubleshooting.md` - passed;
  the guide links back to the top-level security policy and keeps the manual
  remote workflow command readable.
- `git diff --check -- docs/troubleshooting.md README.md vscode/README.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `grep -n "DCMVIEW_BINARY" docs/configuration.md` - passed; Python binary
  override and runtime variable are documented.
- `grep -n "dcmview.binaryPath" docs/configuration.md` - passed; VS Code binary
  override is documented.
- `grep -n "DCMVIEW_SKIP_FRONTEND_BUILD" docs/configuration.md` - passed; the
  build-only frontend skip variable is documented.
- `grep -n "configuration" README.md vscode/README.md` - passed; both user
  entry points link to the configuration reference.
- `git diff --check -- docs/configuration.md README.md vscode/README.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `PYTHONPATH=python python - <<'PY'
from dcmview_py import view
help(view)
PY` - passed by inspection; help now includes arguments, return values,
  exceptions, VS Code bridge behavior, binary resolution, and non-clinical-use
  guidance.
- `python -m unittest discover -s python/tests` - passed; 52 tests run, 1
  skipped.
- `grep -n "Python reference" README.md vscode/README.md` - passed; README and
  VS Code README link to the Python reference.
- `grep -n "Documentation" pyproject.toml` - passed; PyPI metadata links to the
  Python reference.
- `git diff --check -- docs/python.md README.md vscode/README.md pyproject.toml python/dcmview_py/wrapper.py python/tests/test_wrapper.py CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `grep -n "internal to the viewer" docs/api.md README.md` - passed; both the
  dedicated API reference and README mark the API as viewer-internal.
- `grep -n "scan_complete" docs/api.md` - passed; progressive scan completion
  and polling behavior are documented.
- `grep -n "X-Frame-Rows" docs/api.md` - passed; raw-frame metadata headers are
  documented.
- `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --features debug-api --locked` -
  passed; build emitted the existing macOS `xcrun` temp-dir warning and the
  intended `debug-api` permissive-CORS warning.
- `git diff --check -- docs/api.md README.md CURRENT_PROGRESS.md` - passed;
  command emitted the existing macOS temp-dir warning only.
- `rg -n "\\]\\(" README.md docs vscode/README.md` - passed by inspection;
  relative documentation links are visible across README, docs, and the VS Code
  README.
- `grep -n "documentation index" README.md vscode/README.md` - passed; both
  public entry points link to `docs/index.md`.
- `grep -n "Internal Planning Notes" docs/index.md` - passed; maintainer notes
  are grouped separately from user-facing guides.
- `git diff --check -- docs/index.md README.md vscode/README.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `git diff --check -- README.md docs/annotations.md docs/development.md docs/index.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `wc -l README.md` - passed; README is now 311 lines.
- `rg -n "annotation reference|development reference|internal API reference|troubleshooting guide|configuration reference|documentation index|SECURITY.md" README.md`
  - passed; README links to the dedicated references and reporting guidance.
- `rg -n "ROI_coords|ROI_frames|num_ROI|anon_dicom_path" docs/annotations.md`
  - passed; annotation CSV columns and JSON fields are documented.
- `rg -n "Transfer syntax|X-Cache|debug-api|scan_complete|X-Frame-Rows" docs/api.md`
  - passed; API details moved out of README remain available in the dedicated
  reference.
- `rg -n "Development reference|Annotation reference" docs/index.md` - passed;
  the docs index links the new reference pages.
- `rg -n "\\]\\(" README.md docs vscode/README.md` - passed by inspection;
  relative links include the new annotation and development references.
- `git diff --check -- README.md docs/annotations.md docs/development.md docs/index.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `rg -n "PHI|sensitive|DICOM" CONTRIBUTING.md` - passed; contributor guide
  explicitly covers DICOM data, PHI, sensitive reports, fixture policy, and PR
  confirmation.
- `rg -n "Contributing|CONTRIBUTING" docs/index.md CONTRIBUTING.md README.md`
  - passed; the contributor guide exists and is linked from the documentation
  index.
- `git diff --check -- CONTRIBUTING.md docs/index.md CURRENT_PROGRESS.md` -
  passed; command emitted the existing macOS temp-dir warning only.
- `rg -n "Unreleased|vscode/CHANGELOG.md|CLI|Python|VS Code|API And Debugging|Documentation And Packaging" CHANGELOG.md`
  - passed; the root changelog has an Unreleased section, public-surface
  headings, and references the VS Code changelog.
- `rg -n "Changelog|CHANGELOG" docs/index.md CHANGELOG.md vscode/CHANGELOG.md`
  - passed; the documentation index links the root changelog and both changelog
  files are discoverable.
- `git diff --check -- CHANGELOG.md docs/index.md CURRENT_PROGRESS.md` -
  passed; command emitted the existing macOS temp-dir warning only.
- `rg -n "PHI|sensitive|DICOM|redact|clinical" .github/ISSUE_TEMPLATE/bug_report.md`
  - passed; the bug report template includes no-PHI, no-sensitive-DICOM,
  redaction, and non-clinical-use guidance.
- `git diff --cached --check` - passed after staging the bug report template
  and tracker update; command emitted the existing macOS temp-dir warning only.
- `rg -n "PHI|sensitive|DICOM|redact|clinical|synthetic|de-identified" .github/ISSUE_TEMPLATE/dicom_compatibility.md`
  - passed; the compatibility template includes no-PHI, no-sensitive-DICOM,
  redaction, synthetic/de-identified reproduction, and non-clinical-use
  guidance.
- `rg -n "Transfer syntax UID|Photometric interpretation|Number of frames|Annotation CSV" .github/ISSUE_TEMPLATE/dicom_compatibility.md`
  - passed; the compatibility template prompts for DICOM-specific technical
  fields without asking for sensitive dataset content.
- `git diff --cached --check` - passed after staging the compatibility template
  and tracker update; command emitted the existing macOS temp-dir warning only.
- `rg -n "PHI|sensitive|DICOM|redact|clinical|synthetic|de-identified" .github/ISSUE_TEMPLATE/feature_request.md`
  - passed; the feature request template includes no-PHI, no-sensitive-DICOM,
  redaction, synthetic/de-identified example, and non-clinical-use guidance.
- `rg -n "Problem|Proposed behavior|Alternatives considered|Scope" .github/ISSUE_TEMPLATE/feature_request.md`
  - passed; the template prompts users for the workflow need, expected
  behavior, existing workaround, and affected surface.
- `git diff --check -- .github/ISSUE_TEMPLATE/feature_request.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.
- `rg -n "PHI|sensitive|DICOM" CONTRIBUTING.md .github/ISSUE_TEMPLATE .github/pull_request_template.md`
  - passed; contributor and GitHub templates continue to include no-PHI,
  no-sensitive-DICOM, and sensitive-data guidance.
- `rg -n "Verification|Documentation|Safety And Data Hygiene|cargo fmt|typecheck|debug-api" .github/pull_request_template.md`
  - passed; the pull request template includes verification, docs,
  safety/data-hygiene, and area-specific checklist prompts.
- `git diff --check -- .github/pull_request_template.md CURRENT_PROGRESS.md`
  - passed; command emitted the existing macOS temp-dir warning only.

## Commit-Ready Summary

- Commit `.github/pull_request_template.md` and `CURRENT_PROGRESS.md` for the
  Phase 8 pull request template slice.

## Next Recommended Action

- Start Phase 9 by adding the `v0.2.7` Homebrew checklist to
  `docs/releasing.md`, including owner-side prerequisites for
  `HOMEBREW_TAP_REPOSITORY`, `HOMEBREW_TAP_TOKEN`, and tap repository setup,
  while keeping public docs clear that Homebrew is planned but not currently
  available.
