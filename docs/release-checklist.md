# Release Checklist

Use this checklist for every tagged `dcmview` release. The release candidate is
not ready to tag until the exact commit has passed CI. Never move or overwrite a
published tag; if a code defect is found after tagging, prepare a patch release.

The detailed channel configuration and artifact descriptions remain in the
[release process](releasing.md).

## 1. Establish The Release Baseline

- [ ] Fetch current branches and tags without changing the working tree:

  ```bash
  git fetch origin --prune --tags
  ```

- [ ] Identify the latest stable release tag. Treat its commit—not the previous
      ordinary commit—as the release-note baseline:

  ```bash
  LAST_RELEASE="$(git describe --tags --abbrev=0)"
  git show --no-patch --format=fuller "$LAST_RELEASE"
  ```

- [ ] Review every change between that release and the proposed release
      candidate:

  ```bash
  git log --first-parent --stat "$LAST_RELEASE"..HEAD
  git diff --stat "$LAST_RELEASE"..HEAD
  git diff --name-status "$LAST_RELEASE"..HEAD
  ```

- [ ] Inspect merged pull requests, user-visible behavior, API or CLI changes,
      platform support, dependency/security changes, documentation changes,
      deprecations, and known limitations. Do not derive notes from commit
      subjects alone.
- [ ] Confirm the intended version and release scope. Defer unrelated work
      rather than expanding the release candidate late in the process.

## 2. Prepare Every Release-Note Surface

- [ ] Move the relevant entries from `CHANGELOG.md` **Unreleased** into a dated
      version section. Cover CLI, viewer, Python, API, packaging, and
      documentation changes as applicable.
- [ ] Update `vscode/CHANGELOG.md` with Marketplace-facing extension changes.
      Product-wide extension changes should appear in both changelogs.
- [ ] Prepare the GitHub Release notes from the same factual inventory. Include
      highlights, fixes, compatibility or migration information, known
      limitations, install links, and acknowledgements where appropriate.
- [ ] Prepare the corresponding stable-release notes for the `dcmview-docs`
      repository. Do not present the release as current there until GitHub marks
      it as a non-draft, non-prerelease release.
- [ ] Update `README.md`, `vscode/README.md`, API documentation, compatibility
      statements, and other public pages when the release changes their claims.
- [ ] Cross-check all surfaces for the same version, feature names, support
      boundaries, dates, links, and safety language.

## 3. Run The Publication Update Checklist

### Version And Package Metadata

- [ ] Set the release version consistently in `Cargo.toml`, `pyproject.toml`,
      `frontend/package.json`, and `vscode/package.json`.
- [ ] Regenerate the affected Cargo and npm lockfiles; do not hand-edit resolved
      dependency records.
- [ ] Check canonical package-version parity and the proposed tag:

  ```bash
  VERSION="$(python scripts/check_versions.py --print-version)"
  python scripts/check_versions.py --tag "v${VERSION}"
  ```

- [ ] Review package descriptions, supported-platform lists, installation
      commands, publisher identifiers, and release-channel configuration.

### Screenshots, GIFs, And Attribution

- [ ] Determine whether viewer, extension, API, CSS, or capture-plan changes
      make any published screenshot or GIF stale.
- [ ] Recreate affected assets from the release-candidate binary using only the
      approved public, de-identified sources under ignored
      `marketing-source-data/`.
- [ ] Verify each source against its recorded collection, DOI, license,
      Series/SOP identity, and checksum. Keep source payloads and local-path
      linkage untracked.
- [ ] Review every image and animation for identifiers, local paths, hostnames,
      misleading clinical implications, rendering errors, and obsolete UI.
- [ ] Record the dcmview version and commit, source-manifest hash, capture date,
      output hash, dimensions, capture-tool versions, and modification summary.
- [ ] Publish attribution near the asset or through a clearly linked attribution
      page. For CC BY 4.0 material, retain the designated creator/attribution
      party, dataset title when supplied, source/DOI, license link, changes made,
      and a no-endorsement statement.
