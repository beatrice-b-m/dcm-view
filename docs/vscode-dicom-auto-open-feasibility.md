# VS Code DICOM auto-open feasibility

> **Archival note:** This is a point-in-time, non-normative feasibility study.
> The repository now includes an opt-in readonly DICOM custom editor, but the
> body below remains the original design analysis rather than current behavior
> documentation. See the [VS Code extension README](../vscode/README.md).

## Summary

Automatically opening DICOM files in `dcmview` when double-clicked in VS Code is
feasible with a `CustomReadonlyEditorProvider`. The existing extension already
has the hard parts needed for this path: binary resolution, `dcmview`
process startup, structured startup parsing, remote-safe `asExternalUri`
handling, webview iframe rendering, and session cleanup.

The feature should be implemented as an opt-in custom editor for common DICOM
filename patterns. This lets users make `dcmview` the default editor for
matching files while preserving the current explicit `Open with dcmview`
command for folders, multiple selections, and extensionless DICOMs.

## Recommended approach

Add a custom editor contribution:

```json
"customEditors": [
  {
    "viewType": "dcmview.dicomViewer",
    "displayName": "dcmview",
    "selector": [
      { "filenamePattern": "*.dcm" },
      { "filenamePattern": "*.dicom" },
      { "filenamePattern": "*.ima" }
    ],
    "priority": "option"
  }
]
```

Register `onCustomEditor:dcmview.dicomViewer` and a
`CustomReadonlyEditorProvider`. The provider's `openCustomDocument` can store the
resource URI, and `resolveCustomEditor` can reuse the existing launch flow:

1. Validate that the URI is a local file-system URI.
2. Resolve the configured, bundled, debug, or PATH `dcmview` binary.
3. Launch `dcmview --no-browser --port 0 --host 127.0.0.1 --startup-json <file>`.
4. Convert the reported server URL through `vscode.env.asExternalUri`.
5. Fill the custom editor webview with the same iframe HTML used by command
   launched sessions.
6. Stop the child process when the custom editor webview is disposed.

Use `priority: "option"` initially. That makes the editor available through
`Reopen With...` and lets users configure it as their default, but avoids
surprising every `.dcm` double-click immediately after install. After the feature
has been tested across local and remote workspaces, `priority: "default"` can be
considered if automatic ownership of `.dcm` files is desired.

## Why this fits the current extension

The current command flow already maps one or more file URIs to filesystem paths
and opens a VS Code webview backed by a spawned `dcmview` server. A readonly
custom editor is primarily a different VS Code entry point into the same
workflow, not a rendering rewrite.

This path also keeps DICOM decoding inside the Rust backend and keeps PNG/raw
frame delivery over HTTP. A native webview transport would be unnecessary for
double-click behavior and would duplicate existing backend/frontend contracts.

## Limitations

- VS Code custom editor matching is filename-pattern based. It will cover common
  extensions such as `.dcm`, `.dicom`, and `.ima`, but it will not reliably claim
  extensionless DICOM files. Keep the Explorer context-menu command for those.
- Double-clicking a directory cannot be handled by a custom editor. Folder and
  study workflows should continue using `Open with dcmview`.
- Each custom editor tab should own its own child server unless a later shared
  session model is designed. That is simple and predictable, but opening many
  DICOM tabs at once can spawn many local servers.
- Custom editor tabs do not naturally represent multi-selection workflows. The
  existing command remains the better UI for opening a full study or multiple
  files together.
- In remote VS Code, the implementation must keep using `asExternalUri`; direct
  `localhost` iframe URLs are not reliable.
- Annotation export/download behavior should be smoke-tested inside a custom
  editor tab, because custom-editor webviews can differ from the current
  standalone `WebviewPanel` entry point.

## Implementation size

Expected scope is low-to-moderate:

- Manifest additions for `customEditors` and `onCustomEditor`.
- A small readonly document class.
- A provider that reuses existing launch, webview HTML, and cleanup helpers.
- Refactoring `startSession` or extracting a lower-level helper so command
  panels and custom editor webviews can share process lifecycle code.
- Tests for contribution registration, provider URI validation, and child
  termination on editor disposal.
- Manual QA for `.dcm` double-click, `Reopen With...`, user default editor
  association, remote SSH, and unsupported/extensionless files.

## Verdict

Feasible and worth doing. The safest product shape is to ship it as an optional
custom readonly editor first, then document how users can set `dcmview` as the
default editor for `.dcm`/`.dicom` if they want double-click auto-open behavior.
Automatic default ownership can be revisited after local and remote QA.
