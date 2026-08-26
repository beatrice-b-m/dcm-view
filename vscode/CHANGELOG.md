# Changelog

## Unreleased

## 0.2.9 - 2026-08-26

- Keep Explorer and Tags accessible in compact and narrow webviews through
  overlay drawers with keyboard dismissal and focus restoration.
- Use stable absolute GitHub documentation links in the Marketplace README and
  verify their final form inside packaged VSIX artifacts.
- Improve cine playback smoothness in constrained webviews by advancing only
  after frames are loaded and rendered.

## 0.2.5 - 2026-06-12

- Improve VS Code bridge reliability for Remote-SSH and notebook workflows by
  publishing the bridge in the per-user state directory, falling back from stale
  env endpoints to registry discovery, and validating optional client-supplied
  `dcmview` binaries from `dcmview-py`.
- Updated wrappers scan legacy registry locations for one release, so update the
  VS Code extension first when rolling this out across shared hosts.

## 0.2.2

- Add Windows 11 x64 release artifacts across GitHub Releases, PyPI wheels, and
  target-specific VSIX packages.
- Bundle and resolve `dcmview.exe` for Windows Python and VS Code installs.
- Add Windows CI and release validation coverage for the committed fixture
  smoke test.

## 0.2.1

- Publish target-specific VSIX packages for Linux x64, macOS x64, and macOS
  arm64.
- Rename the Marketplace extension identity to `beatricebm.dcmview`.
- Document supported Marketplace platforms and the `dcmview.binaryPath`
  fallback for unsupported systems.

## 0.2.0

- Add initial VSIX packaging with bundled Linux x64, macOS x64, and macOS arm64
  binaries.
- Add VS Code commands for opening files, folders, and workspaces in `dcmview`.
- Add integrated terminal interception for `dcmview` and `dcmview-py` commands.
