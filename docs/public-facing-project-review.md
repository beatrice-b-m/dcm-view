# Public-Facing Project Review

> **Archival note:** This dated review is a point-in-time, non-normative record.
> Installation, security, support, and release documentation has changed since
> the findings were written. Use the current [README](../README.md) and
> [documentation index](index.md) for present behavior.

Date: 2026-06-14

## Scope

This review evaluates `dcmview` from the perspective of a new public user,
potential contributor, or package consumer. It covers the README, package
metadata, CLI help, Python wrapper, VS Code extension documentation, release
automation, repository hygiene, configuration surfaces, and user support
material.

The review did not attempt to validate medical imaging correctness or benchmark
runtime performance. It focuses on whether users can understand, install,
configure, trust, and troubleshoot the project.

## Executive Summary

`dcmview` has a strong product premise and a much more complete technical README
than many early public projects. The core workflow is clear: launch a temporary
local DICOM viewer, inspect frames/tags/annotations, and shut it down. The
README also does well at explaining why the tool exists, warning that it is not
for clinical diagnosis, and documenting the local HTTP API.

The largest public-facing gaps are around trust and self-service onboarding. The
install story is inconsistent with the actual release automation, the security
and PHI exposure model is under-documented for an unauthenticated DICOM server,
CLI help is too terse, configuration is fragmented across code and extension
docs, and the repository lacks common public project files such as
`CONTRIBUTING.md`, `SECURITY.md`, issue templates, and a top-level changelog.

Recommended priority:

| Priority | Theme | Outcome |
|---|---|---|
| P0 | Install and trust | Users know exactly which install channel supports their platform, what data is exposed, and how to report security issues. |
| P1 | Self-service use | `--help`, README, and troubleshooting docs answer common setup, port, codec, browser, and remote workflow problems. |
| P2 | Public polish | Screenshots, examples, docs structure, changelog, metadata, and contributor docs make the project look mature and maintainable. |

## What Works Well

| Area | Assessment |
|---|---|
| Product positioning | The README states a concrete problem and target workflow, especially remote-server DICOM inspection. See `README.md:5`, `README.md:10`, and `README.md:19`. |
| Quick start | Basic file, directory, and remote `--no-browser` examples are easy to follow. See `README.md:61` through `README.md:88`. |
| Safety baseline | The README says the tool is not for clinical diagnosis and notes unauthenticated loopback binding. See `README.md:19` and `README.md:117`. |
| Feature coverage | Viewer features, annotations, shortcuts, CLI options, and HTTP API endpoints are documented in one place. See `README.md:153`, `README.md:185`, `README.md:218`, and `README.md:254`. |
| Release automation | CI and release workflows cover tests, fixtures, platform archives, wheels, VSIX packages, checksums, smoke tests, and optional publishing. See `.github/workflows/ci.yml:27` and `.github/workflows/release.yml:24`. |
| VS Code extension docs | The extension README explains supported platforms, context-menu usage, custom editors, terminal interception, and settings. See `vscode/README.md:14`, `vscode/README.md:26`, and `vscode/README.md:47`. |

## Prioritized Findings

### 1. Install Documentation Conflicts With Release Reality

Severity: High

Evidence:

| Source | Observation |
|---|---|
| `README.md:36` | Says the Python package bundles the binary on supported Linux platforms. |
| `README.md:46` | Sends macOS users to Homebrew or GitHub Releases. |
| `.github/workflows/release.yml:30` | Builds Linux release artifacts and manylinux wheels. |
| `.github/workflows/release.yml:37` | Builds macOS x86_64 wheels. |
| `.github/workflows/release.yml:44` | Builds macOS arm64 wheels. |
| `.github/workflows/release.yml:51` | Builds Windows x64 wheels. |
| `pyproject.toml:16` | Uses `Operating System :: OS Independent`, which is misleading for platform-bundled wheels. |

Impact:

Users may choose the wrong install path, assume macOS and Windows are not
supported by `pip`, or assume the Python package is pure Python when it actually
depends on a bundled or separately resolved native binary. This creates avoidable
first-run failures and support questions.

Concrete improvements:

