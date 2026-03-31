# SYSTEM_SPEC: `dcmview` — Ephemeral DICOM Viewer for Developers & Data Scientists

**Version:** 1.0  
**Status:** Final Draft — Ready for Implementation  
**Target Audience:** Agentic coding tool (OMP/Oh-My-Pi)  
**Stack:** Rust (Axum) backend · Svelte 5 frontend · `dicom-rs` DICOM engine

---

## 1. Project Overview

`dcmview` is a lightweight, ephemeral DICOM inspection tool designed for developers and data scientists working with DICOM objects. It spins up a temporary local web server exposing an interactive browser-based viewer, then exits cleanly when the user is done. The tool is invoked primarily via CLI; an optional thin Python wrapper is provided as a convenience shim for notebook / script workflows.

It is built for **speed above all else** — particularly for multi-frame files (DBT, cine MR, etc.) that cannot be practically explored in a Jupyter notebook. The Rust + `dicom-rs` stack is chosen specifically to eliminate the interpreter overhead and full-array pixel decoding that make Python-based tools sluggish for these cases. Single-frame inspection is a convenience feature; multi-frame inspection is the core value proposition.

### Core Design Principles

- **Ephemeral** — no persistent state, no database, no config files written to disk
- **Fast** — server start time and first-frame render latency are the top priorities; the Rust stack is chosen specifically for this
- **Clean interface** — minimal flags, sensible defaults, no boilerplate
- **Remote-friendly** — first-class SSH tunnel / port-forward support built in
- **Developer-centric** — raw DICOM tag tree alongside pixel data; no clinical UX cruft

---

## 2. Technology Stack

| Layer | Choice | Rationale |
|---|---|---|
| Language | **Rust (2021 edition)** | Zero-cost pixel pipeline, true parallelism (no GIL), sub-millisecond server startup, single self-contained binary |
| DICOM loading | **`dicom-object` 0.9** | Collector API (`open_pixel_data_collector`) enables per-fragment streaming — memory cost is O(one frame), not O(all frames); consistently faster than pydicom in benchmarks |
| Web server | **Axum** (Tokio runtime) | Async-native, streaming response bodies, Tower middleware, clean integration with `rust-embed` |
| Pixel encoding | **`image` crate** | PNG encode for uncompressed/decoded frames — JPEG passthrough bypasses this entirely |
| Frontend | **Svelte 5 + TypeScript** | Compiles to plain JS/CSS — no runtime framework shipped to browser; tiny bundle; runes syntax maps naturally to frame/W-L reactive state |
| Asset embedding | **`rust-embed`** | Bakes compiled Svelte `dist/` into the binary at compile time — zero runtime file I/O, single binary distribution |
| CLI | **`clap` (derive API)** | Standard Rust CLI, auto-generates `--help` |
| SSH tunnel | **`std::process::Command`** (shell out to `ssh -L`) | Reliable, no extra dependencies, leverages user's existing SSH config and keys |
| Serialisation | **`serde` + `serde_json`** | Tag tree → JSON for API responses |
| LRU cache | **`lru` crate** | Frame image cache keyed on `(file_index, frame, window_center, window_width)` |
| File discovery | **`walkdir` + `rayon`** | Parallel recursive directory scan — 200 files should register in well under a second |

### Python wrapper (optional convenience shim — v1.0 stretch goal)

A thin Python package (`dcmview-py`) may be provided that locates the installed `dcmview` binary on `$PATH` and invokes it via `subprocess`. All logic lives in the Rust binary. See §4 for the interface. This is a stretch goal; the CLI is the primary interface.

---

## 3. CLI Interface (Primary API)

### 3.1 Command

```
dcmview [OPTIONS] <PATH> [PATH ...]
```

### 3.2 Options

| Flag | Type | Default | Description |
|---|---|---|---|
| `<PATH>` | path(s) | required | One or more DICOM file paths, or directories |
| `--port`, `-p` | u16 | `0` | Port to bind (`0` = auto-assign a free port) |
| `--host` | str | `127.0.0.1` | Bind address (`0.0.0.0` for all interfaces) |
| `--no-browser` | flag | — | Don't attempt to auto-open a browser tab |
| `--tunnel` | flag | — | Establish SSH reverse tunnel for remote access |
| `--tunnel-host` | str | — | SSH host string, e.g. `user@myserver.com` |
| `--tunnel-port` | u16 | `0` | Local port to expose on the tunnel host (`0` = same as bind port) |
| `--timeout` | u64 | — | Auto-shutdown after N seconds of no browser requests |
| `--no-recursive` | flag | — | When given a directory, scan top level only (default is recursive) |

### 3.3 CLI Examples

```bash
# Single file — server starts, browser opens
dcmview scan.dcm

# Multiple files, fixed port
dcmview -p 8888 frame1.dcm frame2.dcm

# Directory (recursive by default)
dcmview ./study_dir/

# Remote server: establish SSH tunnel, accessible at localhost:8888 on local machine
dcmview --tunnel --tunnel-host user@myserver.com --tunnel-port 8888 scan.dcm

# Non-interactive: don't open browser, shut down after 5 min idle
dcmview --no-browser --timeout 300 ./study_dir/
```

