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