| Action | Detail |
|---|---|
| Add an install matrix | Show Linux x64, macOS Intel, macOS Apple Silicon, Windows x64, unsupported platforms, and source builds. |
| Provide exact commands | Include `python -m pip install dcmview-py`, Homebrew tap/install commands, GitHub Release archive commands, and source build commands. |
| Clarify Python package behavior | Explain when the wheel includes a binary, when `DCMVIEW_BINARY` is needed, and how PATH fallback works. |
| Fix PyPI classifiers | Replace `Operating System :: OS Independent` with concrete OS classifiers for supported wheels. |
| Add project URLs | Add `Homepage`, `Repository`, `Issues`, and `Changelog` URLs under `[project.urls]` in `pyproject.toml`. |

### 2. Security, Privacy, and PHI Exposure Need a Public Policy

Severity: High

Evidence:

| Source | Observation |
|---|---|
| `README.md:117` | Notes the HTTP server is unauthenticated. |
| `src/server.rs:232` | Prints the running URL. |
| `src/server.rs:233` | Warns on non-loopback binds. |
| `src/types.rs:63` | API file summaries include path, patient ID, patient name, study UID, series UID, and SOP instance UID. |
| Repository root | No `SECURITY.md` was found. |

Impact:

The project handles DICOM data that commonly contains PHI. A new user needs a
clear statement of what data is exposed over the local HTTP API, what risk is
created by non-loopback binding, how SSH forwarding should be used safely on
shared systems, and how to report vulnerabilities.

Concrete improvements:

| Action | Detail |
|---|---|
| Add `SECURITY.md` | Include supported versions, vulnerability reporting path, expected response policy, and a note that the tool is unauthenticated by design. |
| Add README privacy section | Explicitly state that image pixels, tags, file paths, patient identifiers, and annotations may be exposed to anyone who can reach the bound host/port. |
| Expand remote guidance | Include safe shared-server defaults, SSH forwarding examples, and warnings against `--host 0.0.0.0` unless protected by network controls. |
| Document VS Code bridge trust | Summarize bridge token/registry behavior for users, with a link to deeper extension docs. |
| Add a release checklist item | Require security/privacy docs review when changing server binding, bridge behavior, metadata fields, or exported annotations. |

### 3. CLI Help Is Too Terse For a Public Command

Severity: Medium-High

Evidence:

| Source | Observation |
|---|---|
| `src/main.rs:25` | Clap has only a short command `about`. |
| `src/main.rs:31` | CLI arguments mostly lack `help`, `value_name`, and long descriptions. |
| `target/debug/dcmview --help` | Prints option names and defaults but no explanation for most flags. |
| `src/main.rs:61` | `--startup-json` is shown in help even though it is primarily an integration flag. |

Impact:

Users who install a CLI commonly run `dcmview --help` before reading the full
README. Current help does not explain what a path can be, why port `0` matters,
what `--tunnel` does, how filters are formed, or when to use annotations.

Concrete improvements:

| Action | Detail |
|---|---|
| Add help text to every argument | Give each flag one sentence explaining behavior and defaults. |
| Add long help examples | Use Clap `after_long_help` or `after_help` for quick-start examples and remote SSH examples. |
| Hide or label integration flags | Hide `--startup-json` or document it as "for integrations; prints a machine-readable startup event." |
| Improve path display | Ensure help shows `<PATH>...` as required in the common CLI path, not just through parse-time errors. |
| Mirror help in Python CLI | Add descriptions in `python/dcmview_py/__main__.py` so `python -m dcmview_py --help` is similarly self-service. |

### 4. Troubleshooting and Limitations Are Not Yet User-Centered

Severity: Medium-High

Evidence:

| Source | Observation |
|---|---|
| `README.md:331` | Transfer syntax behavior is documented, including unsupported JPEG-LS and RLE. |
| `README.md:339` | Shows unsupported syntaxes return HTTP 422. |
| `src/loader.rs:227` | Zero valid files is a CLI error. |
| `src/loader.rs:292` | Discovery requires the `DICM` preamble. |
| `src/server.rs:246` | Tunnel failure prints a manual SSH fallback. |
| Repository root | No troubleshooting guide was found. |

Impact:

