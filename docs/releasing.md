# Releasing `dcmview`

Release automation spans two GitHub Actions workflows and one Azure pipeline:

- `.github/workflows/ci.yml` runs frontend, Rust, Python, packaging, and VS Code
  checks on Linux, with Rust coverage on macOS and Windows
- `.github/workflows/release.yml` builds tagged release artifacts for Linux,
  macOS Intel, macOS Apple Silicon, and Windows x64, then publishes approved
  releases to PyPI, Homebrew, and Open VSX when configured
- `azure-pipelines/vscode-marketplace.yml` publishes VS Code Marketplace
  packages from GitHub Release assets

## Release channels

- **GitHub Releases** are the canonical binary artifacts
- **PyPI wheels** are the preferred Python install path when `PUBLISH_PYPI=1`
- **VSIX files** are target-specific editor packages attached to GitHub Releases,
  published to the VS Code Marketplace by Azure Pipelines, and published to Open
  VSX for Cursor by GitHub Actions
- **Homebrew** formula generation is always part of the release job; publication
  to a separate tap is conditional on `HOMEBREW_TAP_REPOSITORY`

## Release toolchain

Local release checks and CI require Rust 1.88+, Node.js 20.19+ with npm, and
Python 3.9+. Packaging jobs use Python 3.11. CI pins Rust 1.88 for compatibility
checks, while tagged native builds and the manylinux container install the
current stable Rust toolchain.

## Required repository configuration

Optional release publishing is gated behind repository settings:

- `PUBLISH_PYPI=1` enables the PyPI publish job
- `PUBLISH_OPEN_VSX=1` enables the Open VSX publish job; this must be a
  repository-level Actions variable because the job condition is evaluated
  before GitHub declares its environment
- `HOMEBREW_TAP_REPOSITORY` points to the separate tap repo, for example `your-org/homebrew-tap`
- `HOMEBREW_TAP_TOKEN` is a token with push access to the tap repo

For PyPI, prefer GitHub trusted publishing on the `pypi` environment. The workflow already requests `id-token: write`.

VS Code Marketplace publishing is handled in Azure DevOps:

- Azure DevOps organization: `beatricebm`
- Azure DevOps project: `dcmview`
- Visual Studio Marketplace publisher: `beatricebm`
- Service connection: `dcmview-marketplace-publisher`
- Approval environment: `vscode-marketplace`

The Azure pipeline uses Microsoft Entra ID with workload identity federation and
publishes only VSIX assets that already exist on the GitHub Release.

Open VSX publishing is handled by the `publish-open-vsx` job in GitHub Actions:

- Open VSX namespace: `beatricebm`
- GitHub environment: `open-vsx`
- Environment secret: `OPEN_VSX_PAT`
- Repository variable: `PUBLISH_OPEN_VSX=1`

The environment should require maintainer approval. After approval, the job
downloads the target-specific `vscode-vsix` artifact from the same workflow run
and publishes each platform package with the pinned `ovsx` CLI. The workflow
passes `OPEN_VSX_PAT` to the CLI as `OVSX_PAT` and uses `--skip-duplicate` so a
partially completed release can be retried safely. Generate a dedicated CI token
from the Open VSX account settings, store it only in the protected environment,
and rotate or revoke it if its exposure is suspected.

## Homebrew tap publication checklist

Every tagged release renders a dual-architecture macOS formula and attaches it
to the GitHub Release. The separate tap publication job runs only when
`HOMEBREW_TAP_REPOSITORY` is configured. Do not add public install commands to
the README or release notes until the named tap exists and has published a
working formula.

Before the first release intended for a public tap:

- Create or select the Homebrew tap repository that will receive
  `Formula/dcmview.rb`.
- Set the repository variable `HOMEBREW_TAP_REPOSITORY` to the tap repository in
  `owner/repo` form.
- Add the repository secret `HOMEBREW_TAP_TOKEN` with push access to the tap
  repository.
- Confirm the tap repository accepts commits from GitHub Actions and does not
  require branch protection rules that the release workflow cannot satisfy.
- Confirm the generated formula artifact from a dry run or previous release
  contains both macOS archive URLs and SHA-256 checksums.

During that release:

