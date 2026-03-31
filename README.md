# dcmview

`dcmview` is an ephemeral DICOM inspection tool for developers and data scientists.

It scans one or more DICOM files/directories, starts a local web UI, serves image frames and tags through a small HTTP API, and exits cleanly when you stop it.

The focus is fast multi-frame inspection (DBT, cine MR) with a single self-contained Rust binary.

## Current status

Implemented and test-covered core stack:

- CLI startup flow (`discover -> bind -> serve`)
- DICOM discovery with skip accounting (`walkdir` + `rayon` in `spawn_blocking`)
- Frame serving pipeline:
  - JPEG passthrough (`image/jpeg`)
  - JPEG 2000 passthrough (`image/jp2`) with decode fallback to PNG when `Accept` does not include `image/jp2`
  - Uncompressed decode + windowing + PNG
- `X-Cache: HIT|MISS` on frame responses
- Lazy tag-tree endpoint with sequence support and binary truncation
- Embedded Svelte frontend served from Rust binary
- Optional SSH tunnel lifecycle + graceful fallback when `ssh` is missing
- Idle timeout shutdown (`--timeout`) and signal-based graceful shutdown

## Architecture (high level)

- **Backend**: Rust + Axum + Tokio
- **DICOM engine**: `dicom-rs` crates (`dicom-object`, `dicom-pixeldata`, etc.)
- **Frontend**: Svelte 5 + Vite + TypeScript
- **Distribution**: single binary with frontend assets embedded via `rust-embed`

## Prerequisites

Build-time:

- Rust stable (1.75+ recommended)
- Node.js 18+
- npm

Runtime:

- `ssh` on `PATH` only if you use `--tunnel`

## Installation

### From source (local)

```bash
# from repo root
npm --prefix frontend ci
cargo install --path .
```

### Build without install

```bash
npm --prefix frontend ci
cargo build --release
```

Resulting binary:

- `target/release/dcmview` (build)
- `$CARGO_HOME/bin/dcmview` (install)

## Deployment

`dcmview` is designed for straightforward binary deployment.

### Option A: single-host local deployment

1. Build once on your CI/build machine:

```bash
cargo build --release
```

2. Copy `target/release/dcmview` to target host.
3. Run it directly against local file paths.

### Option B: remote host deployment with local browser access

Run on a remote server and let `dcmview` establish SSH forwarding:

```bash
dcmview --host 127.0.0.1 --port 8432 --tunnel --tunnel-host user@my-server /data/study
```

`dcmview` prints the local forwarded URL once the tunnel probe is ready.

If `ssh` is unavailable, it keeps running and prints a manual forwarding command.

### Operational notes

- No persistent state, database, or config files are created.
- Bind to loopback (`127.0.0.1`) by default.
- If binding publicly (`--host 0.0.0.0`), place it behind your own network controls.

## Usage

### CLI synopsis

```bash
dcmview [OPTIONS] <PATH> [PATH ...]
```

### Options

- `-p, --port <u16>`: bind port (`0` = auto assign)
- `--host <addr>`: bind host (default `127.0.0.1`)
- `--no-browser`: do not auto-open browser
- `--tunnel`: enable SSH local forwarding
- `--tunnel-host <user@host>`: SSH target (required with `--tunnel`)
- `--tunnel-port <u16>`: forwarded local port (`0` = use bind port)
- `--timeout <seconds>`: auto-shutdown after idle duration
- `--no-recursive`: disable recursive directory traversal

### Examples

Single file:

```bash
dcmview ./scan.dcm
```

Directory scan (recursive default):

```bash
dcmview ./study_dir
```

Fixed host/port:

```bash
dcmview --host 127.0.0.1 --port 8888 ./study_dir
```

Remote tunnel workflow:

```bash
dcmview --tunnel --tunnel-host user@remote --tunnel-port 9000 ./study_dir
```

Headless/automation style run with idle timeout:

```bash
dcmview --no-browser --timeout 300 ./study_dir
```

## Frontend behavior summary

**Toolbar (top of viewport):**
- Tool selector: WL (Window/Level), Pan, Zoom, Scroll — determines left-drag behavior
- W/L preset dropdown: Default, Full Dynamic, and standard CT presets (Abdomen, Angio, Bone, Brain, Chest, Lung)
- Reset button: resets zoom/pan and W/L to DICOM default; keyboard shortcuts W/P/Z/S switch tools

**Viewport mouse model:**
- Left drag: routes by active tool (W/L / pan / zoom / scroll-through-frames)
- Right drag: always zoom (hard-coded)
- Middle drag: always pan (hard-coded)
- Wheel (multi-frame): scrub frames; Ctrl/Cmd + wheel: zoom
- Double-click: full reset (zoom, pan, W/L)

**Cine controls (multi-frame files):**
- Play/pause, selectable fps (1 / 5 / 10 / 15 / 24), Loop or Sweep mode
- Keyboard: arrow keys or `[` / `]` for frame nav, Space for play/pause

**Tag panel:** filtering, SQ expansion, click-to-copy

**Zoom/pan:** CSS transforms only — no re-fetch unless W/L or window mode changes
## HTTP API

- `GET /api/files`
- `GET /api/file/:index/info`
- `GET /api/file/:index/frame/:frame?wc=&ww=&mode=`
  - `?mode=full_dynamic`: window spans true min/max of frame samples, ignores DICOM default_window
  - `?mode=default` (or absent): explicit wc/ww → DICOM default_window → percentile fallback
- `GET /api/file/:index/tags`

Frame responses include `X-Cache: HIT|MISS`. Cache is keyed on `(file_index, frame, wc, ww, mode)`.

## Development

### Frontend

```bash
npm --prefix frontend ci
npm --prefix frontend run dev
```

### Backend

```bash
cargo check
cargo build
cargo test
```

## Testing

Integration tests use real generated DICOM fixtures (no codec mocks) and cover:

- discovery and skip accounting
- JPEG and JP2 frame behavior
- uncompressed windowing path
- cache semantics (`X-Cache`)
- tags endpoint sequence/binary serialization
- tunnel fallback behavior

Run all tests:

```bash
cargo test
```

## Project principles

- Ephemeral runtime
- Performance-first frame serving
- Single-binary distribution
- Predictable operational behavior
