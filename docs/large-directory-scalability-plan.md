# Large Directory Scalability and Hierarchy Filtering Plan

> **Archival note:** This is a point-in-time, non-normative plan. The current
> server binds before progressive discovery, and normal shutdown cancels and
> awaits remaining scan work; other proposals below may be partial or
> unimplemented. See the [current architecture](architecture.md) and
> [configuration reference](configuration.md).

This plan addresses the report that `dcmview` hangs and fails to launch when
pointed at directories with thousands of files, and adds user-facing filtering
of patient IDs, study attributes, and series attributes. The root causes below
were verified against the current tree before writing this plan.

## Problem Assessment

### Confirmed root causes

1. **The loader reads every file in its entirety before anything else
   happens.** `build_entry` calls `dicom_object::open_file(path)`
   (`src/loader.rs:94`), which parses the whole DICOM object into memory —
   including `PixelData` — to extract roughly twenty header values. A
   directory of thousands of files, especially multi-frame, means gigabytes
   of I/O before the server exists.
2. **The server does not bind until the scan completes.** `run()` awaits
   `loader::discover` before constructing `AppState` and calling
   `server::run` (`src/main.rs:165-221`). The VS Code extension waits only
   `startupTimeoutSeconds` (default 20 s, `vscode/src/extension.ts:244`) for
   the startup line and fails the launch when it does not arrive
   (`vscode/src/extension.ts:538`). The Python wrapper's
   `wait_for_url(timeout)` behaves the same way. A slow scan therefore
   presents as "fails to launch" even though the binary is still working.
3. **The frontend navigator does not scale.** Every tree node defaults to
   expanded (`isCollapsed` returns `false`,
   `frontend/src/lib/FileNavigator.svelte:80-82`), so the browser renders one
   DOM button per file on first paint. The tree build uses linear
   `Array.find` per file for study and series lookup (lines 228 and 241),
   which is quadratic within groups, and re-sorts the already-sorted file
   list on every rebuild (line 213).
4. **There is no way to narrow the working set.** There is no filtering of
   patients, studies, or series in the UI, the HTTP API, or the CLI. The only
   existing filter is the client-side tag filter in
   `frontend/src/lib/TagPanel.svelte:180-225`.

### Non-causes worth recording

- The scan is already parallel (`rayon::par_iter` at `src/loader.rs:59-62`)
  and already off the async executor (`spawn_blocking` at
  `src/loader.rs:16`). Parallelism is not the gap; per-file cost and startup
  ordering are.
- Non-DICOM files are already tolerated: `build_entry` returns `Ok(None)` on
  any open error and the file is counted in `skipped`. Pre-filtering by DICM
  magic (Phase 1) preserves this behavior exactly while skipping parser
  setup.

## Guiding Decisions

- Keep `dcmview` ephemeral: no persistent on-disk index or metadata cache.
- Do not paginate `/api/files`. Hierarchy grouping happens client-side and a
  series may straddle pages; header-only metadata is small enough to ship
  whole. Filtering and progressive load solve the real problem.
- File indices remain the cache and annotation key. Once an index is
  assigned to a file it must never change for the lifetime of the process.
  Under progressive discovery (Phase 4) this means indices are assigned at
  insertion time in append order, and the global path sort at
  `src/loader.rs:81-84` is removed. Display ordering is a frontend concern.
- Hierarchy filtering is client-side first (all `FileSummary` metadata is
  already in the browser), with CLI scan-time filters as the scale valve.
  Do not add query-parameter filtering to `/api/files` in this plan.
- `src/types.rs` is the source of truth for API types. Any change to
  `FilesResponse` (`src/types.rs:115`) must be followed by regenerating
  `frontend/src/generated/api-types.ts` via
  `python3 scripts/generate_frontend_types.py`.
- Prefer small, independently landable phases. Phases 1-3 are independent of
  each other; Phase 4 is the structural change and lands last among the core
  phases.

## Phase 1: Header-Only Metadata Reads

