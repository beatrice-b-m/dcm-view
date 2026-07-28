# VS Code extension feasibility

> **Archival note:** This is a point-in-time, non-normative study written before
> the current VS Code extension was implemented. It remains design history, not
> current extension documentation. See the
> [VS Code extension README](../vscode/README.md) and
> [current architecture](architecture.md).

## Summary

Building a VS Code extension for `dcmview` is feasible, and the lowest-risk path
is to keep the existing Rust binary and HTTP/Svelte viewer as the rendering
engine. A VS Code extension can launch `dcmview --no-browser --port 0`, parse the
printed server URL, and open a VS Code webview panel that frames the local
server URL.

This approach preserves the current pixel pipeline, annotation API, tag
serialization, cache behavior, and embedded frontend. It also keeps the
extension small: the extension is mostly command registration, path selection,
binary discovery, process lifecycle, and VS Code webview setup.

The main challenge is not local desktop VS Code. The main challenge is remote
VS Code, which is also `dcmview`'s primary workflow. In Remote SSH, Dev
Containers, WSL, and Codespaces, the extension must run where the DICOM files
live and must expose the spawned server to the webview through VS Code's remote
URI mechanisms instead of assuming direct `localhost` access.

## Recommended deployment model

### Phase 1: wrapper extension with webview iframe

Implement a Node/TypeScript VS Code extension with `"extensionKind":
["workspace"]`.

Core behavior:

- Register `dcmview.openPath` command.
- Add file explorer context-menu actions for files and folders.
- Accept one or more selected `vscode.Uri` values, convert them to filesystem
  paths, and spawn the bundled or configured `dcmview` binary.
- Always pass `--no-browser --port 0 --host 127.0.0.1`.
- Parse stdout line `dcmview: server running at http://...`.
- Call `vscode.env.asExternalUri` on the spawned server URL.
- Create a `WebviewPanel` with scripts enabled and restrictive CSP.
- Render a minimal iframe pointed at the external URI.
- On panel dispose or extension deactivate, stop the child process.

This is the best first deployment surface because it does not require changing
the current frontend fetch contract. The frontend already uses origin-relative
API paths such as `/api/files`, `/api/file/:i/frame/:n`, and
`/api/file/:i/frame/:n/raw`, which will continue to work when the full viewer is
served from the same `dcmview` HTTP origin.

### Phase 2: custom readonly editor for DICOM files

After the wrapper works, add a `CustomReadonlyEditorProvider` for common DICOM
filename patterns such as `*.dcm`, `*.dicom`, and optionally extensionless files
through an explicit command. This lets users open a DICOM as a VS Code editor
tab and choose `dcmview` through "Reopen With".

This is useful but should not be the first implementation because many DICOM
datasets are directories or extensionless files, and `dcmview` already supports
multi-file study navigation better than a one-resource editor model.

## Why not rewrite as a native webview app first?

A webview-native design would serve Svelte assets from the extension and route
all DICOM operations through `postMessage` to the extension host instead of HTTP.
That can avoid localhost forwarding concerns, but it would require replacing the
existing HTTP API with a webview-message API or adding a second transport layer.

That rewrite would touch most frontend API calls, binary/raw frame transport,
CSV export, error handling, and cache semantics. It would also move large binary
frame payloads through VS Code webview messaging, which is a worse fit than the
current HTTP response model for PNG and raw sample buffers.

Keep this as a later option only if iframe-based webviews prove unacceptable in
target environments.

## Structural changes needed

### New extension package

Add a top-level extension package, for example `vscode/`:

```text
vscode/
|-- package.json
|-- tsconfig.json
|-- src/extension.ts
|-- media/
`-- resources/bin/<platform>/dcmview
```

The extension manifest should contribute:

- Commands: `dcmview.openPath`, `dcmview.openWorkspaceSelection`,
  `dcmview.stopAll`.
- Explorer context menu entries for files and folders.
- Optional custom editor contribution for `*.dcm` and `*.dicom`.
- Settings for `dcmview.binaryPath`, `dcmview.defaultRecursive`, and optional
  `dcmview.extraArgs`.
- `"extensionKind": ["workspace"]` so the extension runs on the machine with the
  workspace files.

### Binary packaging

The extension needs a binary-resolution policy:

1. User-configured `dcmview.binaryPath`.
2. Bundled platform binary inside the extension.
3. `dcmview` found on `PATH`.

Bundling is the most user-friendly option but requires per-platform assets and
release automation. At minimum, package macOS arm64/x64, Linux x64/aarch64, and
Windows x64 if Windows is supported. Native codec dependencies must be covered
by the existing static/dynamic linking strategy; otherwise installation becomes
fragile.

### CLI/process lifecycle

The current CLI is close to sufficient, but a few extension-friendly additions
would reduce wrapper brittleness:

- Add `--print-json-startup` or `--startup-json` so the extension can parse a
  structured startup event instead of matching stdout text.
- Add `--shutdown-token` or a local-only shutdown endpoint if graceful child
  signal delivery proves unreliable on Windows.
- Consider `--idle-timeout` naming or document that existing `--timeout` is safe
  for extension-managed lifecycle.
- Keep `--no-browser`, `--port 0`, and loopback binding as the default extension
  launch mode.

### Webview integration

For the wrapper approach, the extension webview HTML is intentionally small:

- Call `vscode.env.asExternalUri(serverUri)` before building the iframe URL.
- Use a CSP allowing only the iframe source and the webview source.
- Avoid directly fetching `localhost` from the webview.
- Use `retainContextWhenHidden` sparingly; the current viewer can reload from
  server state, but viewport state may be lost unless the frontend adds webview
  state persistence.

VS Code also supports `portMapping`, but official guidance says that
`asExternalUri` is the better option for remote and browser-based scenarios.

### Frontend changes

No frontend changes are required for the iframe wrapper.

Useful future changes:

- Hide or alter the status bar URL display in VS Code mode, because
  `window.location.origin` may show a forwarded VS Code URI rather than the raw
  `127.0.0.1` server URL.
- Add optional `?vscode=1` mode for tighter chrome, smaller top bar, or theme
  adaptation using VS Code color tokens if the frontend is eventually served as
  true webview content.
- Persist active tab/frame/window state with the webview state API only if the
  iframe wrapper is replaced by a native webview bundle.

### Backend changes

No backend API change is required for the wrapper.

Useful hardening:

- Keep all server routes origin-relative and avoid hard-coded localhost URLs.
- Add a lightweight health endpoint for extension readiness checks.
- Add structured startup output.
- Optionally add CORS controls only if a non-iframe integration fetches the
  backend from a different webview origin. The iframe approach does not need
  CORS because the app and API share the same origin.

## Challenges and risks

- Remote development is the core risk. A webview's `localhost` is not reliably
  the same machine as the workspace extension host. The extension must use
  `asExternalUri` or a supported port-mapping strategy.
- VS Code for the Web without a remote extension host cannot spawn the Rust
  binary or access local DICOM files. This surface is not viable unless a server
  already exists elsewhere and the extension only connects to it.
- Packaging native binaries inside VSIX artifacts increases release complexity.
  Platform-specific VSIX builds may be necessary to avoid shipping every binary
  to every user.
- Extension trust and privacy expectations are higher because the viewer exposes
  unauthenticated DICOM data over a local HTTP server. The extension should bind
  only to loopback, avoid public ports, and clearly show the lifecycle of each
  running server.
- Multi-root and multi-selection workflows require careful process ownership.
  Each webview panel should own exactly one child server unless a deliberate
  shared-server model is introduced.
- Annotation export currently downloads from the browser context. In an iframe
  inside VS Code this should be tested explicitly; if blocked, add an extension
  command that fetches `/api/annotations/export.csv` and writes it through
  `showSaveDialog`.
- Keyboard shortcuts may conflict with VS Code editor shortcuts. The current
  viewer uses single-letter shortcuts and arrow keys; webview focus behavior
  must be tested.

## Feasibility verdict

Feasible with low-to-moderate implementation risk if the first version is a
wrapper extension around the existing binary and server. The minimum viable
extension is mostly TypeScript glue and release packaging, not a rewrite.

The structural changes that materially improve quality are structured startup
output, extension binary packaging, robust process cleanup, and explicit remote
URI handling. A deeper webview-native transport is possible but should be
treated as a separate product direction because it duplicates the current HTTP
contract and weakens the efficient binary-frame transport that already exists.

## References

- VS Code Webview API:
  https://code.visualstudio.com/api/extension-guides/webview
- VS Code Remote Development extension guidance:
  https://code.visualstudio.com/api/advanced-topics/remote-extensions
- VS Code Extension Host guidance:
  https://code.visualstudio.com/api/advanced-topics/extension-host
- VS Code Custom Editor API:
  https://code.visualstudio.com/api/extension-guides/custom-editors