---

## 4. Python API (Convenience Wrapper — v1.0 Stretch Goal)

The Rust binary is the sole source of logic. The Python package (`dcmview-py`) is a thin shim that locates the `dcmview` binary on `$PATH` and invokes it via `subprocess`. It exists solely to support notebook and script workflows where users are already in a Python context.

```python
def view(
    files,                           # str | Path | pydicom.Dataset | list[str | Path | Dataset]
    port: int = 0,
    host: str = "127.0.0.1",
    browser: bool = True,
    tunnel: bool = False,
    tunnel_host: str | None = None,
    tunnel_port: int = 0,
    block: bool = True,              # ALWAYS default True — never auto-detect Jupyter
    recursive: bool = True,
    timeout: int | None = None,
) -> "ShutdownHandle | None":
    """Thin shim: assembles CLI args and invokes the dcmview binary via subprocess."""
```

### Key behaviours

- **`block=True` is the unconditional default.** Do not auto-detect IPython/Jupyter and change behaviour silently. The user must pass `block=False` explicitly — this reinforces the ephemeral intent.
- When `block=False`: launch subprocess detached; return a `ShutdownHandle` with `.stop()` (sends SIGINT) and `.url` (string). Print: `dcmview: running in background — call handle.stop() or send SIGINT to terminate`
- **`pydicom.Dataset` support:** the binary has no Python runtime, so in-memory datasets must be serialised first. When a `Dataset` is detected, write it to a `tempfile.NamedTemporaryFile` (`.dcm`) using `pydicom.dcmwrite`, pass the temp path to the binary, and delete the temp file on shutdown. This is transparent to the caller.
- If the `dcmview` binary is not found on `$PATH`, raise `RuntimeError` with: `dcmview binary not found — install with: cargo install dcmview`

---

## 5. Project Structure

```
dcmview/
├── Cargo.toml
├── Cargo.lock
├── build.rs                  # Runs `npm ci && npm run build` in frontend/ at compile time
├── src/
│   ├── main.rs               # CLI entry point (clap), calls server::run()
│   ├── server.rs             # Axum router, state initialisation, graceful shutdown
│   ├── loader.rs             # DICOM discovery, metadata parsing, file registry
│   ├── pixels.rs             # Pixel pipeline: collector API, windowing, encode, LRU cache
│   ├── tunnel.rs             # SSH tunnel management
│   └── types.rs              # Shared types: FileEntry, TagNode, FrameInfo, etc.
├── frontend/
│   ├── package.json
│   ├── svelte.config.js
│   ├── vite.config.ts
│   ├── src/
│   │   ├── App.svelte        # Root component, shared state stores
│   │   ├── lib/
│   │   │   ├── FileTabs.svelte
│   │   │   ├── ImageViewport.svelte
│   │   │   ├── TagPanel.svelte
│   │   │   ├── FrameSlider.svelte
│   │   │   └── StatusBar.svelte
│   │   └── api.ts            # Typed fetch wrappers for all backend endpoints
│   └── dist/                 # Built output — consumed by rust-embed in build.rs
└── tests/
    ├── integration/          # Axum test-client tests against real DICOM fixtures
    └── fixtures/             # Small representative DICOM test files
```

### Build pipeline

`build.rs` must:
1. Check that `node` and `npm` are available on `$PATH`; emit a clear `cargo:error=Node.js and npm are required to build dcmview` if not
2. Emit `cargo:rerun-if-changed=frontend/src` and `cargo:rerun-if-changed=frontend/package.json` so Cargo only re-runs the npm build when frontend source changes
3. Run `npm ci` then `npm run build` inside `frontend/`
4. The `rust-embed` derive macro picks up `frontend/dist/` at compile time

The resulting binary is fully self-contained — no external files needed at runtime. `cargo install dcmview` produces a single executable.

---

## 6. Web Server (`server.rs`)

### 6.1 Application State

```rust
#[derive(Clone)]
struct AppState {
    files: Arc<Vec<FileEntry>>,
    pixel_cache: Arc<Mutex<LruCache<FrameCacheKey, Bytes>>>,
    tunnel_info: Option<Arc<TunnelInfo>>,
    server_start: std::time::Instant,
    last_request: Arc<AtomicU64>,         // Unix timestamp ms, for --timeout
}

pub struct FileEntry {
    pub index: usize,
    pub path: PathBuf,
    pub label: String,            // "PatientID · Modality · StudyDate" or filename
    pub has_pixels: bool,
    pub frame_count: u32,         // 1 for single-frame
    pub rows: u32,
    pub columns: u32,
    pub transfer_syntax_uid: String,
    pub default_window: Option<WindowPreset>,
    pub offset_table: Option<Vec<u32>>,   // cached BOT for encapsulated pixel data
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct FrameCacheKey {
    file_index: usize,
    frame: u32,
    window_center_bits: u64,    // f64::to_bits() for hashability
    window_width_bits: u64,
}
```