Goal: cut per-file scan cost from "entire file" to a few KB of header,
typically a 10-100x I/O reduction. This is the single biggest win and turns
most "hangs" into "slow" on its own.

Implementation steps:

1. In `build_entry` (`src/loader.rs:93`), replace `open_file(path)` with a
   read that stops before pixel data:
   `OpenFileOptions::new().read_until(dicom_dictionary_std::tags::PIXEL_DATA).open_file(path)`
   (dicom-object 0.9). Every element `build_entry` reads sits before
   `PixelData` in tag order, so no other extraction changes.
2. Add a pre-parse magic check before calling the DICOM parser: read the
   first 132 bytes of the candidate and require bytes 128..132 to equal
   `DICM`. On mismatch, return `Ok(None)` so the file counts toward
   `skipped` exactly as it does today. (`open_file` already rejects
   preamble-less files, so this changes no observable behavior.)
3. Replace the `has_pixels` check (`src/loader.rs:121`), which a
   `read_until` object cannot answer. Preferred mechanism: wrap the input in
   a byte-counting `Read` adapter, parse via the reader-based open path with
   the same `read_until` option, and set
   `has_pixels = bytes_consumed < file_len` (unconsumed trailing bytes after
   the last header element are the pixel data element). If the reader-based
   API proves awkward, the fallback heuristic
   `has_pixels = rows > 0 && columns > 0` is acceptable **only** together
   with step 4's new fixture.
4. Extend `examples/generate_test_fixtures.rs` with a fixture that has
   `Rows`/`Columns` populated but no `PixelData` element, and assert
   `has_pixels == false` for it in the loader tests. The existing
   `golden-no-pixels-sr.dcm` is an SR without `Rows`/`Columns` and does not
   exercise this edge.
5. Verify the scan-time improvement manually: build a throwaway directory of
   a few thousand copies of the golden fixtures and compare wall-clock
   startup before and after (`time dcmview --no-browser --timeout 1 <dir>`).
   Record numbers in the PR description.

Acceptance checks:

- `cargo test --test integration loader_discovery`
- `cargo test --test integration golden_fixtures`
- `cargo test` (full suite; pixel endpoints re-open files themselves and must
  be unaffected)
- `has_pixels` is correct for `golden-no-pixels-sr.dcm`, the new
  rows-but-no-pixels fixture, and all pixel-bearing fixtures.

## Phase 2: Navigator Tree Scalability

Goal: keep the browser responsive at tens of thousands of files without a
virtualization library, by capping initial DOM size and removing quadratic
work. Frontend-only; no API changes.

Implementation steps:

1. In the `tree` derivation (`frontend/src/lib/FileNavigator.svelte:210-262`),
   replace the `patient.studies.find(...)` and `study.series.find(...)`
   linear scans (lines 228 and 241) with `Map` lookups keyed by the node keys
   already being computed. Keep the output shape (`NavPatient[]`) unchanged.
2. Remove the `[...files].sort(...)` at line 213. The backend delivers files
   in index order; iterate `files` directly. If within-series ordering needs
   improving, sort each `series.files` by numeric `instance_number` (with
   index as tiebreak) at the end of the build, not the whole input.
3. Add scale-aware default collapse. When `files.length` exceeds a threshold
   (suggested: 500), treat patient and study nodes as collapsed by default.
   Implement by switching `collapsedNodes` semantics from "collapsed set"
   to an explicit override map consulted by `isCollapsed` with a
   scale-dependent default, so user toggles still win. The existing
   `{#if !isCollapsed(...)}` guards (lines 304, 317, 330) already prevent
   rendering of collapsed subtrees, so this alone caps first paint at the
   patient count.
4. When only one patient exists, keep it expanded regardless of file count
   so the common single-study case looks unchanged.

Acceptance checks:

- `npm --prefix frontend run typecheck`
- `npm --prefix frontend run build`
- Manual check against a generated multi-thousand-file directory: first
  paint is immediate, tree shows collapsed patients with correct counts,
  expanding nodes is responsive, small directories look unchanged.

