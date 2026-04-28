# Repository Guidelines

## Project Overview

`dcmview` is an ephemeral, CLI-invoked DICOM inspection tool for developers and data scientists. It starts a temporary local web server exposing an interactive browser-based viewer, then exits cleanly when the user is done. The primary use case is **multi-frame DICOM inspection** (DBT, cine MR) — workloads that are impractical in a Jupyter notebook due to Python interpreter overhead. Single-frame inspection is a convenience.

**Status:** Core implementation complete.

**Design axioms** (non-negotiable):
- **Ephemeral** — no persistent state, no config files, no database
- **Fast** — server start and first-frame render are the primary performance targets; the Rust stack exists specifically for this
- **Single binary** — `cargo install dcmview` produces one self-contained executable with the frontend baked in

---

---

## Git Commit Policy

Every completed task **MUST** be tracked in a descriptive, granular git commit. This requirement is **absolutely critical** and must be followed under all circumstances — no exceptions.

**Rules:**
- Commit after every distinct logical unit of work, not at the end of a session.
- Each commit covers exactly one coherent change (one module, one component, one test suite, one docs section). Do not batch unrelated changes into a single commit.
- Commit messages must be informative: use `type(scope): subject` format, include a blank line, then a body describing *what* changed and *why*.
  - Types: `feat`, `fix`, `test`, `docs`, `refactor`, `chore`
  - Scope: the module, file, or subsystem affected (e.g. `backend`, `frontend`, `pixels`, `server`, `types`, `tests`)
  - Subject: imperative mood, ≤72 characters
  - Body: explain the design decision, the invariant being established, or the behaviour being changed — not a restatement of the diff
- Stage files selectively (`git add <file>`) rather than `git add -A`. Only commit files that belong to the current logical unit.
- Never amend or force-push commits that have been logged here.

**Verification:** After each task, run `git log --oneline -3` to confirm the commit was recorded before moving to the next task.

## Architecture & Data Flow

```
CLI (clap)
  └── main.rs
        ├── loader.rs  ──── walkdir + rayon ──→  Vec<FileEntry>  (parallel, spawn_blocking)
        ├── server.rs  ──── Axum + Tokio   ──→  HTTP server
        │     ├── GET /api/files           ──→  FileEntry summaries (JSON)
        │     ├── GET /api/file/:i/tags    ──→  TagNode tree (lazy, built on first request)
        │     ├── GET /api/file/:i/info    ──→  FrameInfo struct
        │     └── GET /api/file/:i/frame/:n ─→  pixels.rs → image bytes (JPEG|JP2|PNG)
        ├── pixels.rs  ──── dicom-rs collector API, LRU cache, windowing
        └── tunnel.rs  ──── ssh -L subprocess, TCP readiness polling

Frontend (Svelte 5, compiled into binary via rust-embed):
  App.svelte  →  FileTabs | ImageViewport | TagPanel | FrameSlider | StatusBar
                        └── api.ts  (typed fetch wrappers)
```

### Key data structures (`types.rs`)

```rust
// Populated at startup; immutable thereafter
pub struct FileEntry {
    pub index: usize,
    pub path: PathBuf,
    pub label: String,          // "PatientID · Modality · StudyDate" or filename
    pub has_pixels: bool,
    pub frame_count: u32,
    pub rows: u32,
    pub columns: u32,
    pub transfer_syntax_uid: String,
    pub default_window: Option<WindowPreset>,
    pub offset_table: Option<Vec<u32>>,  // BOT cached after first access
}

// Shared Axum state — Clone is cheap (Arc internals)
struct AppState {
    files: Arc<Vec<FileEntry>>,
    pixel_cache: Arc<Mutex<LruCache<FrameCacheKey, Bytes>>>,
    tunnel_info: Option<Arc<TunnelInfo>>,
    server_start: std::time::Instant,
    last_request: Arc<AtomicU64>,   // Unix ms timestamp for --timeout idle watcher
}
```

### Pixel pipeline decision tree (`pixels.rs`)

On every `/frame/:n` request, classify the file's `TransferSyntaxUID` once and cache:

| Class | TS UIDs | Action |
|---|---|---|
| JPEG passthrough | 4.50, 4.51, 4.57, 4.70 | Return raw fragment bytes; `Content-Type: image/jpeg` |
| JPEG 2000 passthrough | 4.90, 4.91 | Return raw fragment bytes; `Content-Type: image/jp2` (fall back to PNG if `Accept` lacks `image/jp2`) |
| Uncompressed | Implicit LE / Explicit LE / Explicit BE | Byte-offset slice → window → PNG |
| JPEG-LS / RLE | 4.80, 4.81, 2.5 | `dicom-pixeldata` decode; 422 if unsupported |
| Other | — | HTTP 422 `{"error": "unsupported transfer syntax: {uid}"}` |

**Critical rule:** never decode JPEG server-side on the passthrough path. The browser's native decoder handles it.

---