### 6.2 Router & Endpoints

```
GET  /                              → serve index.html (rust-embed)
GET  /assets/*path                  → serve Svelte build assets (rust-embed)
GET  /api/files                     → JSON array of FileEntry summaries
GET  /api/file/:index/tags          → full tag tree as JSON (lazy, built on first request)
GET  /api/file/:index/frame/:n      → image bytes; `?wc=<f64>&ww=<f64>` for explicit window override; `?mode=full_dynamic` for full dynamic range; `?mode=default` (default when absent)
GET  /api/file/:index/info          → FrameInfo { frame_count, rows, columns, transfer_syntax, has_pixels, default_window }
```

Response for `/api/files` includes a top-level `"tunnelled"` field and `"server_start_ms"` for the status bar.

Add response header `X-Cache: HIT|MISS` on frame endpoints for cache observability during testing.

### 6.3 Startup Sequence

1. Parse CLI args (clap)
2. Discover and register DICOM files (§8) — build `Vec<FileEntry>`; print load summary to stdout
3. Bind TCP listener on `host:port`; if port is 0, read back assigned port with `listener.local_addr()`
4. Print startup messages (§10)
5. If `--tunnel`: spawn SSH subprocess (§9); wait for readiness before printing "tunnel active" line
6. If browser open enabled: call `open::that(url)` using the `open` crate
7. Start Axum with graceful shutdown wired to `SIGINT`/`SIGTERM` via `tokio::signal`

**Target:** steps 1–7 complete and first request serviceable in **< 300 ms** on a modern machine for a small file set. For large directory scans, the server should bind and print its URL before file discovery completes (stream results in asynchronously), though this is a v1.1 enhancement if needed.

### 6.4 Graceful Shutdown

- Listen for `SIGINT` (Ctrl+C) and `SIGTERM` using `tokio::signal::ctrl_c()` and `unix::signal()`
- On signal: stop accepting new connections; allow in-flight requests up to 5s; kill SSH subprocess; print `dcmview: shutting down...`; exit 0
- If `--timeout N`: background Tokio task monitors `last_request` timestamp; fires shutdown after N seconds of inactivity

---

## 7. Pixel Pipeline (`pixels.rs`)

This is the most performance-critical module. Design around two fast paths and a general fallback.

### 7.1 Transfer Syntax Classification

On first frame request for a file, classify `TransferSyntaxUID` into one of:

| Class | TS UIDs | Action |
|---|---|---|
| **JPEG passthrough** | 1.2.840.10008.1.2.4.50 (JPEG Baseline), .51 (JPEG Extended), .57 (JPEG Lossless Non-Hierarchical), .70 (JPEG Lossless SV1) | Return raw fragment bytes; `Content-Type: image/jpeg` |
| **JPEG 2000 passthrough** | 1.2.840.10008.1.2.4.90 (J2K Lossless), .91 (J2K Lossy) | Return raw fragment bytes; `Content-Type: image/jp2`. Detect browser J2K support via `Accept` header — if `image/jp2` absent, fall back to server-side decode → PNG using `dicom-pixeldata` or the `jpeg2000` crate |
| **Uncompressed** | 1.2.840.10008.1.2 (Implicit LE), 1.2.840.10008.1.2.1 (Explicit LE), 1.2.840.10008.1.2.2 (Explicit BE) | Decode samples → apply windowing → encode PNG |
| **JPEG-LS** | 1.2.840.10008.1.2.4.80, .81 | Attempt decode via `dicom-pixeldata`; return error frame with TS UID if unsupported |
| **RLE Lossless** | 1.2.840.10008.1.2.5 | Attempt decode via `dicom-pixeldata`; return error frame if unsupported |
| **Other / unknown** | Any UID not listed above | Return HTTP 422 with `{ "error": "unsupported transfer syntax: {uid}" }` |

> **Note on JPEG passthrough coverage:** The majority of real-world mammography and DBT files use JPEG Baseline (4.50) or JPEG 2000 (4.90/.91). Prioritise and thoroughly test these paths. JPEG-LS is common in newer modalities (digital radiography, some CR). The passthrough paths require zero server-side decode and are the primary performance win of the Rust stack.

### 7.2 JPEG Passthrough Fast Path (Highest Priority)

For JPEG-compressed files, the frame endpoint must return raw compressed bytes without any server-side decode:

```rust
// Collector-based per-frame access
let mut collector = obj.open_pixel_data_collector()?;
let mut offset_table = Vec::<u32>::new();
collector.read_basic_offset_table(&mut offset_table)?;

// If offset table is non-empty, seek directly to fragment N.
// If absent (all zeros or empty), iterate through fragments to reach frame N.
let mut buf = Vec::new();
for i in 0..=frame_index {
    buf.clear();
    let _len = collector.read_next_fragment(&mut buf)?
        .ok_or(DicomViewError::FrameNotFound(frame_index))?;
}
// buf now contains the raw JPEG bytes for frame_index — return as-is
```