Real users will hit predictable problems: no DICOM files found, extensionless or
non-preamble files skipped, unsupported transfer syntaxes, port conflicts,
browser launch failures, SSH forwarding confusion, missing Node/npm during source
builds, and missing bundled binaries in Python environments. These currently
require reading code, logs, or scattered docs.

Concrete improvements:

| Action | Detail |
|---|---|
| Add `docs/troubleshooting.md` | Use symptom, likely cause, and fix columns. |
| Link it from README | Put it near Quick Start and Install, not only in developer docs. |
| Include common errors | Cover `no valid DICOM files found`, port in use, unsupported transfer syntax, missing frontend build, browser open failure, tunnel failure, and `dcmview binary not found`. |
| Explain supported DICOM assumptions | Document the preamble requirement, skipped non-DICOM files, no-pixel objects, and supported compressed syntaxes in user language. |
| Add smoke-test command | Show users how to verify an install using committed fixtures or a downloaded tiny fixture if running from source. |

### 5. Configuration Reference Is Fragmented

Severity: Medium

Evidence:

| Source | Observation |
|---|---|
| `README.md:218` | CLI options are documented. |
| `build.rs:17` | Build-time env vars include `DCMVIEW_NODE_PATH`, `DCMVIEW_NPM_PATH`, and `DCMVIEW_SKIP_FRONTEND_BUILD`. |
| `python/dcmview_py/wrapper.py:21` | Runtime env vars include `DCMVIEW_BINARY` and several `DCMVIEW_VSCODE_*` variables. |
| `vscode/README.md:47` | VS Code settings are documented separately. |
| `docs/vscode-extension-local-testing.md:51` | Bridge bypass/debug env vars are explained in local testing docs, not in a public configuration reference. |

Impact:

Users and administrators have to infer configuration from multiple places. This
is especially costly for remote servers, VS Code Remote-SSH, notebooks, and
unsupported platforms that need explicit binary resolution.

Concrete improvements:

| Action | Detail |
|---|---|
| Add `docs/configuration.md` | Centralize CLI flags, Python parameters, VS Code settings, and environment variables. |
| Classify variables | Separate user-facing runtime variables from development/build variables. |
| Document precedence | Explain binary resolution order for Python and VS Code, including `DCMVIEW_BINARY`, bundled binary, and PATH. |
| Include examples | Show common configurations for remote SSH, no-browser mode, custom binary path, and source build with custom Node/npm paths. |
| Link from README and VS Code README | Make configuration discoverable from both entry points. |

### 6. Documentation Structure Mixes User Guide, API Reference, and Internal Plans

Severity: Medium

Evidence:

| Source | Observation |
|---|---|
| `README.md:254` | The root README contains a detailed HTTP API reference. |
| `README.md:382` | Development and architecture details are in the same document as install and quick start. |
| `docs/` | Contains feasibility plans, remediation plans, release docs, contracts, and extension testing docs without an index. |
| Repository root | No `docs/index.md` was found. |

Impact:

The README is useful but dense. Public users need a short path to install,
launch, inspect, annotate, and troubleshoot. Contributors need a separate path
to architecture, development commands, fixtures, release procedures, and internal
design plans.

Concrete improvements:

| Action | Detail |
|---|---|
| Keep README task-focused | Limit root README to value proposition, screenshot, install matrix, quick start, remote workflow, safety note, and links. |
| Move reference details | Move HTTP API, annotations schema, transfer syntax matrix, and development architecture to dedicated docs. |
| Add docs index | Create `docs/index.md` with "Users", "Python", "VS Code", "API", "Development", "Release", and "Internal plans" sections. |
| Mark internal docs | Move feasibility/remediation plans under `docs/internal/` or label them clearly as design notes. |
| Add stable anchors | Use predictable docs names that can be linked from PyPI, VS Code Marketplace, and GitHub Releases. |

### 7. Public Governance Files Are Missing

Severity: Medium

Evidence:

| Source | Observation |
|---|---|
| Repository root | No top-level `CONTRIBUTING.md` was found. |
| Repository root | No top-level `SECURITY.md` was found. |
| Repository root | No top-level `CHANGELOG.md` was found. |
| `.github/` | Only workflows were found; no issue or pull request templates. |
| `vscode/CHANGELOG.md:1` | A changelog exists only for the VS Code extension. |

