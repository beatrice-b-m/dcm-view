# DICOM compatibility campaign — 2026-08-28

This is the durable human-readable summary of the implementation campaign
defined by `docs/dicom-compatibility-implementation-plan.md`. Machine-readable
reports under `artifacts/compatibility/2026-08-28` remain the evidence source.
The campaign measures `dcmview` behavior for research inspection; it is not a
DICOM conformance certificate or a clinical validation.

## Frozen inputs

- Read-only sibling dependency: `dicom-test-suite` at commit
  `28d45eb7ced872c5ce92f920e694acccd49e6478`.
- Corpus lock SHA-256:
  `69e49a9f9a0640e42110219dfa14f8494e5ac0686d4b9bfd3ff1c3e2d85eeaba`.
- Support policy SHA-256:
  `62f1e515dadde1e6fb4c78bf8ed33359acc63bfd6671dd688625f1a3587e1e45`.
- Frozen worklist file SHA-256:
  `7e3c761f2f424d5288f9a9dd34ae835d1bc581a5366503a2fb5ac95d1b697e24`.
- Frozen worklist content SHA-256:
  `58334a365a5fa146d83acea2b1bc72a762725904e9193ebcd212405376c3aaa0`.

The resolved immutable worklist is
`artifacts/compatibility/2026-08-28/worklist-resolved.json`. It supersedes the
earlier worklist only because the RT Image Storage policy now matches its
manifest-declared image contract; the pinned suite and prepared corpus bytes
did not change.

The prepared profiles contained 169 physical/152 logical valid cases, one
legacy case, 15 negative cases, 139 physical files across eight stress cases
with seven qualification contracts, and one payload-free fuzz qualification.
Their manifest SHA-256 values are, respectively,
`e3496435d222632c1c1c4316c42c130382c2170bb6b130c67dee71cef5c4a985`,
`bcbedc448913a9c4086f051d0f05b345560336573e1a1d18e097aafa61355e0b`,
`85365fc01fa60d254ce025252cfcc94e2621e25bb12a6b62e17445d13468a630`,
`6035d6bc527dd6c7f68c5f39b16af14bdc96b50142363a80a4393334f4f94ed6`,
and `0a9448e3f30d3bbe0824592d8d232719198d65936fb1cc773849f1ad60a4b2ab`.

## Final campaign results

The resolved reports used `target/debug/dcmview` version 0.2.11 at viewer commit
`ee3f43c`, with campaign binary SHA-256
`75c2097836dbae9602b5b729f1686109ecb0b2642b1f27e9adccb3b5580f2a40`.

| Profile | Result | Evidence |
|---|---|---|
| Valid (`all`) | 169/169 execution-safe; base runner: 132 verified and 37 explicitly unverified; fail-closed evidence: 166 passed, three expected unsupported, and zero failed; zero crashes, timeouts, flaky results, or unavailable cases | `artifacts/compatibility/2026-08-28/resolved-all-run2/` |
| Legacy | 1/1 execution-safe and verified; fail-closed evidence passed | `artifacts/compatibility/2026-08-28/resolved-legacy/` |
| Negative | 15/15 passed the declared bounded failure-layer and healthy-recovery assertions | `artifacts/compatibility/2026-08-28/resolved-negative-run2/report.json` |
| Stress | 139/139 files passed; bounded execution, resource measurements, error recovery, shutdown, and observed in-progress discovery cancellation assertions passed | `artifacts/compatibility/2026-08-28/resolved-stress/report.json` |
| Fuzz | 64 deterministic candidates, 297 mutations, and 24,465 target operations; all 64 were clean rejections, zero unacceptable outcomes, and zero retained payloads | `artifacts/compatibility/2026-08-28/resolved-fuzz-run2/report.json` |

The valid and legacy directories contain the detailed report, normalized
report, evidence report, viewer report, process logs, and SHA-256 artifact
index. The resolved valid artifact-index file itself has SHA-256
`771f27e2b25e713cf2736eef61b50c8dc98bc8f6602de76fc0c330e5350d620b`.
The valid detail report has SHA-256
`86f4aaa6b7ba165dd1b20425a3666b7615704c8c3243f07899ef0ca16c651b72`;
its fail-closed evidence report has SHA-256
`6a7ddec49bbe3e66a4f0dca6a65231dadaf8069a52e66b65cfd80688ae42ac5e`.

All ten previously failed valid-profile results now pass their complete
required assertion sets. The resolution adds exact NM dimension, PET activity,
ultrasound timing, XA/XRF projection, and non-square pixel evidence; records
JPEG Baseline maximum absolute error and RMSE against the manifest recipe;
decodes retained ISO 2022 extension sequences using Specific Character Set;
and classifies RT Image Storage as pixel-faithful inspection while continuing
to exclude treatment-planning interpretation.

## Browser acceptance

The real Svelte application was exercised against the prepared corpus through
a real local `dcmview` backend. The representative matrix covered metadata-only
objects, unsupported-transfer-syntax messaging and recovery, SEG pixel preview
and semantic context, unavailable-overlay explanations, stored Parametric Map
values and declared mappings, stored RT Dose pixels and declared scaling,
typed reference navigation, positioned WSI tile labels/minimap, multiframe and
cine navigation, server-PNG and browser-raw paths, full-dynamic windowing,
zoom/pan/orientation controls, and file switching. No browser failure occurred,
so the plan's failure-only screenshot/log capture path did not produce an
artifact.

## Intentional limitations

The base runner's 37 unverified valid-corpus results remain explicit outcomes,
not silent passes. When a policy-selected assertion lacks evidence, the
fail-closed evidence report records a failure. Current boundaries remain:

- Semantic context is conservative metadata interpretation for SEG,
  Parametric Map, and RT Dose. It does not establish clinical correctness, and
  incompatible or absent geometry disables overlays rather than guessing.
- WSI displays one metadata-positioned tile and does not stitch neighbors or
  reconstruct the Total Pixel Matrix.
- DICOMDIR is recognized and skipped; the file-set hierarchy is not parsed.
- ICC bytes may be preserved, but numeric color transformation and
  frame-to-optical-path profile selection are not claimed.
- The current prepared shutter opens over the full frame, so it proves bounded
  non-regression but not outside-opening replacement.
- Reduced stress qualification records baselines; reviewed hard thresholds and
  the suite's unavailable full-scale scheduled runner remain out of scope.
- The suite provides fuzz qualification evidence rather than a reusable fuzz
  payload corpus. The bounded deterministic adapter retains only unacceptable
  failures; this run retained none.
- Explicitly unsupported transfer syntaxes and out-of-scope semantic object
  families remain controlled metadata or request states according to the
  versioned support policy.

No CI, scheduled workflow, or release-process integration was added.
