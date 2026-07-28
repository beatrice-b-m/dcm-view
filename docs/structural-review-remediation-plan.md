# Structural Review Remediation Plan

> **Archival note:** This is a point-in-time, non-normative remediation plan,
> not a current completion checklist. Several contract, startup, and packaging
> changes described below have since landed, while other ideas may remain
> partial or unimplemented. See the [current architecture](architecture.md) and
> [development reference](development.md).

This plan responds to the structural review findings around rendering semantics,
contract ownership, wrapper protocols, extension packaging, and development
workflow drift. I spot-checked the current tree before writing this plan; the
main findings still reproduce.

## Review Assessment

### Confirmed high-priority issues

1. Rendering semantics are split between backend PNG rendering and frontend raw
   frame rendering. The frontend inverts `MONOCHROME1` frames, while the backend
   display-frame paths do not currently handle `MONOCHROME1`.
2. Frontend TypeScript types mirror Rust API types by hand. The repository has
   `npm --prefix frontend run typecheck`, but CI does not run it.
3. Raw-frame metadata headers are contract-tested on the Rust side, but the
   frontend silently defaults required metadata such as rows, columns, bit
   depth, pixel representation, samples per pixel, photometric interpretation,
   and rescale values.

### Confirmed medium-priority issues

4. The structured startup JSON event exists and is used by the VS Code
   extension, but the Python wrapper still relies on the human-readable startup
   line.
5. The VS Code bridge protocol is implemented independently in the extension,
   Rust bridge client, and Python bridge client. The suites test each copy
   locally, but there is no shared cross-implementation contract.
6. The VS Code extension is structurally in between "dev-only" and "released
   product": it is private, version sync excludes it, release automation does
   not package it, and the extension sets `DCMVIEW_VSCODE_BINARY` although no
   client reads it.

### Confirmed low-priority issues

7. `build.rs` mutates `frontend/dist` and `frontend/node_modules` in the source
   checkout. The implementation is careful, but stale or concurrent `dist`
   hazards remain.
8. CLI flags are duplicated across clap, Python argparse/wrapper code, VS Code
   interception code, and docs.
9. `/api/health` exists and is tested but is not documented or used by the
   extension readiness path.
10. `npm --prefix frontend run dev` has no API proxy, so the standalone Vite
    dev server cannot exercise the app against a backend without manual setup.

## Guiding Decisions

- Keep the Rust binary as the product center. Python and VS Code should remain
  launchers, not alternate DICOM implementations.
- Keep both frame transports: server-rendered PNGs for cine/read-only display
  and raw frames for responsive window/level interaction.
- Keep raw-frame samples source-faithful. Clients continue to apply photometric
  interpretation from metadata; the backend PNG path must be fixed to match.
- Treat `types.rs` as the source of truth for API types, generate checked-in
  TypeScript types from it, and verify generated-file freshness in CI.
- Package bundled platform binaries in target-specific VSIX artifacts, while
  keeping `dcmview.binaryPath` and `PATH` as overrides/fallbacks.
- Make contracts explicit and tested where packages meet: pixel semantics,
  HTTP JSON and headers, startup output, CLI flags, and bridge wire messages.
- Prefer small, independently verifiable changes. Each phase below can land in
  one or more granular commits.

## Phase 1: Pixel Semantics Correctness

Goal: make server PNG rendering and frontend raw rendering agree for grayscale
photometric/windowing behavior.

Implementation steps:

1. Add a `MONOCHROME1` fixture using the existing
   `write_uncompressed_u16_dicom_with_photometric` test helper.
2. Add a failing integration test that requests the same frame through
   `/api/file/:i/frame/:n` and `/api/file/:i/frame/:n/raw`, renders or inspects
   both paths, and proves the expected inversion semantics.
3. Make the backend display path handle `MONOCHROME1` explicitly after
   windowing. This preserves the raw-frame API as decoded source samples plus
   metadata and keeps the frontend renderer's current photometric behavior.
4. Add targeted tests for default-window and `mode=full_dynamic` behavior so
   inversion does not bypass the established window resolution order.

Acceptance checks:

- `cargo test --test integration pixels_uncompressed`
- `cargo test --test integration api_contract`
- A manual spot check of one `MONOCHROME1` fixture in both cine and diagnostic
  window/level modes if a real-world sample is available.

## Phase 2: Frontend Contract Enforcement

Goal: catch frontend API/type drift before runtime.

Implementation steps:

1. Add `npm --prefix frontend ci` and `npm --prefix frontend run typecheck` to
   CI before Rust tests or immediately after Node setup.
2. Change `fetchRawFrame` so required `X-Frame-*` headers must be present and
   parseable. Keep defaults only for optional `X-Frame-Default-Wc` and
   `X-Frame-Default-Ww`.
3. Add a small frontend unit or TypeScript-level test helper for parsing raw
   metadata if the current frontend test setup can support it cheaply. If not,
   keep the function factored and covered through `typecheck` plus Rust
   contract tests.
4. Add checked-in generated TypeScript API types from Rust, using `types.rs` as
   the source of truth. Prefer a regeneration command plus a CI drift check over
   making normal frontend builds depend on Rust type generation.
5. Move `api.ts` to import or re-export the generated contract types instead of
   maintaining duplicate hand-written interfaces where generation applies.

Acceptance checks:

- `npm --prefix frontend run typecheck`
- `npm --prefix frontend run build`
- `cargo test --test integration api_contract`
- CI drift check for generated TypeScript types

