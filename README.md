![dcmview](https://raw.githubusercontent.com/beatrice-b-m/dcmview/main/dcmview-wordmark-darkmode-opaque-background.png)

# dcmview

`dcmview` is a fast, temporary DICOM viewer for research and development work.
Point it at one or more DICOM files from the command line or Python, and it
starts a local browser viewer for images, tags, cine playback, and rectangular
ROI annotations. Stop the process and the server is gone.

The main problem it solves is remote-server inspection. Medical imaging research
often happens where the data already live: an SSH session, a shared compute
server, or a locked-down institutional network. Viewing those images usually
means choosing between slow notebook plots, setting up a web viewer on the
server, opening firewall ports, or uploading data and annotations into a
third-party cloud tool. `dcmview` keeps the workflow local to the machine with
the files: start the viewer, forward the loopback port over SSH when needed, and
inspect the study in seconds.

`dcmview` is intended for developer and research inspection on secure networks,
not clinical diagnosis. Avoid public-facing server binds; use the default
loopback binding and SSH forwarding for remote workflows.

## Why use it?

- Inspect DICOM files where they already are, including remote servers.
- Avoid notebook-based frame rendering for multi-frame studies.
- Keep data off third-party viewers when all you need is quick review.
- Open a browser UI with familiar viewer tools: pan, zoom, scroll,
  window/level, flips, rotation, tags, and cine playback.
- Load, edit, and export rectangular ROI annotations without modifying the
  source DICOM files.
- Use the same tool from a shell command, a Python script, or a notebook.
- Run as an ephemeral server with no database, config file, or persistent state.

## Install

The current public install channels are the Python package, GitHub Releases, and
the VS Code Marketplace extension:

| Platform | Recommended channel | Notes |
|---|---|---|
| Linux x64 | `dcmview-py` or GitHub Releases | PyPI wheels bundle the `dcmview` binary. |
| macOS x64 | `dcmview-py` or GitHub Releases | PyPI wheels bundle the `dcmview` binary. |
| macOS arm64 | `dcmview-py` or GitHub Releases | PyPI wheels bundle the `dcmview` binary. |
| Windows x64 | `dcmview-py`, GitHub Releases, or VS Code Marketplace | PyPI wheels bundle `dcmview.exe`. |
| VS Code | VS Code Marketplace | The extension bundles platform-specific binaries for supported hosts. |
| Other platforms | Source build | Build the Rust binary locally and point wrappers at it when needed. |

Install the Python package:

```bash
python -m pip install --user dcmview-py
dcmview --help
```

The package installs both `dcmview` and `dcmview-py`; `dcmview` is the primary
command. If you are using an unsupported platform or a local debug binary, set
`DCMVIEW_BINARY` to an absolute path to a compatible `dcmview` executable.

Homebrew distribution is planned for the `v0.2.7` release but is not configured
yet. Until then, use PyPI, the VS Code extension, GitHub Releases, or a source
build.

Source builds are available for contributors and unsupported platforms:

```bash
cargo install --path .
```

Build prerequisites for source installs:

- Rust stable 1.75+
- Node.js 18+ and npm at build time
- `ssh` on `PATH` only when using SSH forwarding helpers

If install or binary discovery fails, see the
[troubleshooting guide](docs/troubleshooting.md).

## Quick Start

Open one file:

```bash
dcmview ./scan.dcm
```

Scan a study directory recursively:

```bash
dcmview ./study_dir
```

Run without opening a browser, useful on a remote server:

```bash
dcmview --no-browser ./study_dir
```

When ready, `dcmview` prints a URL:

```text
dcmview: server running at http://127.0.0.1:<port>
```

Press Ctrl+C to stop the server.

If startup reports skipped files, no valid DICOM files, a port conflict, or a
browser launch failure, see the
[troubleshooting guide](docs/troubleshooting.md).

## Remote Server Workflow

The safest default is to keep `dcmview` bound to loopback on the remote machine
and access it through SSH port forwarding. `dcmview` is intended for research
and development use on secure networks; do not expose the server directly to a
public network.

On the remote server:

```bash
dcmview --no-browser --port 8888 /path/to/dicom_or_study_dir
```

On your local machine:

```bash
ssh -L 8888:localhost:8888 user@remote-server
```

Then open:

```text
http://localhost:8888
```

You can also let `dcmview` use an auto-assigned port by omitting `--port`; copy
the printed port into your SSH command. The optional `--tunnel` flags are
available for environments where the `dcmview` process can start the SSH helper
itself.

The HTTP server is unauthenticated. It binds to `127.0.0.1` by default. If you
bind to `0.0.0.0` or another public interface, use your own network access
controls. Anyone who can reach the server may be able to access image pixels,
DICOM tags, file paths, patient identifiers, study identifiers, and in-memory
annotations.

## Python Usage

`dcmview-py` is a small subprocess wrapper around the Rust binary. It is useful
when a script or notebook has already selected the cases to inspect.

```python
from dcmview_py import view

# Blocking call; returns when dcmview exits.
view(["./scan.dcm"], browser=False, timeout=300)

# Non-blocking call.
handle = view(["./study_dir"], browser=False, block=False)
print(handle.url)
handle.stop()
```

Context manager:

```python
from dcmview_py import view

with view(["./study_dir"], browser=False, block=False) as handle:
    print(handle.url)
```

The module CLI mirrors the Rust options:

```bash
python -m dcmview_py --no-browser --timeout 120 ./study_dir
```

## Viewer Features

The embedded browser viewer includes:

- File tabs labeled from `PatientID`, `Modality`, and `StudyDate` when present.
- Canvas-based image viewing with pan, zoom, scroll, window/level, reset,
  horizontal/vertical flips, and 90-degree rotation.
- Window presets including DICOM defaults, full dynamic range, and common CT
  presets.
- Multi-frame controls with previous/next, cine playback, FPS selection, loop,
  and sweep.
- Lazy DICOM tag browsing with filtering, sequence expansion, binary length
  display, resizable columns, and click-to-copy values.
- Rectangular ROI annotation display and editing, including draw, select, move,
  resize, delete, frame scoping, and CSV export.

Common shortcuts:

| Action | Shortcut |
|---|---|
| Previous/next frame | Left/Right arrows or `[` / `]` |
| Play/pause cine | Space |
| Window/level tool | `W` |
| Pan tool | `P` |
| Zoom tool | `Z` |
| Scroll tool | `S` |
| ROI tool | `R` |
| Reset viewport | Double-click |

Right-drag always zooms, middle-drag always pans, the wheel scrolls frames, and
Ctrl/Cmd+wheel zooms.

## Annotations

`--annotations` loads an EMBED-style ROI CSV into memory:

```bash
dcmview --annotations ./embed_annotations.csv ./study_dir
```

`dcmview` never modifies the input CSV or DICOM files. Viewer edits are kept in
memory and can be downloaded with **Export ROIs**.

Required columns:

- `anon_dicom_path`
- `ROI_coords`

Optional columns:

- `num_ROI`; when present, it must equal `len(ROI_coords)`
- `ROI_frames`; when omitted or `[]`, ROIs apply to all frames

`ROI_coords` is a JSON array of `[ymin, xmin, ymax, xmax]` boxes. `ROI_frames`
is a JSON array of frame-index lists. JSON-valued fields must be CSV-quoted.

```csv
anon_dicom_path,num_ROI,ROI_coords,ROI_frames
/path/to/dbt_case.dcm,2,"[[120,340,220,430],[400,510,480,590]]","[[0,1,2],[5,6]]"
/path/to/ffdm_case.dcm,1,"[[80,150,190,260]]","[]"
```

Matching uses normalized path equality against loaded DICOM paths. Frame indices
are zero-based and must be less than `NumberOfFrames`.

## CLI Reference

```text
dcmview [OPTIONS] <PATH> [PATH ...]
python -m dcmview_py [OPTIONS] <PATH> [PATH ...]
```

The Python module CLI forwards the same options to the underlying `dcmview`
binary.

| Option | Default | Description |
|---|---:|---|
| `<PATH>...` | required | DICOM file or directory to inspect; repeat for multiple inputs |
| `-p, --port <PORT>` | `0` | Local HTTP port to bind; `0` selects an available port |
| `--host <ADDR>` | `127.0.0.1` | Local interface to bind; keep `127.0.0.1` unless you understand the network exposure |
| `--no-browser` | `false` | Print the viewer URL instead of opening a browser automatically |
| `--timeout <SECONDS>` | none | Exit after this many seconds without API or browser requests |
| `--no-recursive` | `false` | Scan only the top level of input directories |
| `--filter <FIELD=VALUE>` | none | Include only files whose metadata field contains the value; repeatable |
| `--tunnel` | `false` | Start an SSH local port-forward helper after the viewer starts |
| `--tunnel-host <SSH_HOST>` | none | SSH host used with `--tunnel`, for example `user@example.org` |
| `--tunnel-port <PORT>` | `0` | Local forwarded port for `--tunnel`; `0` reuses the viewer port |
| `--annotations <CSV>` | none | Load EMBED-style ROI annotations from CSV without modifying the file |

Examples:

```bash
dcmview ./scan.dcm
dcmview --no-recursive ./study_dir
dcmview --host 127.0.0.1 --port 8888 --no-browser ./study_dir
ssh -L 8888:127.0.0.1:8888 user@remote
dcmview --timeout 300 ./study_dir
dcmview --annotations ./embed_annotations.csv ./study_dir
dcmview --filter modality=MR --filter patient_id=1234 ./archive_dir
python -m dcmview_py --no-browser --timeout 120 ./study_dir
```

Filter fields are `patient_id`, `patient_name`, `study_description`,
`study_date`, `study_uid`, `series_description`, `series_number`,
`series_uid`, and `modality`. Matching is case-insensitive substring matching;
multiple filters are combined with AND semantics.

## HTTP API

The browser UI uses a small local HTTP API. This API is internal to the viewer
and is not a stable public integration surface; use it only for `dcmview`
debugging and test automation.

Production builds do not enable cross-origin browser API access for external
debugging tools. To debug the viewer API from another browser origin, build with
`cargo build --features debug-api`; this enables permissive CORS and prints a
build warning. Do not enable `debug-api` outside `dcmview` debugging contexts.

Static frontend assets are served at `/` and `/assets/*`.

| Method | Path | Description |
|---|---|---|
| GET | `/api/health` | Ready-state probe with file count and server start time |
| GET | `/api/files` | File registry and server metadata |
| GET | `/api/file/:index/info` | Frame metadata for one file |
| GET | `/api/file/:index/frame/:frame` | Display frame; supported image transfer syntaxes return PNG |
| GET | `/api/file/:index/frame/:frame/raw` | Decoded frame sample bytes for client-side rendering |
| GET | `/api/file/:index/tags` | Lazy DICOM tag tree |
| GET | `/api/file/:index/annotations` | Current in-memory ROI annotations |
| PUT | `/api/file/:index/annotations` | Replace in-memory ROI annotations for one file |
| GET | `/api/annotations/export.csv` | Download current annotations as EMBED CSV |

### Health and Files

`GET /api/health` returns:

```json
{
  "status": "ok",
  "file_count": 2,
  "server_start_ms": 1714300000000
}
```

`GET /api/files` returns:

```json
{
  "files": [
    {
      "index": 0,
      "path": "/path/to/scan.dcm",
      "label": "PATIENT - MG - 20240101",
      "has_pixels": true,
      "frame_count": 60,
      "rows": 3000,
      "columns": 2500,
      "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
      "default_window": { "center": 200.0, "width": 4000.0 }
    }
  ],
  "tunnelled": false,
  "tunnel_host": null,
  "server_start_ms": 1714300000000,
  "scan_complete": true,
  "scanned": 2,
  "skipped": 0,
  "filtered": 0
}
```

`GET /api/file/:index/info` returns `frame_count`, `rows`, `columns`,
`transfer_syntax`, `has_pixels`, and `default_window`.

### Display Frames

`GET /api/file/:index/frame/:frame` returns `image/png` for supported display
paths.

Query parameters:

| Param | Description |
|---|---|
| `wc` | Window center; used with `ww` in default mode |
| `ww` | Window width; used with `wc` in default mode |
| `mode` | `default` or `full_dynamic` |

Window selection:

- `default`: explicit `wc` and `ww`, then DICOM Window Center/Width, then
  1st/99th percentile fallback.
- `full_dynamic`: true min/max of the current frame; ignores DICOM defaults and
  query window values.

Transfer syntax behavior:

| Transfer syntax | Display behavior |
|---|---|
| JPEG Baseline / Extended | Decoded server-side and PNG-encoded |
| JPEG Lossless / Lossless SV1 | Decoded server-side and PNG-encoded |
| JPEG 2000 lossless/lossy | Decoded server-side and PNG-encoded |
| Implicit LE / Explicit LE / Explicit BE | Windowed server-side and PNG-encoded |
| JPEG-LS / RLE / other | `422 {"error": "unsupported transfer syntax: ..."}` |

Response headers include `X-Cache: HIT` or `X-Cache: MISS`.

### Raw Frames

`GET /api/file/:index/frame/:frame/raw` returns `application/octet-stream` plus
metadata headers. This is a decoded sample transport for the frontend, not a
copy of the original DICOM Pixel Data element for compressed syntaxes.

Supported raw paths:

- Uncompressed frames: native sample bytes, normalized to little-endian by
  `dicom-object`.
- JPEG Baseline / Extended: decoded to 8-bit grayscale samples.
- JPEG Lossless: decoded to 8-bit or 16-bit grayscale samples when supported by
  the codec stack.
- Grayscale JPEG 2000: decoded to 8-bit or 16-bit samples.

JPEG-LS, RLE, unsupported syntaxes, and multi-component JP2 raw decoding return
422 or a decode error.

### Tags and Annotations

`GET /api/file/:index/tags` returns an array of DICOM tag nodes. Pixel data and
other binary VRs are represented by byte length, not by full values. Long
numeric arrays and sequences may be truncated with `truncated` and `total`
fields.

`GET /api/file/:index/annotations` returns:

```json
{
  "num_roi": 2,
  "roi_coords": [[120, 340, 220, 430], [400, 510, 480, 590]],
  "roi_frames": [[0, 1, 2], [5, 6]]
}
```

`PUT /api/file/:index/annotations` replaces one file's in-memory annotations and
returns the canonicalized payload. Invalid coordinates or frame mappings return
`400 {"error": "..."}`.

## Development

Frontend:

```bash
npm --prefix frontend ci
dcmview --no-browser --host 127.0.0.1 --port 8888 tests/fixtures
npm --prefix frontend run dev
npm --prefix frontend run build
```

The Vite dev server proxies `/api` to `http://127.0.0.1:8888`, so start a
backend on that host and port before using the standalone frontend dev server.

Backend:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo check
cargo build
cargo build --release
```

Tests:

```bash
cargo test
```

Integration tests use real DICOM fixtures and cover discovery, display-frame
decoding, raw-frame transport, cache headers, tag serialization, annotations,
and tunnel fallback.

Architecture summary:

- Backend: Rust, Axum, Tokio
- DICOM: `dicom-rs`, `dicom-pixeldata`, `jpeg2k`
- Pixel pipeline: server-side display PNGs, raw sample transport for
  interactive rendering, LRU caches
- Frontend: Svelte 5, Vite, TypeScript, embedded via `rust-embed`
- Distribution: one executable with no runtime frontend assets

Backend frame cache budgets are currently 256 MiB for display PNGs and 384 MiB
for raw sample frames. The frontend also keeps active frame blobs, raw buffers,
and rendered bitmaps in memory for responsiveness, so cache budget changes
should consider total browser plus server memory pressure.

## Reporting Issues

Use GitHub issues for DICOM compatibility problems, install failures, and feature
requests. Do not attach DICOM files, screenshots, logs, paths, patient
identifiers, or other sensitive information unless you have fully de-identified
them.

Before filing an issue, check the
[troubleshooting guide](docs/troubleshooting.md) for common install, startup,
decode, tunnel, VS Code, and annotation CSV problems.

Report suspected security vulnerabilities privately to the maintainers before
public disclosure; see `SECURITY.md`.

`dcmview` is not for clinical use, clinical diagnosis, or clinical
decision-making.

## License

MIT
