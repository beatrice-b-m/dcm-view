# Current Progress

Date: 2026-06-14

## Durable State

- Recreated tracker from `CURRENT_PLAN.md`; prior tracker was absent.
- Current plan phases are treated as the source of truth.
- Working tree was clean before slice selection except for the absent tracker.

## Phase Status

- Phase 1: Command-Line Self-Service - complete.
- Phase 2: Troubleshooting Guide - pending.
- Phase 3: Configuration Reference - pending.
- Phase 4: Python Wrapper Reference - pending.
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

## Commit-Ready Summary

- Commit `README.md`, `python/dcmview_py/__main__.py`,
  `python/tests/test_wrapper.py`, and `CURRENT_PROGRESS.md` for the Python CLI
  help and README CLI alignment slice.

## Next Recommended Action

- Start Phase 2 by adding `docs/troubleshooting.md` with symptom, likely cause,
  fix, and safe no-PHI issue-reporting guidance for the planned troubleshooting
  entries.
