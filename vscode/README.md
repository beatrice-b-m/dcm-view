![dcmview](https://raw.githubusercontent.com/beatrice-b-m/dcmview/main/dcmview-wordmark-darkmode-opaque-background.png)

# dcmview

Open local DICOM files and folders in `dcmview` directly from VS Code.

`dcmview` is a fast, temporary DICOM inspection tool for research and
development workflows. The extension starts a local loopback `dcmview` server
from the selected file or folder and displays the viewer in a VS Code webview.

`dcmview` is intended for developer and research inspection on secure networks,
not clinical diagnosis. Avoid public-facing server binds; the extension launches
the bundled server on loopback and displays it in a VS Code webview.

Do not include PHI or sensitive DICOM content in public issue reports. Report
security issues privately to the maintainers before public disclosure.

For common install, binary resolution, VS Code interception, startup, tunnel,
and annotation CSV problems, see the main
[troubleshooting guide](../docs/troubleshooting.md).

## Supported Platforms

Marketplace builds currently bundle `dcmview` binaries for:

- Linux x64
- macOS x64
- macOS arm64
- Windows x64

On unsupported platforms, or when you need to test a locally built binary, set
`dcmview.binaryPath` to an absolute path to a compatible `dcmview` executable.

## Usage

Use the Explorer context menu command `Open with dcmview` on DICOM files or
folders. The extension launches `dcmview --no-browser --port 0`, waits for the
local server URL, and opens the viewer beside your current editor.

For files named `*.dcm`, `*.dicom`, or `*.ima`, use VS Code's `Reopen With...`
command and choose `dcmview` to open the file in a readonly dcmview editor tab.
Set `dcmview` as the default editor for those patterns if you want double-clicks
to open matching DICOM files directly in dcmview. Extensionless DICOM files and
folders should still use the Explorer context menu command.

The command `dcmview: Open Workspace with dcmview` opens a selected workspace
folder. The command `dcmview: Stop All dcmview Sessions` terminates extension
managed viewer sessions.

When `dcmview.terminalInterception.enabled` is true, new integrated terminals
route `dcmview`, `dcmview-py`, and `python -m dcmview_py` invocations into VS
Code webview panels. Set `DCMVIEW_VSCODE_BYPASS=1` in a terminal to bypass that
integration for a single shell session.

## Settings

- `dcmview.binaryPath`: absolute path to a `dcmview` binary override.
- `dcmview.defaultRecursive`: recursively scan selected folders by default.
- `dcmview.extraArgs`: additional command-line arguments passed to `dcmview`.
- `dcmview.startupTimeoutSeconds`: seconds to wait for startup.
- `dcmview.terminalInterception.enabled`: route integrated terminal launches
  into VS Code webviews.