## Phase 3: Startup Protocol Consolidation

Goal: make structured startup output the contract and leave the human startup
line as user-facing text only.

Implementation steps:

1. Update the Python wrapper to launch the binary with `--startup-json`.
2. Parse `{"type":"server_started","url":...,"host":...,"port":...}` in the
   wrapper reader thread.
3. Keep the legacy `dcmview: server running at ...` parser as a fallback for
   `DCMVIEW_BINARY` pointing at older binaries.
4. Add Python tests for structured startup parsing, fallback parsing, malformed
   JSON lines, and timeout behavior.
5. Keep or extend the existing Rust startup event test as the source of truth
   for event fields.

Acceptance checks:

- `python -m unittest python.tests.test_wrapper`
- `cargo test --test integration server_minimal`

## Phase 4: Bridge Protocol Contract

Goal: prevent the VS Code bridge server, Rust client, and Python client from
drifting independently.

Implementation steps:

1. Decide the bridge source of truth. The lowest-friction option is a checked-in
   JSON fixture file containing canonical `/launch`, `/sessions/:id/wait`, and
   `/sessions/:id/stop` request/response examples plus auth expectations.
2. Add Rust tests asserting the bridge client serializes requests and parses
   responses according to the fixture.
3. Add Python tests asserting the same fixture against `_bridge_json_request`
   request construction and response handling.
4. Extract the VS Code bridge handler enough to test it without a full VS Code
   extension host, or add focused tests that assert its wire responses match the
   fixture.
5. Resolve `DCMVIEW_VSCODE_BINARY`: either make bridge clients read it for
   terminal interception, or remove it from the extension environment.

Acceptance checks:

- `cargo test`
- `python -m unittest python.tests.test_wrapper`
- `npm --prefix vscode run compile`
- `npm --prefix vscode test`

## Phase 5: VS Code Extension Marketplace Release

Goal: package target-specific VSIX artifacts and publish them to the VS Code
Marketplace from GitHub Release assets.

Implementation steps:

1. Add VSIX packaging scripts to `vscode/package.json`, using `vsce package`
   with target-specific Marketplace settings.
2. Add `.vscodeignore` so the VSIX contains the compiled extension runtime,
   package metadata, Marketplace docs, and bundled binaries, but excludes
   source, tests, transient build output, and package-manager caches.
3. Populate one `vscode/resources/bin/<platform>-<arch>/dcmview` binary at a
   time from the existing release binaries for the supported Marketplace
   platforms:
   `linux-x64`, `darwin-x64`, and `darwin-arm64`.
4. Package target-specific VSIX artifacts and verify each contains only its
   matching platform binary.
5. Keep `dcmview.binaryPath` as the first lookup override and `PATH` as the
   fallback after bundled binaries. Do not make the extension install or depend
   on `dcmview-py`/pip at runtime.
6. Include `vscode/package.json` in version synchronization so the VSIX version
   tracks the Rust binary and Python package during releases.
7. Document Marketplace publishing through Azure Pipelines, including the
   bundled-binary platforms and the `dcmview.binaryPath` escape hatch.

Acceptance checks:

- `python scripts/check_versions.py`
- `npm --prefix vscode run compile`
- `npm --prefix vscode test`
- VSIX package contains compiled runtime files and the expected
  target-specific `resources/bin/**` binary
- Updated docs in `docs/vscode-extension-local-testing.md` and `docs/releasing.md`

## Phase 6: Low-Risk Developer Workflow Hardening

Goal: clean up smaller drift surfaces after the correctness and protocol work.

Implementation steps:

1. Add a Rust test or script that compares wrapper-used CLI flags with
   `Cli::command()` output.
2. Document `/api/health` in the README API section or wire the extension to use
   it for readiness after startup URL discovery.
3. Add a Vite dev-server proxy option for `/api` and document the expected
   backend URL/port workflow.
4. Investigate moving frontend build output to `$OUT_DIR` for Cargo builds. Keep
   this optional until the higher-priority contract fixes land, because it
   touches build and embedding behavior.
5. Add comments or docs cross-referencing server and frontend cache budgets so
   future tuning considers total memory pressure.

Acceptance checks:

- `cargo test`
- `npm --prefix frontend run typecheck`
- `npm --prefix frontend run build`
- `npm --prefix vscode test`

## Suggested Execution Order

1. Phase 1, because it fixes a confirmed user-visible rendering discrepancy.
2. Phase 2, because it adds cheap CI coverage and makes later contract work less
   fragile.
3. Phase 3, because it reuses an existing structured protocol and simplifies
   startup ownership.
4. Phase 4, because it is broader and spans all launcher surfaces.
5. Phase 5, because it is a product/release decision rather than a correctness
   fix.
6. Phase 6, because these are useful guardrails but lower-risk than rendering
   and protocol drift.

## Resolved Decisions

1. Raw-frame samples remain faithful to source photometric interpretation.
   Clients apply inversion from metadata, and the backend PNG path is fixed to
   apply matching `MONOCHROME1` inversion after windowing.
2. The next release includes VSIX packaging for broader closed testing. The
   VSIX bundles supported platform binaries and does not depend on pip at
   runtime, though release automation can reuse the same binary artifacts that
   feed the Python wheels.
3. Rust-to-TypeScript API type generation uses checked-in generated artifacts
   verified by CI, not build-time generation in every normal frontend build.
