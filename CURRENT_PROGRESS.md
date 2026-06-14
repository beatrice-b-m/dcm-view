# Current Progress

Date: 2026-06-14

## Durable State

- Recreated tracker from `CURRENT_PLAN.md`; prior tracker was absent.
- Current plan phases are treated as the source of truth.
- Working tree was clean before slice selection except for the absent tracker.

## Phase Status

- Phase 1: Command-Line Self-Service - in progress; Rust CLI help slice complete.
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

## Commit-Ready Summary

- Commit `src/main.rs` and `CURRENT_PROGRESS.md` for the Rust CLI help slice.

## Next Recommended Action

- Continue Phase 1 with the Python module CLI help in `python/dcmview_py/__main__.py`, then align the README CLI table with the completed Rust and Python help text.