Impact:

Public users and contributors lack clear expectations for bug reports, feature
requests, security disclosure, development setup, test requirements, release
history, and clinical-safety boundaries.

Concrete improvements:

| Action | Detail |
|---|---|
| Add `CONTRIBUTING.md` | Include setup, test commands, fixture policy, coding style, commit expectations, and scope boundaries. |
| Add `SECURITY.md` | See finding 2. |
| Add top-level `CHANGELOG.md` | Track user-visible changes across CLI, Python, VS Code, and API. |
| Add issue templates | Provide bug report, installation problem, DICOM compatibility issue, and feature request templates. |
| Add PR template | Require tests, docs updates, platform impact, and security/privacy consideration. |

### 8. Python API Documentation Is Too Thin Relative To Its Surface Area

Severity: Medium

Evidence:

| Source | Observation |
|---|---|
| `README.md:121` | Python usage shows basic blocking and non-blocking examples. |
| `python/dcmview_py/wrapper.py:164` | `view()` accepts port, host, browser, tunnel, recursive, timeout, annotations, filters, and `vscode_bridge`. |
| `python/dcmview_py/wrapper.py:746` | Binary resolution uses `DCMVIEW_BINARY`, bundled binary, and PATH. |
| `python/dcmview_py/__main__.py:28` | The Python module CLI has minimal parser help text. |

Impact:

Notebook and scripting users are a core audience, but the public docs do not
fully explain parameters, return values, exceptions, lifecycle behavior,
VS Code bridge behavior, or binary resolution.

Concrete improvements:

| Action | Detail |
|---|---|
| Expand Python docs | Add `docs/python.md` with a full `view()` parameter table and lifecycle examples. |
| Improve docstrings | Give `view()` a complete docstring that can be surfaced by `help(dcmview_py.view)`. |
| Document exceptions | Explain `ValueError`, `TypeError`, `RuntimeError`, and `subprocess.CalledProcessError` cases. |
| Explain VS Code bridge | Document `vscode_bridge=True`, how to bypass it, and how handles behave when the session is VS Code-managed. |
| Link from PyPI | Use `[project.urls]` and README anchors to route PyPI users to Python-specific guidance. |

### 9. API Reference Examples Are Slightly Stale

Severity: Medium-Low

Evidence:

| Source | Observation |
|---|---|
| `src/types.rs:114` | `FilesResponse` includes `scan_complete`, `scanned`, `skipped`, and `filtered`. |
| `src/server.rs:344` | `/api/files` returns those progressive scan fields. |
| `README.md:285` | The `/api/files` example omits those fields. |

Impact:

Scripts using `/api/files` may miss useful progressive discovery status. The
omission also hides an important large-directory usability feature.

Concrete improvements:

| Action | Detail |
|---|---|
| Update README sample | Include `scan_complete`, `scanned`, `skipped`, and `filtered`. |
| Add API schema docs | Link generated frontend types or add JSON schema examples for stable public endpoints. |
| Document polling pattern | Show how scripts should poll `/api/files` until `scan_complete` or expected files appear. |
| Add versioning note | State whether the HTTP API is stable, best-effort, or internal-to-viewer. |

### 10. Package Metadata Is Too Sparse For Public Discovery

Severity: Medium-Low

Evidence:

| Source | Observation |
|---|---|
| `Cargo.toml:1` | Cargo package metadata includes name, version, edition, and build script but no description, license, repository, homepage, readme, keywords, or categories. |
| `pyproject.toml:5` | Python metadata has name, version, description, readme, author, license, and classifiers but no project URLs. |
| `vscode/package.json:19` | VS Code keywords are present. |
| `vscode/package.json:33` | VS Code category is only `Other`. |

Impact:

Package indexes, release pages, and downstream tools have less context than they
could. Sparse metadata also makes the project look less mature than its
implementation actually is.

Concrete improvements:

| Action | Detail |
|---|---|
| Enrich Cargo metadata | Add `description`, `license`, `repository`, `homepage`, `readme`, `keywords`, and `categories`. |
| Enrich PyPI metadata | Add `[project.urls]`, OS classifiers, intended audience classifiers, and topic classifiers. |
| Review VS Code category | Consider a more discoverable category if appropriate, while keeping keywords. |
| Keep version sync | Continue using `scripts/check_versions.py` to enforce shared version numbers. |