Cache the `offset_table` in `FileEntry` after first access to avoid re-reading the BOT on every request.

**Never decode the JPEG server-side in this path.** The browser's native decoder handles all decompression.

### 7.3 Uncompressed Frame Extraction

For uncompressed multi-frame data, frame N occupies a contiguous region of the pixel data element. Compute the byte offset:

```
frame_size_bytes = rows × columns × samples_per_pixel × (bits_allocated / 8)
offset = N × frame_size_bytes
```

Read exactly `frame_size_bytes` bytes starting at `offset`. Do **not** read or decode the entire pixel data element.

Steps after reading raw bytes:
1. Reinterpret as `u16` (or appropriate type) per `BitsAllocated`, `BitsStored`, `HighBit`, `PixelRepresentation`
2. Apply `RescaleSlope` / `RescaleIntercept` if present (cast to `f64` working type)
3. Apply windowing (§7.4) → `u8` output
4. Encode as PNG with the `image` crate (`ImageBuffer::from_vec`)
5. Return with `Content-Type: image/png`

### 7.4 Windowing

```rust
fn apply_window(samples: &[f64], center: f64, width: f64) -> Vec<u8> {
    let low  = center - width / 2.0;
    let high = center + width / 2.0;
    samples.iter().map(|&v| {
        ((v.clamp(low, high) - low) / (high - low) * 255.0).round() as u8
    }).collect()
}
```

**Window mode** (`?mode=` query param, `WindowMode` enum in `types.rs`):
- `Default` (absent or `?mode=default`): use the resolution order below
- `FullDynamic` (`?mode=full_dynamic`): ignore explicit params and DICOM tags; compute window from true min/max of current frame samples (no clipping)

Window centre/width resolution order for `Default` mode (first match wins):
1. Query parameters `?wc=` and `?ww=` (user override from UI drag or preset)
2. DICOM tags `WindowCenter` (0028,1050) + `WindowWidth` (0028,1051) — first value if multi-valued
3. Fallback: 1st/99th percentile of the current frame's sample values (compute inline, no pre-scan)

### 7.5 LRU Frame Cache

- Capacity: 128 entries
- Key: `FrameCacheKey` (file_index, frame, wc bits, ww bits, window_mode)
- Value: `Bytes` — the fully encoded response body, ready to serve
- For JPEG passthrough frames, cache the raw fragment bytes (they are the response body)
- Lock (`Arc<Mutex<...>>`) held only during cache lookup and insert — not during decode/encode
- Add `X-Cache: HIT` / `X-Cache: MISS` response header for observability

---

## 8. DICOM Discovery & Loading (`loader.rs`)

### 8.1 File Discovery

Given one or more CLI paths:
- **File path**: add directly to candidate list
- **Directory path**: walk with `walkdir`; recursive by default, top-level only if `--no-recursive`
- For each candidate: attempt `dicom_object::open_file()`; on success add to registry; on failure increment skip counter
- DICOM detection: rely on `dicom-object`'s own validation (preamble magic bytes `DICM` at offset 128, or implicit-VR fallback detection). Do **not** filter by file extension — real-world DICOM files frequently have no extension.

Parallelise candidate processing with `rayon::par_iter()`. For 200 files this should complete in well under a second.

### 8.2 Metadata Extraction Per File

Open each file and extract only the tags needed for `FileEntry`. Read with pixel data deferred — use `dicom_object::open_file()` and do not access `PixelData` yet:

- `(0002,0010)` TransferSyntaxUID
- `(0008,0016)` SOPClassUID
- `(0008,0060)` Modality
- `(0008,0020)` StudyDate
- `(0010,0020)` PatientID
- `(0028,0008)` NumberOfFrames (default 1 if absent)
- `(0028,0010)` Rows
- `(0028,0011)` Columns
- `(0028,1050)` WindowCenter
- `(0028,1051)` WindowWidth
- `(7FE0,0010)` PixelData — check presence only (`element_by_name("PixelData").is_ok()`)

### 8.3 Tag Tree Serialisation

Built lazily on first `/api/file/:index/tags` request (not at startup). Serialised to JSON as an array of `TagNode`:

```typescript
// TypeScript mirror of the Rust TagNode type
interface TagNode {
  tag: string;           // "(0028,0010)"
  vr: string;            // "US"
  keyword: string;       // "Rows"  — from DICOM data dictionary
  value: TagValue;
}

type TagValue =
  | { type: "string";   value: string }
  | { type: "number";   value: number }
  | { type: "numbers";  value: number[] }
  | { type: "binary";   length: number }           // OB, OW, OD, OF, UN — never include raw bytes
  | { type: "sequence"; items: TagNode[][] }        // SQ — array of items, each item is TagNode[]
  | { type: "error";    message: string }           // serialisation fallback — never crash
```

