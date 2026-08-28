# Changelog

All notable user-visible changes to `dcmview` are tracked here. The VS Code
extension also keeps Marketplace-focused notes in
[`vscode/CHANGELOG.md`](vscode/CHANGELOG.md); extension changes that affect the
overall product should be summarized in both places.

`dcmview` is a research and development inspection tool, not a clinical
diagnostic viewer.

## Unreleased

### Viewer Reliability

- Prevented active-file cleanup from recursively updating ROI selection during
  logical stack source changes, and preserved later ROI selection and editing.
- Restored frame slider, keyboard, scroll, and cine navigation for multiframe
  files that use per-file fallback instead of a catalog-backed logical stack.
- Kept files without complete Study and Series Instance UIDs independent rather
  than merging unrelated objects into one logical navigation sequence.

## 0.2.9 - 2026-08-26

### Viewer Reliability

- Matched annotation CSV paths through normalized absolute keys, including
  relative paths, parent components, and symlink aliases, while moving CSV
  ingestion behind server startup and keeping unmatched large datasets cheap.
- Made Study and Directory explorer presentation deterministic without changing
  progressive file indices or adding CLI sorting controls.
- Replaced interval-driven cine playback with render-paced Loop and Sweep
  scheduling, shared in-flight frame work, decoded-frame prefetching, and
  bounded active-stack retention.
- Kept Explorer and Tags available in narrow browser and VS Code webview layouts
  through accessible overlay drawers with Escape/backdrop dismissal and focus
  restoration.
- Repaired Marketplace documentation links and added packaged-VSIX README
  verification so repository-relative links cannot recur in release artifacts.

## 0.2.7 - 2026-07-28

### CLI

- Expanded Rust CLI help with clearer option descriptions, value names, and
  examples for single-file viewing, recursive directory scans, remote
  `--no-browser` use with SSH forwarding, annotation CSV loading, and filters.
- Expanded `python -m dcmview_py --help` with matching option descriptions and
  examples.
- Hid the integration-only `--startup-json` flag from normal user-facing help.

### Python

- Added a Python wrapper reference covering `view()` parameters, blocking and
  non-blocking usage, context-manager behavior, handle lifecycle, exceptions,
  binary resolution, VS Code bridge behavior, and bypass options.
- Expanded the public `dcmview_py.view()` docstring so `help(dcmview_py.view)`
  is useful in scripts, notebooks, and interactive Python sessions.

### VS Code

- Linked the VS Code README to the shared troubleshooting, configuration,
  Python, and documentation index pages.
- Documented VS Code settings, binary resolution, bridge environment variables,
  and bridge bypass/debug behavior in the shared configuration reference.
- See [`vscode/CHANGELOG.md`](vscode/CHANGELOG.md) for Marketplace-specific
  extension release notes.

### API And Debugging

- Added a dedicated internal HTTP API reference for debugging and test
  automation, including progressive scan fields, polling guidance, cache
  headers, raw-frame metadata headers, annotation behavior, and error semantics.
- Clarified that the viewer HTTP API is internal to the local viewer and should
  not be treated as a stable external integration contract.
- Documented the debugging-only `debug-api` Cargo feature and its permissive
  CORS behavior.

### Documentation And Packaging

- Added troubleshooting, configuration, Python wrapper, annotation, development,
  and documentation index references.
- Tightened the README into a shorter public landing page while preserving
  install guidance, quick start workflows, remote usage, Python and VS Code
  pointers, safety notes, troubleshooting links, and issue-reporting guidance.
- Added contributor guidance for setup, tests, fixture policy, documentation
  expectations, pull requests, and no-PHI reporting rules.
- Clarified that Homebrew distribution is planned but not yet configured; public
  install guidance continues to point to PyPI, VS Code Marketplace, GitHub
  Releases, and source builds.
