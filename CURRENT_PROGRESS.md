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
- Phase 5: Internal API Documentation - pending.
- Phase 6: Documentation Navigation - pending.
- Phase 7: README Landing Page Cleanup - pending.
- Phase 8: Public Project Hygiene - pending.
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

## Commit-Ready Summary

- Commit `docs/python.md`, `python/dcmview_py/wrapper.py`,
  `python/tests/test_wrapper.py`, `README.md`, `vscode/README.md`,
  `pyproject.toml`, and `CURRENT_PROGRESS.md` for the Python wrapper reference
  slice.

## Next Recommended Action

- Start Phase 5 by adding `docs/api.md` for the viewer-internal HTTP API,
  including progressive scan fields, polling behavior, cache headers, raw-frame
  metadata headers, error semantics, and the debugging-only `debug-api` feature.