- [ ] Use this attribution structure, expanding it once per dataset when an
      asset combines multiple sources:

  ```text
  Source imagery: {dataset title}, {creator or designated attribution party},
  {year/version}. {DOI or canonical dataset URL}. Retrieved through {repository}
  where applicable. Licensed under CC BY 4.0:
  https://creativecommons.org/licenses/by/4.0/

  Changes: Displayed using dcmview {version}; {windowed, cropped, resized,
  colorized, annotated, or animated as applicable} for demonstration.

  Provenance: Captured from dcmview commit {full commit SHA} using source
  manifest {SHA-256}. No endorsement by the dataset creators, repository,
  TCIA, IDC, NIH, or NCI is implied.
  ```
- [ ] Update every consuming surface from the same approved asset set: root
      README/PyPI, `vscode/README.md`, VS Code Marketplace, Open VSX, GitHub
      Release notes, and `dcmview-docs`.
- [ ] Ensure Marketplace README image URLs resolve through HTTPS and are pinned
      to the release tag rather than `main`.

Until deterministic capture tooling is committed, the capture itself and the
privacy/aesthetic review are manual release gates. Once a capture manifest and
media lock exist, the release workflow should reject a mismatched UI-input
digest or media hash; human review should still remain required.

### Documentation And Links

- [ ] Check that public examples use the release version and supported commands.
- [ ] Check GitHub, PyPI, VS Code Marketplace, Open VSX, documentation, DOI,
      license, and download links.
- [ ] Confirm new public files are included in source archives, wheels, and VSIX
      packages where intended, and excluded where they are not needed.
- [ ] Confirm no DICOM source payload, credential, local-path linkage file, PHI,
      or sensitive log has become tracked or packaged.

## 4. Build And Verify The Final Release Candidate

- [ ] Regenerate committed fixtures if their generator or expected output
      changed:

  ```bash
  cargo run --locked --example generate_test_fixtures
  ```

- [ ] Run the CI-aligned core profile:

  ```bash
  python scripts/check.py core --install
  ```

- [ ] Run the real-process and VS Code Electron profile on a capable host:

  ```bash
  python scripts/check.py e2e --install
  ```

- [ ] Run the independent network-backed fixture profile when the release
      changes discovery, metadata, codecs, pixels, or upstream compatibility:

  ```bash
  python scripts/check.py external --install
  ```

- [ ] Perform any release-specific manual browser, platform, remote, semantic,
      WSI, annotation, or Marketplace checks that automated profiles do not
      cover.
- [ ] Confirm `git status --short` is clean and review the final diff from the
      previous stable release.
- [ ] Commit each remaining logical change according to the repository commit
      policy.

## 5. Push And Qualify The Release-Candidate Commit

- [ ] Record the exact candidate commit:

  ```bash
  RC_SHA="$(git rev-parse HEAD)"
  git show --no-patch --format=fuller "$RC_SHA"
  ```

- [ ] Push the final release-candidate commit through the normal protected-branch
      or pull-request process so it becomes the intended commit on `main`.
- [ ] Monitor every required job in `.github/workflows/ci.yml` to completion.
- [ ] Resolve code, test, packaging, documentation, and deterministic tooling
      failures in the repository. Commit and push each fix, then treat the new
      `HEAD` as a new release candidate and restart this section.
- [ ] For an external blocker that cannot be fixed from the codebase, give the
      maintainer the failing workflow/job URL, affected release channel,
      relevant error excerpt, actions already attempted, release impact, and a
      concrete recommended resolution. Do not tag while a required gate is
      unresolved.
- [ ] After CI passes, verify the remote branch still points at the candidate:

  ```bash
  git fetch origin main
  test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)"
  ```

## 6. Tag The Passing Commit And Monitor Publication

- [ ] Create an annotated tag on the exact CI-passing commit:

  ```bash
  VERSION="$(python scripts/check_versions.py --print-version)"
  git tag -a "v${VERSION}" "$(git rev-parse HEAD)" -m "dcmview v${VERSION}"
  git show --no-patch "v${VERSION}"
  ```

- [ ] Push only that tag. `main` must already contain the tagged commit:

  ```bash
  VERSION="$(python scripts/check_versions.py --print-version)"
  git push origin "v${VERSION}"
  ```

