# Releasing `dcmview`

Release automation is split across two workflows:

- `.github/workflows/ci.yml` runs Linux and Windows tests on pushes and pull requests
- `.github/workflows/release.yml` builds tagged release artifacts for Linux,
  macOS, and Windows x64
- `azure-pipelines/vscode-marketplace.yml` publishes VS Code Marketplace
  packages from GitHub Release assets

## Release channels

- **GitHub Releases** are the canonical binary artifacts
- **PyPI wheels** are the preferred Python install path when `PUBLISH_PYPI=1`
- **VSIX files** are target-specific Marketplace packages attached to GitHub
  Releases and published by Azure Pipelines
- **Homebrew** formula generation is always part of the release job, and tap publication is enabled when `HOMEBREW_TAP_REPOSITORY` is configured

## Required repository configuration

Optional release publishing is gated behind repository settings:

- `PUBLISH_PYPI=1` enables the PyPI publish job
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

## Homebrew v0.2.7 checklist

Homebrew publication is planned for `v0.2.7`, but public install commands should
not be added to the README or release notes until the tap exists and a release
has published a formula successfully.

Before tagging `v0.2.7`:

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

During the `v0.2.7` release:

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
   `cargo run --example generate_test_fixtures`
2. Run the local checks:
   `python scripts/check_versions.py`
   `cargo fmt --all -- --check`
   `cargo test`
   `python -m unittest discover -s python/tests`
   `npm --prefix vscode run compile`
3. Tag the exact version declared in `Cargo.toml`, `pyproject.toml`, and
   `vscode/package.json`:
   `VERSION="$(python scripts/check_versions.py --print-version)"`
   `git tag "v${VERSION}"`
   `git push origin "v${VERSION}"`

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
- optionally publish to PyPI and the configured tap repo
- trigger the Azure pipeline, which waits for the GitHub Release VSIX assets and
  publishes them to the VS Code Marketplace after `vscode-marketplace` approval

## VS Code Marketplace packages

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
