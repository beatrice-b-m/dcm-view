# DICOM compatibility campaign — 2026-08-28

This is the durable human-readable summary of the implementation campaign
defined by `docs/dicom-compatibility-implementation-plan.md`. Machine-readable
reports under `artifacts/compatibility/2026-08-28` remain the evidence source.
The campaign measures `dcmview` behavior for research inspection; it is not a
DICOM conformance certificate or a clinical validation.

## Frozen inputs

- Read-only sibling dependency: `dicom-test-suite` at commit
  `28d45a535669083522f5ca5a5a6712fb5015b612`.
- Corpus lock SHA-256:
  `69e49a9f9a0640e42110219dfa14f8494e5ac0686d4b9bfd3ff1c3e2d85eeaba`.
- Support policy SHA-256:
  `55155658127605f1a5c29731ef3bf44dd8a89bc9ae4ae02f5f05db32d9cca769`.
- Frozen worklist file SHA-256:
  `ae488f48a2e81f9fa583a47bff079d5f1c75c6cf2187787c1248d2643f8ee288`.
- Frozen worklist content SHA-256:
  `4a38e1198b83c318f7a22da691974087c4f471cf97580bbfeac0f519abe3b16b`.

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

The final reports used `target/debug/dcmview` version 0.2.11 with binary
SHA-256 `db1aa8718ad0faf9cc8646946432852f1977b6d67b9ab51b22bc9766594f10cf`.

| Profile | Result | Evidence |
|---|---|---|
| Valid (`all`) | 169/169 execution-safe; 126 verified, 43 explicitly unverified; zero compatibility failures, crashes, timeouts, flaky results, or unavailable cases | `artifacts/compatibility/2026-08-28/final-all-final/` |
| Legacy | 1/1 execution-safe and verified | `artifacts/compatibility/2026-08-28/final-legacy-final/` |
| Negative | 15/15 passed the declared bounded failure-layer and healthy-recovery assertions | `artifacts/compatibility/2026-08-28/final-negative-final/report.json` |
| Stress | 139/139 files passed; bounded execution, resource measurements, error recovery, shutdown, and cancellation assertions passed | `artifacts/compatibility/2026-08-28/final-stress-final/report.json` |
| Fuzz | 64 deterministic candidates, 243 mutations, and 37,829 target operations; all 64 were clean rejections, zero unacceptable outcomes, and zero retained payloads | `artifacts/compatibility/2026-08-28/final-fuzz-final/report.json` |

The valid and legacy directories contain the detailed report, normalized
report, evidence report, viewer process logs, and SHA-256 artifact index. The
valid artifact-index file itself has SHA-256
`315fdc64157f38bc08f2be112863aaa0c8489adba813a01065bec170e9a4a037`;
the legacy index has
`079b48b1d6b3e4137cfebb02b520d96f498c72acd50796a46a6870898f25c4be`.

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

The 43 unverified valid-corpus results are explicit policy outcomes where the
suite does not provide a sufficient oracle for the capability; they are not
silent passes. Current boundaries remain:

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
