# DICOM compatibility campaign

This directory owns DICOM View's manifest-driven compatibility automation. It
measures viewer behavior; it does not grade DICOM conformance or clinical
suitability.

Freeze the prepared corpus scope before running any probes:

```bash
python scripts/compatibility/scope.py \
  --suite-root /path/to/dicom-test-suite \
  --output /path/outside/both/repos/canonical-worklist.json
```

The scope freezer verifies every manifest-selected file hash, records each
manifest hash, and deduplicates copies only when both the DICOM file hash and a
normalized expected-contract hash match. The output is created read-only and
will not be replaced by non-identical content. It separately records execution
safety (`safe`, `timeout`, `crash`, `flaky`) and compatibility outcomes (`full_support`,
`metadata_only`, `known_gap`, `failure`, `unavailable`).

Run a bounded HTTP campaign against a previously built binary:

```bash
python scripts/compatibility/run.py \
  --suite-root /path/to/dicom-test-suite \
  --worklist /outside/canonical-worklist.json \
  --binary target/debug/dcmview \
  --root smoke \
  --output /outside/smoke-run-1
```

The runner launches a loopback, port-zero, no-browser process with structured
startup output and VS Code bridge bypass. It waits for `scan_complete`, maps
files by normalized path plus SOP Instance UID, and probes file information,
tags, display and raw frames, structured errors, and cache MISS/HIT behavior.
It also captures the completed `/api/series` catalog once per shard and records
each file's logical stack, virtual position, ordered source frames, geometry
warnings, concatenation identity, and WSI level/companion classification.
Manifest-declared series capabilities are evaluated from that server-owned
evidence rather than by reimplementing DICOM ordering in the harness. For
reference-bearing objects, the runner compares the typed reference endpoint to
the manifest identity closure and requires every declared SOP/path/frame target
to resolve locally before marking reference resolution as probed. Physical
pixel aspect is compared exactly from `/api/files`; that observation is labeled
as API metadata evidence and does not claim a real-browser layout check.

For lossless transfer syntaxes, every manifest frame hash is compared with the
decoded raw endpoint, not only the first frame. The first and last display
frames are also decoded far enough to validate PNG dimensions, while declared
small visual patterns are checked from decoded PNG pixels where an explicit
validator exists.

It writes separate process logs, a companion-schema detail report, a timing-free
normalized report for reproducibility comparison, and a SHA-256 artifact index.