- Verify the `Render Homebrew formula` step uploads the `homebrew-formula`
  artifact.
- Verify the `publish-homebrew-tap` job runs when `HOMEBREW_TAP_REPOSITORY` is
  set and commits `Formula/dcmview.rb` to the tap.
- Run `brew audit --strict --online dcmview` and `brew test dcmview` from the
  tap repository after publication.
- Add public Homebrew install commands only after the tap contains a working
  formula for the tagged release.

## Standard release flow

1. Regenerate fixtures if they changed:
   `cargo run --locked --example generate_test_fixtures`
2. Run the CI-aligned core profile:
   `python scripts/check.py core --install`
3. Where the host can launch the VS Code test runtime, run the end-to-end
   profile:
   `python scripts/check.py e2e --install`
   On headless Linux, prefix that command with `xvfb-run -a`.
4. Run network-backed, feature-gated remote fixtures separately when needed:
   `python scripts/check.py external --install`
5. Tag the exact version declared in `Cargo.toml`, `pyproject.toml`, and
   `vscode/package.json`:
   `VERSION="$(python scripts/check_versions.py --print-version)"`
   `git tag "v${VERSION}"`
   `git push origin "v${VERSION}"`

`core` checks version parity, generated frontend contracts, frontend
types/tests/build, Rust formatting and strict Clippy, deterministic fixture
freshness, the locked Rust suite, Python unit/package-helper tests, and VS Code
compilation. `e2e` runs `core` and then adds real-binary Python integration,
release smoke coverage, and VS Code integration. Individual profiles such as
`frontend`, `rust`, `python-unit`, `python-integration`, `vscode`, and
`vscode-integration` are available for targeted iteration.

The release workflow will:

- build `dcmview` on Ubuntu 22.04, macOS Intel, macOS Apple Silicon, and
  Windows x64
- fail before release builds if the pushed tag does not match the checked-in package versions
- build the Linux PyPI wheel inside a `manylinux_2_28_x86_64` container so the published wheel is PyPI-compatible
- smoke test each built binary against the committed fixture corpus
- validate the Linux release artifact on Ubuntu 22.04 and Ubuntu 24.04
- validate the Windows zip artifact on Windows latest
- build bundled `dcmview-py` wheels
- package target-specific VSIX artifacts for Linux x64, macOS x64, macOS
  arm64, and Windows x64
- publish release tarballs, the Windows zip, checksums, and wheels to GitHub
  Releases
- publish the VSIX artifacts to GitHub Releases
- render `packaging/homebrew/dcmview.rb`
- optionally publish to PyPI, Open VSX, and the configured tap repo
- trigger the Azure pipeline, which waits for the GitHub Release VSIX assets and
  publishes them to the VS Code Marketplace after `vscode-marketplace` approval

## Editor marketplace packages

The VSIX packaging job downloads the same platform archives produced by the
release build matrix and runs:

```bash
npm --prefix vscode ci
npm --prefix vscode run package:release
```

`package:release` builds these target-specific VSIX artifacts:

- `dist/dcmview-<version>-linux-x64.vsix`
- `dist/dcmview-<version>-darwin-x64.vsix`
- `dist/dcmview-<version>-darwin-arm64.vsix`
- `dist/dcmview-<version>-win32-x64.vsix`

Each package contains exactly one bundled binary at
`vscode/resources/bin/<target>/dcmview`, except Windows x64, which contains
`vscode/resources/bin/win32-x64/dcmview.exe`. `dcmview.binaryPath` remains the
override for unsupported platforms, local debug binaries, and troubleshooting
bundled-binary issues.

The Azure Marketplace pipeline is tag-triggered, but the publish deployment is
bound to the `vscode-marketplace` environment. Its approval check provides the
final manual gate without requiring a separate manually triggered release flow.

The GitHub Open VSX job runs only when the repository-level
`PUBLISH_OPEN_VSX` variable equals `1`. It is independently bound to the
`open-vsx` environment, so its required-review rule gates publication without
coupling Cursor availability to the Azure deployment. After a successful first
publication, confirm that Open VSX lists all four target platforms and that
Cursor can find `beatricebm.dcmview`; Cursor's additional security scan may
delay marketplace visibility.