Rules:
- `PixelData` (7FE0,0010) and any OB/OW element > 256 bytes → always `{ type: "binary", length: N }`
- `PersonName` VR → formatted string
- Multi-value elements → `numbers` for numeric VRs, semicolon-joined string for string VRs
- Sequences → recurse; emit `TagNode[][]` (array of items)
- Any individual tag that fails → emit `{ type: "error", message: "..." }` and continue; never abort the response
- Tags in ascending (group, element) order

---

## 9. SSH Tunnel (`tunnel.rs`)

### 9.1 Mechanism

When `--tunnel` is passed, spawn an `ssh` subprocess:

```bash
ssh -N \
    -o ExitOnForwardFailure=yes \
    -o ServerAliveInterval=10 \
    -L {tunnel_port}:127.0.0.1:{bind_port} \
    {tunnel_host}
```

- `-N` — no remote command, tunnel only
- `-o ExitOnForwardFailure=yes` — fail fast if remote port binding fails
- `-o ServerAliveInterval=10` — detect dropped connections
- Pipe `stderr` to a background thread that logs warnings

### 9.2 Readiness Detection

After spawning, poll `TcpStream::connect("127.0.0.1:{tunnel_port}")` every 100ms for up to 5 seconds. First successful connection → tunnel is ready → print "SSH tunnel active" line. If 5s elapsed without success → print error and fall back to manual instructions.

### 9.3 Cleanup

Store `Child` handle in a `Mutex`. On shutdown: `child.kill()` + `child.wait()`.

### 9.4 Fallback

If `ssh` is not found on `$PATH`:
```
dcmview: warning — ssh not found on PATH, cannot establish tunnel
dcmview: to forward manually, run on your local machine:
dcmview:   ssh -L {port}:localhost:{port} {tunnel_host}
```

---

## 10. Startup Output

### 10.1 Load Summary (printed before server binds)

```
dcmview: loaded 47 DICOM file(s) from ./study_dir/ (searched recursively)
dcmview: loaded 44 DICOM file(s) from ./study_dir/ (3 skipped — not valid DICOM, searched recursively)
dcmview: loaded 1 DICOM file
dcmview: loaded 3 DICOM file(s)
```

### 10.2 Server Ready (printed after bind, after tunnel readiness if applicable)

With tunnel:
```
dcmview: server running at http://127.0.0.1:8432
dcmview: SSH tunnel active — access at http://localhost:8432 on your local machine
dcmview: press Ctrl+C to stop
```

Without tunnel:
```
dcmview: server running at http://127.0.0.1:8432
dcmview: (on a remote server? run on your local machine: ssh -L 8432:localhost:8432 user@host)
dcmview: press Ctrl+C to stop
```

### 10.3 Shutdown

```
dcmview: shutting down...
```

---

## 11. Frontend (Svelte 5 + TypeScript)

### 11.1 Build Configuration

- **Vite** as the build tool (standard Svelte 5 setup)
- Output to `frontend/dist/`; consumed by `rust-embed` at Rust compile time
- `build.rs` runs `npm ci && npm run build` — plain `cargo build --release` is sufficient after initial setup
- Use **Svelte 5 runes syntax** (`$state`, `$derived`, `$effect`) for all reactive state
- No external CSS frameworks — scoped Svelte styles only

