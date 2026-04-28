# dcmview

`dcmview` is an ephemeral DICOM inspection tool for developers and data scientists.
It scans one or more DICOM files or directories, starts a local web UI, serves image frames and tags through a small HTTP API, and exits cleanly when you stop it.

The focus is fast multi-frame inspection (DBT, cine MR) with a single self-contained Rust binary.

## Installation

### From source

```bash
# install frontend build dependencies once
npm --prefix frontend ci

# build and install to $CARGO_HOME/bin
cargo install --path .
```

Build without installing:

```bash
npm --prefix frontend ci
cargo build --release
# binary at target/release/dcmview
```

### Prerequisites

Build-time: Rust stable 1.75+, Node.js 18+, npm.
Runtime: `ssh` on `PATH` (only if you use `--tunnel`).

## Quick start

```bash
# open a single file in the browser
dcmview ./scan.dcm

# scan a directory recursively
dcmview ./study_dir

# fixed port, no browser, auto-shutdown after 5 minutes idle
dcmview --host 127.0.0.1 --port 8888 --no-browser --timeout 300 ./study_dir
```

`dcmview` prints the local URL (`dcmview: server running at http://...`) as soon as the server is ready.

## CLI reference

```
dcmview [OPTIONS] <PATH> [PATH ...]
```

### Options

| Flag | Type | Default | Description |
|---|---|---|---|
| `<PATH>` | path(s) | required | One or more DICOM files or directories |
| `-p, --port` | u16 | `0` | Bind port (`0` = auto-assign) |
| `--host` | addr | `127.0.0.1` | Bind address (`0.0.0.0` for all interfaces) |
| `--no-browser` | flag | — | Do not auto-open browser |
| `--tunnel` | flag | — | Establish SSH reverse tunnel |
| `--tunnel-host` | str | — | SSH host string (e.g. `user@myserver.com`) |
| `--tunnel-port` | u16 | `0` | Local port to expose on tunnel host (`0` = same as bind port) |
| `--timeout` | u64 | — | Auto-shutdown after N seconds of no browser requests |
| `--no-recursive` | flag | — | Scan directories top-level only (default is recursive) |
| `--annotations` | path | — | Load EMBED ROI annotations from CSV for overlay display |

### Examples

Single file:

```bash
dcmview ./scan.dcm
```

Directory scan (recursive by default):

```bash
dcmview ./study_dir
```

Fixed host and port:

```bash
dcmview --host 127.0.0.1 --port 8888 ./study_dir
```

Remote tunnel workflow:

```bash
dcmview --tunnel --tunnel-host user@remote --tunnel-port 9000 ./study_dir
```

Headless run with idle timeout:

```bash
dcmview --no-browser --timeout 300 ./study_dir
```

With EMBED ROI annotations:

```bash
dcmview --annotations ./embed_annotations.csv ./study_dir
```

### EMBED annotation CSV format

`--annotations` accepts a CSV file. The parser is strict and fails startup on malformed rows.

Required columns (exact names):

- `anon_dicom_path` — path to the DICOM file
- `num_ROI` — integer, number of regions of interest
- `ROI_coords` — JSON array of `[ymin, xmin, ymax, xmax]` bounding boxes
- `ROI_frames` — JSON array of frame-index lists (or `[]` for non-frame-specific rows)

JSON-valued fields must be CSV-quoted:

```csv
anon_dicom_path,num_ROI,ROI_coords,ROI_frames
/path/to/dbt_case.dcm,2,"[[120,340,220,430],[400,510,480,590]]","[[0,1,2],[5,6]]"
/path/to/ffdm_case.dcm,1,"[[80,150,190,260]]","[]"
```

Matching and behavior:

- Matching is by normalized path equality: `anon_dicom_path` must match the loaded DICOM path after path normalization
- CSV rows without a matching loaded file are ignored
- Loaded files without a matching CSV row show no ROIs
- `len(ROI_coords)` must equal `num_ROI`
- If `ROI_frames` is non-empty, its length must equal `num_ROI`
- Frame indices are zero-based and must be `< NumberOfFrames` for the matched file
- Duplicate `anon_dicom_path` rows are rejected

## Deployment

### Local

```bash
cargo build --release
# copy target/release/dcmview to target host; run directly
```

### Remote host with local browser access