- [ ] Monitor every required job in `.github/workflows/release.yml`, including
      native builds, archive and wheel smoke tests, VSIX packaging, GitHub
      Release creation, and each enabled PyPI, Open VSX, and Homebrew publisher.
- [ ] Monitor the Azure VS Code Marketplace pipeline and its approval-bound
      deployment.
- [ ] Apply the prepared notes to the GitHub Release and verify its files,
      checksums, version, links, and non-draft/non-prerelease status.
- [ ] Install or download representative published artifacts rather than relying
      only on build-job outputs. Confirm the CLI version and one viewer launch.
- [ ] Verify the live PyPI, VS Code Marketplace, Open VSX, Homebrew, and GitHub
      pages for every enabled channel.

If a workflow fails because of transient infrastructure, credentials, an
approval gate, or an idempotently retryable publisher, preserve the tag and
retry the failed operation after resolving the external condition. If the
tagged code or packaged contents are defective, do not move the tag: fix the
problem on `main` and publish a patch release.

## 7. Synchronize `dcmview-docs` Through A Pull Request

- [ ] Confirm GitHub identifies the new release as the latest stable
      non-draft, non-prerelease release.
- [ ] In `dcmview-docs`, create a release-synchronization branch from current
      `main`.
- [ ] Update every field in `docs-source.json` to the released tag, exact commit,
      and synchronization time in the same pull request.
- [ ] Update release notes and every affected getting-started, guide, concept,
      reference, compatibility, API, and troubleshooting page. Describe only
      behavior present in the recorded release commit.
- [ ] Copy the approved release media and attribution records; do not copy the
      ignored DICOM source directory or its local-path linkage document.
- [ ] Run the documentation repository's required checks:

  ```bash
  npm run format
  npm run validate
  ```

- [ ] Push the branch and open a pull request. Review the Cloudflare preview,
      internal links, media loading, responsive layout, release version, source
      metadata, attribution, and safety language before merge.
- [ ] Merge only after required checks and review pass, then verify the live site.

## 8. Resolve Or Escalate Post-Release Problems

- [ ] Classify each problem as code/package content, release automation,
      credentials/approval, registry availability, documentation, or an
      external service issue.
- [ ] Fix repository-controlled problems with normal commits and tests. Use a
      patch release for changes to already-tagged code or artifacts.
- [ ] For issues outside repository control, clearly flag them to the maintainer
      with the affected channel, severity, evidence, user impact, owner or
      service involved, recommended action, and whether unaffected channels may
      remain available.
- [ ] Record any intentionally deferred issue in release notes or public status
      messaging when users could encounter it.

## 9. Start The Next Development Version

Complete this only after all required publication and documentation checks pass
or every remaining external issue has an explicit maintainer-owned resolution.

- [ ] Choose the next working version. Default to the next patch version unless
      the planned development scope requires a minor or major increment.
- [ ] Update the canonical manifests and regenerate their lockfiles. At minimum,
      keep `Cargo.toml`, `Cargo.lock`, `pyproject.toml`,
      `frontend/package.json`, `frontend/package-lock.json`,
      `vscode/package.json`, and `vscode/package-lock.json` consistent.
- [ ] Leave the released notes under their dated version sections and retain a
      new empty **Unreleased** section in both changelogs.
- [ ] Do not advance `dcmview-docs`; it must continue to describe the latest
      stable release rather than the new development version.
- [ ] Run the package-version check and the appropriate core checks.
- [ ] Commit the version transition separately, for example:

  ```text
  chore(release): begin <next-version> development
  ```

- [ ] Push the version-transition commit to `main` through the normal branch
      policy and verify CI. This commit is the unambiguous starting point for
      the next development cycle.

## Completion Record

Record these values in the release issue, pull request, or maintainer log:

```text
Previous stable tag:
Release tag:
Release commit:
CI workflow URL:
Release workflow URL:
GitHub Release URL:
PyPI result:
VS Code Marketplace result:
Open VSX result:
Homebrew result:
dcmview-docs PR and preview URL:
Media manifest/hash:
Known issues or external follow-ups:
Next development version commit:
```