## Phase 3: Hierarchy Filter UI

Goal: let the user filter the navigator by patient ID, study attributes, and
series attributes. Frontend-only; all needed metadata is already present in
`FileSummary`.

Implementation steps:

1. Add a filter text input to the navigator header area of
   `FileNavigator.svelte`, styled consistently with the tag filter input in
   `TagPanel.svelte` (line 298) and visible only when the navigator is not
   collapsed.
2. Filter the flat `files` array **before** the `tree` derivation so the
   tree, counts, and DOM all shrink together. Matching rules:
   - Free text matches case-insensitive substrings across: `patient_id`,
     `patient_name`, `study_description`, `study_date`,
     `study_instance_uid`, `series_description`, `series_number`,
     `modality`.
   - Scoped terms narrow to one field group: `patient:<text>` (patient ID
     and name), `study:<text>` (study description, date, UID),
     `series:<text>` (series description, number, UID), and
     `modality:<text>`. Multiple whitespace-separated terms AND together.
3. Show a result line under the input when a filter is active:
   `showing N of M images`. Never hide files silently.
4. When a filter is active, render matching subtrees expanded (filter
   results that arrive collapsed read as empty), and restore normal
   collapse defaults when the filter clears.
5. Preserve selection behavior: if the active file is filtered out, keep it
   open in the viewer; only the navigator narrows.

Acceptance checks:

- `npm --prefix frontend run typecheck` and `npm --prefix frontend run build`
- Manual checks: free-text and scoped filters narrow correctly; counts
  update; clearing the filter restores the prior tree; filtering by a
  patient ID in a multi-patient directory isolates that patient.

## Phase 4: Progressive Startup

Goal: bind the HTTP server, emit the startup line/JSON, and serve the UI
immediately, while discovery streams results in the background. This removes
the launcher-timeout failure mode entirely and gives the user feedback during
long scans.

Implementation steps:

1. Introduce a shared file registry to replace the immutable
   `Arc<Vec<FileEntry>>` + precomputed `file_summaries` pair in `AppState`
   (`src/main.rs:195-208`). Suggested shape: a `FileRegistry` holding
   `RwLock<Vec<Arc<FileEntry>>>`, a parallel `RwLock<Vec<FileSummary>>`
   appended in lockstep, scan counters (`scanned`, `skipped`), and a
   `scan_complete: AtomicBool`. Index = insertion position, assigned once,
   never reordered (see Guiding Decisions). All existing handlers that look
   files up by index switch to reading through the registry.
2. Restructure `run()` in `src/main.rs`: construct `AppState` with an empty
   registry, start `server::run` immediately, and spawn discovery as a
   background task. Keep the rayon parallel parse, but deliver entries
   through an `mpsc` channel (or append under the write lock in batches of
   ~64 files or ~250 ms, whichever first) instead of collecting a final
   `Vec`. Move `print_load_summary` (`src/main.rs:174`) to scan completion.
3. Annotations (`src/main.rs:175-180`): `load_annotations_for_files` matches
   CSV rows to files by normalized path and validates frame ranges
   (`src/annotations.rs:116-128`). Parse and normalize the CSV rows once at
   startup; match and validate per file as it is inserted into the registry,
   inserting into the `AnnotationStore` incrementally. A CSV row that never
   matches any scanned file is reported at scan completion, matching today's
   semantics as closely as possible.
4. Preserve the zero-files contract. Today discovery errors with
   `dcmview: no valid DICOM files found` before the server starts
   (`src/loader.rs:77-79`). Under progressive startup: open the browser only
   after the first file is registered (or scan completion, whichever first),
   and if the scan completes with zero files, shut the server down and exit
   nonzero with the same message. CLI behavior for the empty case is then
   indistinguishable from today.
5. Extend `FilesResponse` (`src/types.rs:115`) with `scan_complete: bool`,
   `scanned: usize`, and `skipped: usize`. Regenerate the frontend types
   with `python3 scripts/generate_frontend_types.py` and update the
   `api_contract` integration tests.