### 11.2 Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│  dcmview  │ File 1 (MG) │ File 2 (DBT) │ File 3 (MR) │             │  ← FileTabs
├────────────────────────────────┬─────────────────────────────────────┤
│                                │  DICOM Tags                         │
│                                │  ─────────────────────────────────  │
│        ImageViewport           │  🔍  [filter tags...          ]     │
│                                │                                      │
│                                │  (0008,0060)  Modality    MG        │
│   [◀]  frame 4 / 60  [▶]  ▶   │  (0028,0010)  Rows        3000     │
│   W: [  4000 ]  C: [  200 ]   │  (0028,0011)  Columns     2500     │
│                                │  ▶ (0028,9110)  PixelMeasuresSq    │
│                                │  (7FE0,0010)  PixelData  [OW · …]  │
├────────────────────────────────┴─────────────────────────────────────┤
│  http://127.0.0.1:8432  ·  3 files loaded  ·  uptime 00:04:12       │  ← StatusBar
└──────────────────────────────────────────────────────────────────────┘
```

### 11.3 ImageViewport Component

**Image display:**
- Fetch frame from `/api/file/{index}/frame/{n}` with optional `?wc=&ww=&mode=` params
- Display in an `<img>` tag (not `<canvas>`) — native browser JPEG/PNG decode is fast and handles colour profiles correctly
- Show a spinner overlay while fetching; show `No pixel data` placeholder if `has_pixels = false`
- Overlay displays frame counter and current W/C values (or `W/L N/A` for JPEG passthrough files where windowing is not applied server-side)

**Viewer toolbar (ViewerToolbar component):**
- Tool selector: **WL** (Window/Level), **Pan**, **Zoom**, **Scroll** — determines left-button drag behavior
- W/L preset dropdown: Default | Full Dynamic | CT Abdomen | CT Angio | CT Bone | CT Brain | CT Chest | CT Lung
- Reset button: resets zoom/pan to identity and W/L to DICOM default
- Keyboard shortcuts: `W` / `P` / `Z` / `S` to switch active tool

**Mouse interactions:**
- **Left drag**: routes by active tool:
  - `WL` tool — horizontal: adjust window width; vertical: adjust window centre; 150ms debounced re-fetch
  - `Pan` tool — `transform: translate(...)` on `<img>` — instant, no request
  - `Zoom` tool — drag up = zoom in, drag down = zoom out; pivot at initial click point; instant, no request
  - `Scroll` tool — drag down = advance frame, drag up = retreat frame; 10px per frame step
- **Right drag**: always zoom (hard-coded, ignores active tool)
- **Middle drag**: always pan (hard-coded, ignores active tool)
- **Wheel (multi-frame)**: scrub through frames by default; discrete wheel = 1 frame/event; trackpad pixel-mode = accumulate at 30px/frame threshold
- **Ctrl/Cmd + wheel**: always zoom (regardless of frame count or active tool); trackpad pinch gesture also routes here
- **Wheel (single-frame)**: discrete wheel = zoom; trackpad two-finger scroll = pan
- **Reset**: double-click — restore default W/C, reset zoom/pan to identity
- Zoom and pan state is **per file** (not per frame) — switching frames within a file preserves zoom; switching files resets

### 11.4 FrameSlider Component

Rendered only when `frame_count > 1`.

- **◄ / ► buttons** — previous/next frame
- **Frame counter** — `frame N / total` (1-indexed display, 0-indexed internally)
- **► / ⏸ play-pause** — cine playback using `setInterval`
- **FPS selector** — selectable speed: 1 / 5 / 10 / 15 / 24 fps (default: 10 fps)
- **Loop / Sweep toggle** — loop: wraps to frame 0 at end; sweep: reverses direction at both boundaries (bounce playback)
- **Keyboard** — `←` / `→` arrow keys (or `[` / `]`) for frame navigation; `Space` for play/pause
- **Prefetch** — on navigation to frame N, fire background `fetch()` calls for frames N+1 and N+2; browser HTTP cache and Rust LRU cache serve repeat requests instantly

### 11.5 TagPanel Component

- Scrollable, filterable table: columns **Tag**, **Keyword**, **VR**, **Value**
- **Filter input**: live client-side filter across all columns simultaneously, case-insensitive; no debounce needed (filtering is over already-fetched JSON)
- **SQ rows**: rendered with a `▶` chevron; click to expand inline as nested rows with indent
- **Binary values**: `[OW · 1,234,567 bytes]` in a muted/dimmed style
- **Long string values** (> 80 chars): truncated with `…` expand-on-click
- **Click to copy**: clicking any row copies `(GGGG,EEEE)  Keyword  =  value` to clipboard; show a brief `Copied ✓` tooltip that fades after 1.5s
- Tags in ascending (group, element) order
- Tag data fetched once per file on first tab activation; cached in Svelte store — not re-fetched on frame changes

### 11.6 FileTabs Component

- One tab per file; label: `PatientID · Modality · StudyDate` if available, otherwise filename
- Active tab highlighted; clicking updates shared `activeFileIndex` store and resets frame to 0
- Overflow: horizontal scroll when tabs exceed header width (no wrapping, no dropdown)

### 11.7 StatusBar Component

- Left: bind URL
- Centre: `N files loaded`
- Right: live uptime counter (compute from `server_start_ms` field in `/api/files` response)
- If tunnelled (from `/api/files` response `tunnelled` field): append `· tunnelled from {host}`

### 11.8 Styling

- Dark theme: background `#1a1a1a`, surface `#242424`, text `#e0e0e0`
- Monospace font for tag values (`JetBrains Mono` or system `ui-monospace`)
- Sans-serif for UI chrome (`system-ui`)
- Single accent colour: muted blue `#4a9eff` for active states, focus rings, links
- No external CSS frameworks
- Target minimum viewport: 1280px wide; no mobile breakpoints required

---

## 12. Error Handling & Edge Cases

| Scenario | Behaviour |
|---|---|
| File not found | Log warning to stderr, skip; fatal exit if zero files loaded |
| Not a valid DICOM file | Log warning with path, skip; increment skip counter for load summary |
| No pixel data (SR, KOS, PR, etc.) | `has_pixels: false`; image viewport shows `No pixel data` placeholder |
| Unsupported compressed TS | Image viewport shows `Unsupported transfer syntax: {uid}` |
| JPEG 2000 browser support absent | Detect via `Accept` header; fall back to server-side decode → PNG |
| Port in use (explicit `--port N`) | Fatal: `dcmview: port N is already in use — try --port 0 for auto-assign` |
| `--port 0` auto-assign | Bind to port 0, read assigned port with `listener.local_addr()` |
| Large multi-frame (300-frame DBT) | Collector API reads one fragment at a time; memory ∝ one frame, not all frames |
| Corrupt frame data | Return HTTP 500 `{ "error": "frame decode failed: ..." }`; server continues |
| SSH subprocess dies mid-session | Log warning; server continues; status bar shows `tunnel lost` |
| Zero valid files after scan | Exit with non-zero status: `dcmview: no valid DICOM files found` |
| `build.rs` — node/npm not found | Emit `cargo:error=Node.js and npm are required to build dcmview` |