```bash
dcmview --host 127.0.0.1 --port 8432 --tunnel --tunnel-host user@my-server /data/study
```

`dcmview` prints the local forwarded URL once the tunnel probe is ready. If `ssh` is unavailable, it keeps running and prints a manual forwarding command.

### Operational notes

- No persistent state, database, or config files are created
- Binds to loopback (`127.0.0.1`) by default
- If binding publicly (`--host 0.0.0.0`), place behind your own network controls

## Python wrapper

The Python package (`dcmview-py`) is a thin subprocess wrapper around the `dcmview` binary. It does not bundle the Rust binary — `dcmview` must be on `PATH`.

### Install

```bash
cargo install --path .           # install the Rust binary
python -m pip install -e .       # install the Python wrapper
python -m dcmview_py --help      # verify
```

### Script usage

```python
from dcmview_py import view

# blocking call (returns when dcmview exits)
view(["./scan.dcm"], browser=False, timeout=300)

# non-blocking
handle = view(["./study_dir"], browser=False, annotations="./embed_annotations.csv", block=False)
print(handle.url)
handle.stop()
```

### Context manager

```python
from dcmview_py import view

with view(["./study_dir"], browser=False, block=False) as handle:
    print(handle.url)
```

### Notebook usage

No inline notebook renderer is provided. Use the returned URL in your browser:

```python
from dcmview_py import view

handle = view(["./scan.dcm"], browser=False, block=False)
print(f"Open in browser: {handle.url}")
handle.stop()
```

### CLI entrypoint

```bash
python -m dcmview_py --no-browser --timeout 120 ./study_dir
python -m dcmview_py --annotations ./embed_annotations.csv ./study_dir
```

Module flags mirror the Rust CLI options (`--host`, `--port`, `--tunnel`, `--no-recursive`, `--annotations`, etc.).

## HTTP API

### Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/files` | File registry and server metadata |
| GET | `/api/file/:index/info` | Frame dimensions and transfer syntax |
| GET | `/api/file/:index/frame/:frame` | Rendered frame image (JPEG, JP2, or PNG) |
| GET | `/api/file/:index/frame/:frame/raw` | Raw pixel data with metadata headers |
| GET | `/api/file/:index/tags` | DICOM tag tree |
| GET | `/api/file/:index/annotations` | EMBED ROI annotations for this file |

Static assets are served at `/` (index.html) and `/assets/*` (Svelte build output).

### `GET /api/files`

Returns the file registry and server state.

Response:

```json
{
  "files": [
    {
      "index": 0,
      "path": "/path/to/scan.dcm",
      "label": "PATIENT · MG · 20240101",
      "has_pixels": true,
      "frame_count": 1,
      "rows": 3000,
      "columns": 2500,
      "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
      "default_window": { "center": 200.0, "width": 4000.0 }
    }
  ],
  "tunnelled": false,
  "tunnel_host": null,
  "server_start_ms": 1714300000000
}
```

### `GET /api/file/:index/info`

Returns frame-level metadata for a single file.

Response:

```json
{
  "frame_count": 60,
  "rows": 3000,
  "columns": 2500,
  "transfer_syntax": "1.2.840.10008.1.2.4.50",
  "has_pixels": true,
  "default_window": { "center": 200.0, "width": 4000.0 }
}
```

Returns 404 if `:index` is out of range.

### `GET /api/file/:index/frame/:frame`

Returns image bytes for a specific frame.

Query parameters:

| Param | Type | Description |
|---|---|---|
| `wc` | f64 | Explicit window center (overrides DICOM default) |
| `ww` | f64 | Explicit window width (overrides DICOM default) |
| `mode` | string | `default` (absent) or `full_dynamic` |

Window resolution order for `default` mode: query params → DICOM tags (0028,1050/1051) → 1st/99th percentile fallback.

`full_dynamic` mode ignores all overrides and computes window from true min/max of frame samples.

Response varies by transfer syntax:

| Transfer syntax | Content-Type | Behavior |
|---|---|---|
| JPEG Baseline / Extended / Lossless / SV1 | `image/jpeg` | Raw JPEG bytes, no server-side decode |
| JPEG 2000 | `image/jp2` | Raw JP2 bytes if `Accept` includes `image/jp2`; falls back to PNG |
| Uncompressed (LE/BE) | `image/png` | Windowed and PNG-encoded |
| JPEG-LS / RLE | `image/png` | Decoded and PNG-encoded, or 422 if unsupported |

