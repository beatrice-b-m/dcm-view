# dcmview Internal HTTP API

The `dcmview` browser UI talks to the local Rust server through this HTTP API.
The API is internal to the viewer and intended for `dcmview` debugging, smoke
tests, and local automation only. It is not a stable public integration
contract.

The server is unauthenticated. Keep it bound to loopback for normal use and use
SSH forwarding for remote workflows. API responses can expose DICOM metadata,
file paths, annotations, and rendered or decoded pixel data.

The authoritative Rust wire declarations and endpoint operations—including
method, route, query/body types, success status, response media type, response
header kind, and error type—live in `src/api/contracts.rs`. Browser-facing
TypeScript types and typed endpoint metadata are generated from that source into
`frontend/src/generated/api-types.ts`; `frontend/src/api.ts` is the fetch
boundary used by Svelte components. See the
[architecture and test model](architecture.md#executable-http-contract) for
ownership and dependency direction.

## Contract Enforcement

The endpoint registry is executable rather than documentation-only:

- `src/server/api/routes.rs` registers each declared operation and constrains
  its handler-facing request, response, header, and error types.
- Rust unit tests check operation, method/path, query, media, header, and wire
  type invariants.
- Axum integration tests execute every declared endpoint and compare its
  success status, content type, and required headers with the registry.
- `npm --prefix frontend run check:contracts` rejects generated TypeScript
  drift and tests the Rust-to-TypeScript generator.

When adding or changing an endpoint, update the Rust declaration first,
regenerate the checked-in TypeScript, use the typed wrapper in `api.ts`, and
extend the runtime contract tests.

## Cross-Origin Debugging

Normal builds do not enable cross-origin browser access to the API. To inspect
the API from a separate browser origin while debugging `dcmview`, build with:

```bash
cargo build --features debug-api
```

The `debug-api` feature enables permissive CORS and prints a build warning. Do
not use it for normal research workflows or public-facing deployments.

## Endpoints

Static frontend assets are served at `/` and `/assets/*`.

| Method | Path | Description |
|---|---|---|
| GET | `/api/health` | Ready-state probe with viewer build identity, file count, and server start time. |
| GET | `/api/files` | File registry, tunnel metadata, and progressive scan status. |
| GET | `/api/file/:index/info` | Frame metadata for one file. |
| GET | `/api/file/:index/frame/:frame` | Display frame; supported image transfer syntaxes return PNG. |
| GET | `/api/file/:index/frame/:frame/raw` | Decoded frame sample bytes plus rendering metadata headers. |
| GET | `/api/file/:index/tags` | Lazy DICOM tag tree. |
| GET | `/api/file/:index/annotations` | Current in-memory ROI annotations for one file. |
| PUT | `/api/file/:index/annotations` | Replace in-memory ROI annotations for one file. |
| GET | `/api/annotations/export.csv` | Download current annotations as EMBED-style CSV. |

## Health

`GET /api/health` returns JSON:

```json
{
  "status": "ok",
  "file_count": 2,
  "server_start_ms": 1714300000000
}
```

`file_count` is the number of valid DICOM files currently visible to the
registry. During progressive directory scans, it can grow after the server is
ready.

## File Registry

`GET /api/files` returns file summaries, tunnel metadata, and scan progress:

```json
{
  "files": [
    {
      "index": 0,
      "path": "/path/to/scan.dcm",
      "label": "PATIENT - MG - 20240101",
      "patient_id": "PATIENT",
      "patient_name": "PATIENT",
      "study_instance_uid": "1.2.3",
      "study_date": "20240101",
      "study_description": "Screening",
      "series_instance_uid": "1.2.3.4",
      "series_number": "1",
      "series_description": "MLO",
      "modality": "MG",
      "instance_number": "1",
      "sop_instance_uid": "1.2.3.4.5",
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

Progress fields:

| Field | Meaning |
|---|---|
| `scan_complete` | `true` after all requested paths have been scanned. |
| `scanned` | Count of valid DICOM files accepted into the registry. |
| `skipped` | Count of files skipped because they could not be read as supported DICOM objects. |
| `filtered` | Count of readable DICOM files excluded by `--filter` metadata filters. |

Scripts should poll `/api/files` while `scan_complete` is `false` if they need a
complete scan result:

```js
async function waitForCompleteFiles() {
  while (true) {
    const response = await fetch("/api/files");
    const body = await response.json();
    if (body.scan_complete) return body.files;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}
```

For workflows that only need the first available file, poll until
`files.length > 0` or `scan_complete` becomes `true`.

## File Info

`GET /api/file/:index/info` is the specialized single-file metadata endpoint.
It is useful when a client already knows the file index and does not need to
fetch the complete registry payload. It returns:

```json
{
  "frame_count": 60,
  "rows": 3000,
  "columns": 2500,
  "transfer_syntax_uid": "1.2.840.10008.1.2.4.50",
  "has_pixels": true,
  "default_window": { "center": 200.0, "width": 4000.0 }
}
```

An unknown file index returns `404 {"error": "file index out of range"}`.

## Display Frames

`GET /api/file/:index/frame/:frame` returns `image/png` for supported display
paths. Supported compressed and native transfer syntaxes are decoded server-side
before PNG encoding; the endpoint does not return original compressed DICOM
fragments.

Query parameters:

| Parameter | Description |
|---|---|
| `wc` | Window center. Used with `ww` in default mode. |
| `ww` | Window width. Used with `wc` in default mode. |
| `mode` | `default` or `full_dynamic`. |

Window selection order:

1. `mode=full_dynamic`, which uses current-frame min/max and ignores explicit
   and DICOM window values.
2. Explicit `wc` and `ww`.
3. DICOM Window Center/Width from loader metadata.
4. 1st/99th percentile fallback from current-frame samples.

Transfer syntax behavior:

| Transfer syntax | Display behavior |
|---|---|
| JPEG Baseline / Extended | Decoded server-side and PNG-encoded. |
| JPEG Lossless / Lossless SV1 | Decoded server-side and PNG-encoded. |
| JPEG 2000 lossless/lossy | Decoded server-side and PNG-encoded. |
| Implicit LE / Explicit LE / Explicit BE | Windowed server-side and PNG-encoded. |
| JPEG-LS / RLE / other | `422 {"code":"unsupported_transfer_syntax","error":"unsupported transfer syntax: ..."}`. |

Every successful display-frame response includes `X-Cache: HIT` or
`X-Cache: MISS`. The display cache key includes file index, frame index, window
center, window width, and window mode.

## Raw Frames

`GET /api/file/:index/frame/:frame/raw` returns `application/octet-stream`. This
endpoint transports decoded samples for client-side rendering; for compressed
syntaxes it is not a byte-for-byte copy of the DICOM Pixel Data element.

Supported raw paths:

| Transfer syntax | Raw behavior |
|---|---|
| Uncompressed | Native sample bytes normalized to little-endian by `dicom-object`. |
| JPEG Baseline / Extended | Decoded to 8-bit grayscale samples. |
| JPEG Lossless | Decoded to 8-bit or 16-bit grayscale samples when supported by the codec stack. |
| Grayscale JPEG 2000 | Decoded to 8-bit or 16-bit samples. |
| JPEG-LS / RLE / unsupported | `422` or a decode error. |
| Multi-component JPEG 2000 raw decode | `422` or a decode error. |

Successful raw-frame responses include `X-Cache: HIT` or `X-Cache: MISS`. The
raw cache key includes file index and frame index only; window parameters do not
change raw sample bytes.

Required metadata headers:

| Header | Meaning |
|---|---|
| `X-Frame-Rows` | Frame row count. |
| `X-Frame-Columns` | Frame column count. |
| `X-Frame-Bits-Allocated` | Bits allocated per sample. |
| `X-Frame-Pixel-Representation` | `0` for unsigned samples, `1` for signed samples. |
| `X-Frame-Samples-Per-Pixel` | Samples per pixel in the raw response. |
| `X-Frame-Photometric-Interpretation` | Photometric interpretation used by the renderer. |
| `X-Frame-Rescale-Slope` | DICOM rescale slope. |
| `X-Frame-Rescale-Intercept` | DICOM rescale intercept. |

Optional metadata headers:

| Header | Meaning |
|---|---|
| `X-Frame-Default-Wc` | DICOM default window center, when available. |
| `X-Frame-Default-Ww` | DICOM default window width, when available. |

## Tags

`GET /api/file/:index/tags` returns an array of tag nodes:

```json
[
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

Pixel Data and other binary VRs are represented by byte length, not by full
values. Long numeric arrays and sequences can include `truncated` and `total`
fields. Individual serialization failures are returned as
`{"type": "error", "message": "..."}` values so one bad tag does not prevent
the rest of the tag tree from rendering.

## Annotations

`GET /api/file/:index/annotations` returns the current in-memory EMBED-style ROI
payload for one file:

```json
{
  "num_roi": 2,
  "roi_coords": [[120, 340, 220, 430], [400, 510, 480, 590]],
  "roi_frames": [[0, 1, 2], [5, 6]]
}
```

`PUT /api/file/:index/annotations` replaces one file's in-memory annotations and
returns the canonicalized payload. Coordinates and frame indices are validated
against the file dimensions and frame count. Invalid payloads return
`400 {"error": "..."}`.

`GET /api/annotations/export.csv` returns `text/csv; charset=utf-8` with
`Content-Disposition: attachment; filename="dcmview-annotations.csv"`. The CSV
is generated from current in-memory annotations; source DICOM files and input
CSV files are not modified.

## Error Semantics

All API failures—including path, query, and JSON extractor rejections—use a JSON
body:

```json
{ "error": "file index out of range" }
```

Common statuses:

| Status | Typical cause |
|---|---|
| `400` | Malformed path/query input, an invalid window pair, or invalid annotation values. |
| `404` | Unknown API route or file index, missing pixel data, or out-of-range frame. |
| `405` | Unsupported method on a known API route. |
| `422` | Structurally invalid JSON, unsupported transfer syntax, or unsupported raw component layout. |

All API failures use the shared envelope
`{"code":"stable_machine_code","error":"human-readable detail"}`. Clients
should branch on `code`; the `error` text is diagnostic context and is not a
stable programmatic interface.
| `500` | Decode, filesystem, tag serialization task, or export failure. |

Frame decode errors are returned for the request that failed; the server remains
running.