---

## 13. Cargo Dependencies

```toml
[dependencies]
# DICOM
dicom-object          = "0.9"
dicom-core            = "0.9"
dicom-dictionary-std  = "0.9"
dicom-pixeldata       = "0.9"    # decode fallback for JPEG-LS, RLE, J2K when passthrough unavailable

# Web server
axum                  = { version = "0.7", features = ["macros"] }
tokio                 = { version = "1",   features = ["full"] }
tower                 = "0.4"
tower-http            = { version = "0.5", features = ["cors", "trace"] }

# Asset embedding
rust-embed            = { version = "8",   features = ["axum"] }

# Serialisation
serde                 = { version = "1",   features = ["derive"] }
serde_json            = "1"

# Image encoding (PNG fallback path)
image                 = "0.25"

# CLI
clap                  = { version = "4",   features = ["derive"] }

# Cache
lru                   = "0.12"

# Parallel file discovery
rayon                 = "1"
walkdir               = "2"

# Browser open
open                  = "5"

# Utilities
bytes                 = "1"
tracing               = "0.1"
tracing-subscriber    = { version = "0.3", features = ["env-filter"] }
anyhow                = "1"

[dev-dependencies]
tempfile              = "3"
axum-test             = "14"   # integration test helpers
```

> **Dependency note:** `dicom-pixeldata` pulls in `dicom-object` transitively — ensure version pins are consistent across all `dicom-*` crates (all `0.9`). If `dicom-pixeldata` does not yet support a given TS in `0.9`, return the HTTP 422 error response rather than panicking.

---

## 14. Build & Installation

### 14.1 Prerequisites

- Rust stable toolchain (1.75+)
- Node.js 18+ and npm (build-time only — not present in the final binary)

### 14.2 Build

```bash
# First time: install Node deps explicitly (build.rs will also do this, but explicit is clearer)
npm --prefix frontend ci

# Compile everything — build.rs handles the npm build step
cargo build --release
```

### 14.3 Install from Source

```bash
cargo install --path .
```

### 14.4 Distribution

The binary is fully self-contained — no companion files, no runtime dependencies beyond `ssh` (for the tunnel feature). Distribute via:
- `cargo install dcmview` (crates.io)
- Pre-built binaries on GitHub Releases (Linux x86_64, Linux aarch64, macOS aarch64)

---

## 15. Non-Goals (v1.0)

- In-process `pydicom.Dataset` passthrough (binary has no Python runtime; use the temp-file serialisation path in the Python shim)
- DICOM networking (C-STORE, C-FIND, DICOM web / WADO-RS)
- Clinical-grade display (GSDF calibration, presentation states, ICC profiles)
- DICOM RT, SEG, SR structured report rendering beyond raw tag display
- Editing or writing DICOM files
- Authentication or multi-user access
- Windows SSH tunnel support (`ssh -L` assumes a POSIX `ssh` binary; Windows users should use WSL or forward manually)
- Mobile / small-screen layout (minimum viewport 1280px)
- WASM frontend build (Leptos/trunk) — out of scope; Svelte is the chosen frontend stack

---

## 16. Testing Checklist (for OMP to verify)

### Server & Pixel Pipeline
- [ ] Single uncompressed file — server starts in < 300ms; PNG returned with correct dimensions
- [ ] Single JPEG Baseline (TS 4.50) file — raw JPEG bytes returned; `Content-Type: image/jpeg`; no server-side decode (verify via `X-Cache: MISS` on first request and negligible CPU)
- [ ] Single JPEG 2000 (TS 4.91) file — raw JP2 bytes returned when browser supports `image/jp2`; falls back to PNG otherwise
- [ ] JPEG-LS file — `dicom-pixeldata` decode path exercised; PNG returned or clean 422 if unsupported
- [ ] Multi-frame DBT (JPEG Baseline, 100+ frames) — frame 0 served in < 200ms; frame 50 accessible without loading frames 0–49; RSS stays roughly constant across sequential frame requests
- [ ] Collector API offset table — BOT read once and cached in `FileEntry`; second frame request for same file does not re-read BOT
- [ ] `X-Cache: HIT` on second identical frame request (same file, frame, W/C)
- [ ] `X-Cache: MISS` on first request, and after W/C params change
- [ ] Window override `?wc=200&ww=400` — PNG output reflects narrowed window vs default
- [ ] Uncompressed multi-frame — per-frame byte offset calculation correct; pixel values at known coordinates match reference values
- [ ] Big-endian uncompressed (Explicit BE TS) — byte-swap applied before windowing
- [ ] File with no pixel data (SR) — `has_pixels: false` in `/api/files`; `/frame/0` returns 404
- [ ] `--port 0` — assigned port printed and matches port server actually binds
- [ ] Explicit port in use — clean error message and non-zero exit code
- [ ] `--timeout 5` — server exits ~5 seconds after last request
- [ ] `Ctrl+C` — clean shutdown; SSH subprocess killed; no zombie processes; exit code 0
- [ ] Directory with 50% non-DICOM files — correct skip count in load summary