Response headers:

- `X-Cache: HIT` or `X-Cache: MISS` — frame cache observability

Status codes:

- `200` — image bytes
- `404` — file index out of range, or file has no pixel data
- `422` — unsupported transfer syntax (`{"error": "unsupported transfer syntax: {uid}"}`)
- `500` — frame decode failed (`{"error": "frame decode failed: ..."}`)

### `GET /api/file/:index/frame/:frame/raw`

Returns raw pixel data for client-side rendering.

Response headers (metadata):

- `X-Frame-Rows`, `X-Frame-Columns`
- `X-Frame-Bits-Allocated`, `X-Frame-Pixel-Representation`
- `X-Frame-Samples-Per-Pixel`, `X-Frame-Photometric-Interpretation`
- `X-Frame-Rescale-Slope`, `X-Frame-Rescale-Intercept`
- `X-Frame-Default-Wc`, `X-Frame-Default-Ww` (if available)

Body: raw pixel buffer.

### `GET /api/file/:index/tags`

Returns the full DICOM tag tree for a file, lazily built on first request.

Response: array of `TagNode` objects.

```json
[
  {
    "tag": "(0008,0060)",
    "vr": "CS",
    "keyword": "Modality",
    "value": { "type": "string", "value": "MG" }
  },
  {
    "tag": "(0028,0010)",
    "vr": "US",
    "keyword": "Rows",
    "value": { "type": "number", "value": 3000 }
  },
  {
    "tag": "(7FE0,0010)",
    "vr": "OW",
    "keyword": "PixelData",
    "value": { "type": "binary", "length": 15000000 }
  }
]
```

Tag value types: `string`, `number`, `numbers`, `binary` (OB/OW/OD/OF/UN with length only), `sequence` (nested items), `error` (serialisation fallback).

Tags are in ascending (group, element) order. Sequences contain nested `TagNode[][]` arrays.

### `GET /api/file/:index/annotations`

Returns EMBED ROI annotations for a file.

```json
{
  "num_roi": 2,
  "roi_coords": [[120, 340, 220, 430], [400, 510, 480, 590]],
  "roi_frames": [[0, 1, 2], [5, 6]]
}
```

Returns an empty payload (`{"num_roi": 0, "roi_coords": [], "roi_frames": []}`) for files without matching annotations.

## Frontend

The embedded Svelte 5 frontend provides:

- **File tabs** — one tab per file, labeled `PatientID · Modality · StudyDate` (or filename)
- **Image viewport** — frame display with zoom/pan (CSS transforms), window/level adjustment, and tool switching (WL, Pan, Zoom, Scroll)
- **Viewer toolbar** — tool selector, W/L preset dropdown (Default, Full Dynamic, CT presets), reset button
- **Frame slider** — for multi-frame files: prev/next, cine play/pause, FPS selector (1–24), loop/sweep mode
- **Tag panel** — filterable DICOM tag table with SQ expansion, binary length display, click-to-copy
- **Status bar** — server URL, file count, live uptime

Mouse model: left-drag routes by active tool; right-drag always zooms; middle-drag always pans; wheel scrubs frames (Ctrl/Cmd+wheel for zoom); double-click resets.

Keyboard: arrow keys or `[` / `]` for frame navigation, Space for play/pause, W/P/Z/S to switch tools.

## Development

### Frontend

```bash
npm --prefix frontend ci
npm --prefix frontend run dev     # watch mode
npm --prefix frontend run build   # production build
```

### Backend

```bash
cargo check
cargo build
cargo build --release
```

### Tests

```bash
cargo test
```

Integration tests use real DICOM fixtures (no codec mocks) and cover discovery, JPEG/JP2 frame behavior, uncompressed windowing, cache semantics, tags, and tunnel fallback.

### Architecture

- **Backend**: Rust + Axum + Tokio
- **DICOM engine**: `dicom-rs` crates (collector API for per-frame streaming)
- **Pixel pipeline**: JPEG/JP2 passthrough (zero server-side decode), uncompressed windowing + PNG, LRU frame cache
- **Frontend**: Svelte 5 + Vite + TypeScript, embedded in the binary via `rust-embed`
- **Distribution**: single self-contained binary

## License

MIT