6. Frontend: after the initial `fetchFiles()` (`frontend/src/api.ts:55`),
   poll `/api/files` every ~500 ms while `scan_complete` is false, then
   stop. Show a progress line in the navigator while scanning
   ("indexed 3,214 files..."). The Phase 2/3 tree build and filtering apply
   to each snapshot unchanged.
7. Add an integration test that drives discovery through a controllable
   slow path (e.g., a fixture directory plus an injected per-file delay or a
   large generated directory) and asserts: the server answers
   `/api/health` and `/api/files` while `scan_complete` is false; file
   indices visible in an early snapshot are identical in the final
   snapshot; pixel and tag endpoints work for already-registered files
   mid-scan.

Acceptance checks:

- `cargo test` (including the new mid-scan integration test and updated
  `api_contract`)
- `npm --prefix frontend run typecheck` and `npm --prefix frontend run build`
- `python3 -m unittest python.tests.test_wrapper`
- Manual: launching against a multi-thousand-file directory opens the UI in
  under a second with a visible progress indicator; the VS Code extension
  launch no longer approaches its 20 s startup timeout.

## Phase 5: CLI Scan-Time Filters

Goal: let users exclude non-matching files at discovery time so huge archives
never reach the registry, caches, or the wire. This is the scalability
complement to Phase 3's UI filter.

Implementation steps:

1. Add a repeatable `--filter <FIELD>=<VALUE>` flag to the clap CLI.
   Supported fields: `patient_id`, `patient_name`, `study_description`,
   `study_date`, `study_uid`, `series_description`, `series_number`,
   `series_uid`, `modality`. Matching is case-insensitive substring;
   multiple `--filter` flags AND together. Reject unknown fields at parse
   time with a clear error listing valid fields.
2. Apply filters in the loader after `build_entry` succeeds and before
   registry insertion. Filtered-out files count toward a distinct
   `filtered` counter (not `skipped`) reported in the load summary.
3. Surface active filters in the startup summary line so a user who filtered
   everything out can see why the viewer is empty. An all-filtered scan is
   treated like the zero-files case (exit nonzero, message names the active
   filters).
4. Mirror the flag in the documented CLI surface: README usage section,
   Python wrapper passthrough (`python/dcmview_py/wrapper.py` argument
   plumbing), and the VS Code extension's argument interception list if it
   enumerates flags.
5. Add loader integration tests: filter matches subset, filter matches
   nothing, multiple filters AND, unknown field rejected.

Acceptance checks:

- `cargo test --test integration loader_discovery`
- `python3 -m unittest python.tests.test_wrapper`
- Manual: `dcmview --filter modality=MR --filter patient_id=1234 <dir>`
  loads only the matching subset.

## Suggested Execution Order

1. **Phase 1** (loader) — largest win, smallest diff, fully covered by
   existing tests plus one new fixture.
2. **Phase 2** (tree scalability) — frontend-only, independent of Phase 1.
3. **Phase 3** (filter UI) — builds on Phase 2's tree code paths.
4. **Phase 4** (progressive startup) — the structural change; lands after
   the loader is fast so background scans are short in the common case.
5. **Phase 5** (CLI filters) — small once the loader and registry shapes
   are settled.

Phases 1-3 together resolve the practical hang for most directory sizes;
Phase 4 removes the launcher-timeout failure mode categorically.

## Resolved Decisions

- No persistent metadata index; rescanning a large directory on each launch
  is acceptable once scans are header-only and progressive.
- No `/api/files` pagination or server-side query filtering; the client owns
  hierarchy grouping and filtering.
- File indices are append-order at insertion time and immutable for the
  process lifetime; the global path sort is removed in Phase 4.
- Browser open is deferred until the first file is registered so the
  zero-files error contract is preserved.
- UI filtering operates on the flat file list before tree derivation, and
  active filters always display matched/total counts — no silent narrowing.