## Key Directories

```
dcmview/
├── src/
│   ├── main.rs       CLI entry point; clap struct; calls server::run()
│   ├── server.rs     Axum router, AppState init, graceful shutdown
│   ├── loader.rs     DICOM discovery (walkdir+rayon), metadata extraction, FileEntry construction
│   ├── pixels.rs     Pixel pipeline: collector API, windowing, PNG encode, LRU cache
│   ├── tunnel.rs     SSH subprocess management, TCP readiness polling, cleanup
│   └── types.rs      Shared types: FileEntry, TagNode, FrameInfo, FrameCacheKey, etc.
├── frontend/
│   ├── src/
│   │   ├── App.svelte          Root; shared $state stores (activeFileIndex, etc.)
│   │   ├── api.ts              Typed fetch wrappers for all backend endpoints
│   │   └── lib/
│   │       ├── FileTabs.svelte
│   │       ├── ImageViewport.svelte
│   │       ├── TagPanel.svelte
│   │       ├── FrameSlider.svelte
│   │       └── StatusBar.svelte
│   ├── dist/                   Build output consumed by rust-embed (git-ignored)
│   ├── package.json
│   ├── svelte.config.js
│   └── vite.config.ts
├── tests/
│   ├── integration/            axum-test client tests against real DICOM fixtures
│   └── fixtures/               Small representative DICOM test files
├── build.rs                    Invokes `npm ci && npm run build` in frontend/ at compile time
├── Cargo.toml
```

---

## Development Commands

```bash
# First-time frontend dependency install
npm --prefix frontend ci

# Build everything (build.rs triggers the npm build)
cargo build --release

# Development build (no optimisations; rust-embed debug-embed feature serves dist/ from disk)
cargo build

# Install binary to $CARGO_HOME/bin
cargo install --path .

# Run all tests
cargo test

# Run integration tests only
cargo test --test '*'

# Frontend dev build (outside of Cargo, for iterating on UI only)
npm --prefix frontend run build

# Frontend watch mode (pair with a debug Cargo build)
npm --prefix frontend run dev
```

**Prerequisites:**
- Rust stable ≥ 1.75
- Node.js ≥ 18 + npm (build-time only; not present in the final binary)
- `ssh` on `$PATH` (runtime, for `--tunnel` feature only)

---

## Code Conventions & Common Patterns

### Rust

**Async / blocking boundary** — this is the single most important runtime rule:
- `loader.rs` file discovery (rayon) **must** run inside `tokio::task::spawn_blocking`. Never call rayon from an async context.
- Pixel decode/encode for uncompressed frames also uses `spawn_blocking` to avoid blocking the Tokio executor.
- The `Arc<Mutex<LruCache>>` is locked only for cache lookup and insert — never during decode/encode work. Compute first, then lock-insert.

**Error handling:**
- Use `anyhow` for fallible non-API code paths (loader, pixel pipeline internals).
- Frame decode errors → HTTP 500 `{ "error": "frame decode failed: ..." }`; **server continues**.
- Unsupported transfer syntax → HTTP 422 `{ "error": "unsupported transfer syntax: {uid}" }`; **never panic**.
- Tag serialisation errors per-tag → emit `{ "type": "error", "message": "..." }` node and continue; **never abort the response**.
- Zero valid files after scan → non-zero exit, message to stderr.

**LRU cache key:** `FrameCacheKey` uses `f64::to_bits()` to make window centre/width hashable. This is intentional — floating-point equality is fine here because values come from UI or DICOM tags, not arithmetic.

**`rust-embed` in development:** use the `debug-embed` feature (or a custom `debug` feature flag) to serve `frontend/dist/` from disk instead of baking it in — avoids recompiling Rust on every frontend change.

**dicom-rs collector API pattern:**
```rust
let mut collector = obj.open_pixel_data_collector()?;
let mut offset_table = Vec::<u32>::new();
collector.read_basic_offset_table(&mut offset_table)?;
// If offset_table is empty/all-zeros, iterate fragments sequentially.
// Cache per-frame byte positions discovered during iteration.
```

**Uncompressed frame byte offset:**
```
frame_size = rows × columns × samples_per_pixel × (bits_allocated / 8)
offset = frame_index × frame_size
```

**Windowing formula:**
```rust
fn apply_window(samples: &[f64], center: f64, width: f64) -> Vec<u8> {
    let low  = center - width / 2.0;
    let high = center + width / 2.0;
    samples.iter().map(|&v| {
        ((v.clamp(low, high) - low) / (high - low) * 255.0).round() as u8
    }).collect()
}
```
Window resolution order: (1) `?wc=&ww=` query params → (2) DICOM tags 0028,1050/1051 → (3) 1st/99th percentile of current frame samples.

**`X-Cache` header:** all frame endpoints must set `X-Cache: HIT` or `X-Cache: MISS`. This is used by integration tests and is not optional.

### Svelte 5 / TypeScript frontend

