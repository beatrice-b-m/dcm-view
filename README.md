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

### Quick start (CLI)

If you want to run the Rust CLI directly from this repository:

1. Install frontend build dependencies once:

```bash
npm --prefix frontend ci
```

2. Install `dcmview` to your Cargo bin path:

```bash
cargo install --path .
```

3. Launch the viewer against one or more files/directories:

```bash
dcmview ./study_dir
```

`dcmview` prints the local URL (`dcmview: server running at http://...`) as soon as the server is ready.

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
- `--annotations <path>`: load read-only EMBED ROI CSV annotations for overlay/list viewing

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

Run with EMBED ROI annotations (strict CSV validation at startup):

```bash
dcmview --annotations ./embed_annotations.csv ./study_dir
```

### EMBED annotation CSV requirements

`--annotations` accepts a CSV file only. The parser is strict and fails startup on malformed rows.

Required columns (exact names):

- `anon_dicom_path`
- `num_ROI`
- `ROI_coords`
- `ROI_frames`

Expected row encoding:

- `num_ROI`: integer
- `ROI_coords`: JSON list of `[ymin, xmin, ymax, xmax]` boxes
- `ROI_frames`: JSON list of frame-index lists (or `[]` for non-frame-specific rows)
- JSON-valued fields must be CSV-quoted (see example below)

Example CSV:

```csv
anon_dicom_path,num_ROI,ROI_coords,ROI_frames
/path/to/dbt_case.dcm,2,"[[120,340,220,430],[400,510,480,590]]","[[0,1,2],[5,6]]"
/path/to/ffdm_case.dcm,1,"[[80,150,190,260]]","[]"
```

Matching and behavior notes:

- Matching is by normalized path equality: `anon_dicom_path` must match the loaded DICOM path after path normalization.
- CSV rows without a matching loaded file are ignored.
- Loaded files without a matching CSV row remain valid and show no ROIs.
- `len(ROI_coords)` must equal `num_ROI`.
- If `ROI_frames` is non-empty, its length must equal `num_ROI`.
- Frame indices are zero-based and must be `< NumberOfFrames` for the matched file(s).
- Duplicate `anon_dicom_path` rows are rejected.
## Python wrapper package

> The Python package is a thin subprocess wrapper around the `dcmview` binary.

Install from this repository:

```bash
# from repo root
python -m pip install -e .
```

The wrapper does not reimplement DICOM logic. It launches the installed Rust binary and streams the same web UI/API behavior.

### Binary requirement

The Python API requires `dcmview` on `PATH` (for example via `cargo install --path .` or `cargo install dcmview`).

### Python package quick start

The Python package is a subprocess wrapper. It does not bundle the Rust binary.

Typical setup from this repo:

```bash
# 1) install the Rust binary
cargo install --path .

# 2) install the Python wrapper package
python -m pip install -e .

# 3) verify wrapper import and binary resolution
python -m dcmview_py --help
```

### Script usage

```python
from dcmview_py import view

# Blocking call (returns when dcmview exits)
view(["./scan.dcm"], browser=False, timeout=300)

# Non-blocking call with EMBED annotations
handle = view(["./study_dir"], browser=False, annotations="./embed_annotations.csv", block=False)
print(handle.url)
# ...do other work...
handle.stop()
```

Context-manager form for deterministic cleanup:

```python
from dcmview_py import view

with view(["./study_dir"], browser=False, block=False) as handle:
	print(handle.url)
	# notebook/script work continues while dcmview serves frames
```

### Notebook usage

No inline notebook renderer is provided. Use the returned URL in your browser:

```python
from dcmview_py import view

handle = view(["./scan.dcm"], browser=False, block=False)
print(f"Open in browser: {handle.url}")
# When done:
handle.stop()
```

### Python CLI entrypoint

The package also exposes CLI-compatible execution via module mode:

```bash
python -m dcmview_py --no-browser --timeout 120 ./study_dir
python -m dcmview_py --annotations ./embed_annotations.csv ./study_dir
```

Module flags mirror the Rust CLI options (`--host`, `--port`, `--tunnel`, `--no-recursive`, `--annotations`, etc.).

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
- `GET /api/file/:index/annotations`
  - Returns `{ num_roi, roi_coords, roi_frames }` in EMBED schema shape
  - Returns an empty payload for files without a matching annotation row

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