### Frontend
- [ ] FileTabs rendered for multiple files; tab label shows PatientID/Modality/Date when available
- [ ] Switching tabs resets frame to 0, loads new tags, loads new image
- [ ] Frame slider visible only for multi-frame files
- [ ] ▶ / ⏸ play-pause advances frames at ~10 fps
- [ ] Keyboard `←` / `→` navigation
- [ ] Frame prefetch — DevTools network panel shows N+1 and N+2 fetches after navigation
- [ ] Tag filter — live filter across Tag, Keyword, VR, Value columns simultaneously
- [ ] SQ row expands to show nested items inline on click
- [ ] Binary tag rendered as `[OW · N bytes]`, not raw data
- [ ] Click-to-copy — clipboard contains correctly formatted tag string; tooltip appears
- [ ] Right-click drag changes W/C values and triggers re-fetch after 150ms debounce
- [ ] Wheel zoom and left-drag pan work; double-click resets to defaults
- [ ] Status bar shows correct URL, file count, uptime; increments uptime every second

### SSH Tunnel
- [ ] `--tunnel --tunnel-host user@host` — correct `ssh -L` command constructed and invoked
- [ ] Readiness polling — "SSH tunnel active" line only printed after successful TCP probe
- [ ] Tunnel subprocess killed on Ctrl+C — no orphaned `ssh` process
- [ ] `ssh` not on `$PATH` — graceful warning with manual command; server still starts

---

## 17. Implementation Order (Recommended for OMP)

1. **`loader.rs`** — file discovery with `walkdir`/`rayon`, parallel metadata extraction, `FileEntry` construction; unit-test against fixture files; verify skip counter
2. **`pixels.rs` — JPEG passthrough only** — collector API, BOT read and cache, raw fragment extraction; round-trip verify with a real JPEG Baseline DICOM using `curl` and compare bytes to a reference extraction
3. **`server.rs` — minimal** — Axum router, AppState, `/api/files` and `/api/file/:index/frame/:n` (JPEG passthrough only); smoke-test with `curl`; confirm `X-Cache` headers present
4. **`pixels.rs` — uncompressed + windowing** — byte-offset frame extraction, big-endian swap, RescaleSlope/Intercept, windowing, PNG encode; verify pixel values at known coordinates against a reference tool
5. **`server.rs` — tag endpoint** — `/api/file/:index/tags`; lazy build, `TagNode` JSON structure; verify SQ nesting, binary truncation, error fallback
6. **Frontend scaffold** — Svelte 5 project, Vite config, `build.rs` integration with `cargo:rerun-if-changed`; verify `rust-embed` serves `index.html` at `/` and assets at `/assets/*`
7. **`FileTabs` + `TagPanel`** — wire to `/api/files` and `/api/file/:index/tags`; implement live filter, SQ expand/collapse, click-to-copy
8. **`ImageViewport` + `FrameSlider`** — frame fetch, zoom/pan CSS transforms, right-click W/L drag with 150ms debounce, keyboard nav, N+1/N+2 prefetch, play-pause cine
9. **`tunnel.rs`** — SSH subprocess, `ExitOnForwardFailure`, readiness TCP polling, stderr logging thread, cleanup on shutdown
10. **Polish** — status bar uptime counter, `--timeout` idle watcher, tunnelled status in UI, startup messages, error states in UI (unsupported TS, no pixel data), `build.rs` incremental fingerprinting

### Rust-specific implementation notes for OMP

- **Collector API and the BOT:** `read_basic_offset_table` may return an empty/all-zeros table for some files. When the BOT is absent, iterate through fragments sequentially to reach frame N — do not assume direct seeking. Cache the per-frame byte positions discovered during iteration for future requests on the same file.
- **`rayon` + `tokio`:** Do not call `rayon` from within an async Tokio context. File discovery in `loader.rs` should run in a `tokio::task::spawn_blocking` block. Similarly, pixel decode/encode for uncompressed frames should use `spawn_blocking` to avoid blocking the async runtime.
- **`Arc<Mutex<LruCache>>` contention:** the Mutex is only held during cache lookup and insert, never during the decode/encode work itself. Compute the frame image first, then lock-insert.
- **`rust-embed` in development:** consider a `debug` feature flag that serves `frontend/dist/` from disk (via `tower-http::services::ServeDir`) to avoid recompiling Rust on every frontend change during development. `rust-embed` supports this pattern with its `debug-embed` feature.
- **Svelte 5 runes:** use `$state` for `activeFileIndex`, `currentFrame`, `windowCenter`, `windowWidth`; use `$derived` for the frame URL string; use `$effect` to trigger tag fetch on file switch. Avoid legacy `$:` reactive declarations.