- **Runes syntax only** — `$state`, `$derived`, `$effect`. No legacy `$:` reactive declarations.
- Shared global stores live in `App.svelte`: `activeFileIndex`, `currentFrame`, `windowCenter`, `windowWidth`.
- `$derived` for the frame URL string. `$effect` to trigger tag fetch on file switch.
- All backend calls go through `api.ts` typed wrappers — no raw `fetch` calls in components.
- Window/Level re-fetch is debounced at **150ms** after drag movement stops. Do not flood requests during drag.
- Zoom and pan use CSS `transform: scale/translate` — **no re-fetch** unless W/L changes.
- Zoom/pan state is **per file**, not per frame. Switching frames preserves zoom; switching files resets to identity.
- No external CSS frameworks. Scoped Svelte styles only.
- Dark theme palette: background `#1a1a1a`, surface `#242424`, text `#e0e0e0`, accent `#4a9eff`.
- Monospace (`JetBrains Mono` / `ui-monospace`) for tag values; `system-ui` for chrome.

### CLI (clap derive API)

```
dcmview [OPTIONS] <PATH> [PATH ...]
  -p, --port <u16>          default: 0 (auto-assign)
  --host <str>              default: 127.0.0.1
  --no-browser
  --tunnel
  --tunnel-host <str>
  --tunnel-port <u16>       default: 0
  --timeout <u64>           seconds; no timeout if absent
  --no-recursive
```

---

## Important Files

| File | Role |
|---|---|
| `README.md` | User-facing documentation — installation, usage, API reference |
| `src/main.rs` | CLI struct (clap derive), startup orchestration |
| `src/server.rs` | Axum router, `AppState`, startup sequence, graceful shutdown |
| `src/loader.rs` | DICOM discovery + `FileEntry` construction |
| `src/pixels.rs` | All pixel decode/encode logic; LRU cache; the hot path |
| `src/types.rs` | All shared types; the contract between modules |
| `src/tunnel.rs` | SSH subprocess lifecycle |
| `build.rs` | Frontend build integration; emits `cargo:rerun-if-changed` fingerprints |
| `frontend/src/api.ts` | All typed fetch wrappers — single source of API shape on the frontend |
| `frontend/src/App.svelte` | Root component; shared reactive stores |

---

## Runtime / Tooling

- **Runtime:** Rust stable ≥ 1.75, Tokio async runtime (`features = ["full"]`)
- **Frontend toolchain:** Vite + Svelte 5 (TypeScript); Node 18+, npm
- **Package manager (Rust):** Cargo
- **Package manager (frontend):** npm (not bun, not pnpm — `build.rs` calls `npm ci`)
- **Build integration:** `build.rs` runs `npm ci && npm run build` inside `frontend/` at Cargo build time. The Rust binary has no runtime file dependencies — `frontend/dist/` is embedded via `rust-embed`.
- **Single binary distribution:** `cargo install dcmview` is the intended install path.

### Cargo feature flags (planned)

- `debug-embed` (via `rust-embed`): serve `frontend/dist/` from disk during development instead of embedding. Enables fast frontend iteration without Rust recompilation.

---

## Testing & QA

**Framework:** `cargo test` + `axum-test` crate for integration tests.

**Test layout:**
```
tests/
├── integration/    Axum test-client tests; exercise real HTTP routes against fixture DICOMs
└── fixtures/       Small but representative DICOM files:
                      - Single-frame uncompressed (Explicit LE)
                      - Single-frame JPEG Baseline (TS 4.50)
                      - Single-frame JPEG 2000 (TS 4.91)
                      - Multi-frame JPEG Baseline (≥ 10 frames for BOT + offset tests)
                      - File without pixel data (SR or KOS)
                      - Non-DICOM file (for skip-counter test)
```

**Key integration test assertions:**
- `X-Cache: MISS` on first frame request; `X-Cache: HIT` on identical repeat
- `X-Cache: MISS` after any W/C param change
- JPEG passthrough: `Content-Type: image/jpeg`; response bytes match reference extraction (no decode)
- Uncompressed: pixel values at known coordinates match a reference tool (e.g., pydicom)
- Multi-frame BOT: second request for a frame on the same file does **not** re-read the BOT
- `has_pixels: false` in `/api/files` for SR files; `/frame/0` returns 404
- Port 0 auto-assign: printed URL port matches `listener.local_addr()`
- `--timeout 5`: server process exits ~5s after last request
- Skip counter: directory with mixed DICOM/non-DICOM files reports correct counts in load summary

**Do not mock** the DICOM layer — integration tests must run against real fixture files to catch codec path bugs.

**Performance targets** (verify with timing instrumentation, not mocks):
- Steps 1–7 of startup sequence complete in < 300ms for a small file set
- First frame (JPEG passthrough) served in < 200ms for a 100+-frame DBT file
- Memory usage stays roughly constant across sequential frame requests for large multi-frame files (collector API ensures O(one frame) allocation)