### 11. Visual Onboarding Is Minimal

Severity: Low

Evidence:

| Source | Observation |
|---|---|
| `README.md:1` | The README starts with a wordmark. |
| Repository assets | Logo and wordmark assets exist. |
| Repository docs | No screenshot or animated walkthrough was found. |

Impact:

For a viewer, a screenshot quickly communicates capability, UI maturity, and the
annotation workflow. Without one, users must install and run the project before
they know what experience to expect.

Concrete improvements:

| Action | Detail |
|---|---|
| Add a screenshot | Show image viewport, file list, tags, and ROI tools using synthetic fixtures. |
| Add a short GIF or video | Demonstrate remote start, frame scroll, window/level, and ROI export. |
| Use non-PHI fixtures | Generate visuals from committed synthetic fixtures only. |
| Reuse assets across channels | Include the same screenshot in README, PyPI, VS Code Marketplace, and GitHub Releases. |

### 12. Release Notes Are Fragmented

Severity: Low

Evidence:

| Source | Observation |
|---|---|
| `vscode/CHANGELOG.md:1` | VS Code extension changes are tracked. |
| Repository root | No top-level `CHANGELOG.md` was found. |
| `docs/releasing.md:40` | Release flow is documented for maintainers. |

Impact:

Users installing the CLI or Python package do not have a clear history of
breaking changes, new supported platforms, codec support changes, annotation
schema changes, or security-relevant updates.

Concrete improvements:

| Action | Detail |
|---|---|
| Add top-level changelog | Track changes across all public surfaces. |
| Keep extension changelog focused | Either mirror extension changes into the root changelog or make it clear that `vscode/CHANGELOG.md` is Marketplace-specific. |
| Link release notes | Add changelog links to README, PyPI metadata, and GitHub Release descriptions. |

## Suggested Documentation Structure

| Path | Purpose |
|---|---|
| `README.md` | Short public landing page: problem, screenshot, install matrix, quick start, remote workflow, safety note, links. |
| `docs/index.md` | Public documentation table of contents. |
| `docs/install.md` | Detailed platform/install matrix, source builds, Homebrew, GitHub Releases, Python wheels. |
| `docs/user-guide.md` | Viewer controls, frame navigation, tags, windowing, annotations, export. |
| `docs/remote-workflows.md` | SSH forwarding, `--no-browser`, VS Code Remote-SSH, notebook workflows. |
| `docs/configuration.md` | CLI flags, Python parameters, VS Code settings, environment variables, precedence rules. |
| `docs/troubleshooting.md` | Symptom/cause/fix guide. |
| `docs/api.md` | HTTP API reference, schemas, examples, stability policy. |
| `docs/python.md` | Python wrapper reference. |
| `docs/development.md` | Contributor setup, build, tests, fixtures, architecture summary. |
| `docs/releasing.md` | Maintainer release process. |
| `docs/internal/` | Feasibility notes, remediation plans, and design investigations. |

## Suggested First Remediation Pass

This is the smallest coherent set of changes that would materially improve the
project for public users:

| Step | Change |
|---|---|
| 1 | Update README install section with an explicit platform/channel matrix and exact commands. |
| 2 | Add `SECURITY.md` and a README privacy/security section focused on unauthenticated DICOM/PHI exposure. |
| 3 | Add help text to Rust and Python CLIs; hide or clearly label `--startup-json`. |
| 4 | Add `docs/troubleshooting.md` and link it from README. |
| 5 | Add `[project.urls]`, better classifiers, and Cargo package metadata. |
| 6 | Add top-level `CONTRIBUTING.md`, `CHANGELOG.md`, issue templates, and a PR template. |
| 7 | Add a synthetic-fixture screenshot to the README and VS Code Marketplace README. |

## Review Notes

The project already has strong implementation and testing signals. The main
public-facing work is not to explain more internals, but to reduce uncertainty:
which install path to use, what happens to sensitive DICOM data, how to recover
from common failures, which APIs are stable, and how users should report issues
or contribute fixes.
