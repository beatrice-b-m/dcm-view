# Marketing Media Workflow

The release media workflow captures the real embedded viewer and VS Code
extension from pinned, public DICOM series. Source DICOM payloads, local file
paths, intermediate frames, and review bundles stay ignored. Only a visually
approved, content-addressed publication bundle is copied into tracked public
surfaces.

Human review remains intentional: automation can enforce identities, checksums,
render completion, expected public subject identifiers, hidden local paths,
fixed capture settings, and drift, but it cannot decide whether an image is
misleading or aesthetically ready to publish.

## One-Time Tool Setup

Use Python 3.9+, Node.js 20.19+, npm, Rust, and the normal dcmview build
prerequisites. Install the pinned retrieval and capture dependencies:

```bash
python -m pip install -r marketing/requirements.txt
npm --prefix marketing ci
npm --prefix marketing run install-browser
```

The VS Code scene downloads the exact editor version recorded in
`marketing/captures.json` on first use. The downloaded editor and Chromium are
tool caches, not repository content.

## Retrieve And Lock Public Sources

`marketing/sources.json` is the tracked source-of-truth for collection,
attribution, DOI, license, public subject identifiers, Series Instance UIDs, and
expected object counts.

```bash
python scripts/marketing_media.py validate
python scripts/marketing_media.py fetch
python scripts/marketing_media.py verify-sources
```

Downloads go to the ignored `marketing-source-data/` directory. Fetch writes
`SOURCE_FILES.json`, which records a SHA-256 and source linkage for every DICOM
object, and `SOURCE_LINKAGE.md`, which provides the human-readable
file-to-dataset mapping requested for later citation. Do not move either file
outside that ignored directory or commit the DICOM payload.

Use `--group 02-chest-ct` on `fetch` or `verify-sources` to work with selected
groups. A full release capture requires every group.

## Capture A Review Bundle

Commit the release-candidate implementation first; capture rejects a dirty
worktree by default so provenance resolves to an exact commit.

```bash
python scripts/marketing_media.py capture
```

The command builds dcmview, compiles the extension when needed, waits for the
real progressive catalog, captures every scene, and writes
`marketing-review/current/`. It does not alter published media.

For focused iteration, select one or more scenes or surfaces:

```bash
python scripts/marketing_media.py capture --scene brain-mr-seg
python scripts/marketing_media.py capture --surface browser
```

`--no-build` may reuse `target/debug/dcmview`. `--allow-dirty` is only for local
experimentation: its bundle is marked dirty and cannot pass verification or be
published. `--skip-source-verification` is likewise a diagnostic escape hatch;
the source inventory is still required to create the lock.

The review bundle contains:

- PNG and GIF outputs from the scene manifest;
- one machine-readable report per scene with the selected SOP/Series identity,
  public subject IDs, visible-text hash, viewport, and tool versions;
- `ATTRIBUTION.md`, including dataset, creator, version, DOI, CC BY 4.0 link,
  changes, and no-endorsement statement; and
- `media-lock.json`, binding the release commit and version to source, capture
  input, inventory, and output hashes.

Review every frame of every PNG/GIF for rendering quality, public identifiers,
local paths or hostnames, unintended UI, clinical overclaiming, and sensible
windowing/crop. This visual decision is the only mandatory manual capture step.

## Verify And Publish The Approved Set

After review, verify the unchanged bundle:

```bash
python scripts/marketing_media.py verify
```

Any relevant frontend, semantic/pixel, extension, capture-plan, source-manifest,
inventory, attribution, or artifact change invalidates the bundle.

Publish only the complete reviewed set, using the tag that matches the captured
package version:

```bash
VERSION="$(python scripts/check_versions.py --print-version)"
python scripts/marketing_media.py publish \
  --tag "v${VERSION}" \
  --docs-repo ../dcmview-docs \
  --approve
```

Publication copies the same bundle into the root README/PyPI asset directory,
the VS Code/Open VSX Marketplace asset directory, and `dcmview-docs`; it updates
bounded Markdown gallery blocks and creates the documentation attribution page.
The command never publishes a partial scene selection. Review and commit the
main-repository changes normally, then create the separate documentation branch
and pull request required by the release checklist.

Once approved media are committed, the CI-safe check below verifies their
hashes and UI-input digest without requiring ignored source DICOM files:

```bash
python scripts/check.py marketing
```
