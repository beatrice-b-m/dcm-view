## Summary

Describe the focused change this pull request makes and why it is needed.

## Verification

List the commands or inspections you ran, and note any checks that were not run.

```text

```

## Documentation

- [ ] I updated README, docs, Python docstrings, VS Code docs, or release notes when this change affects user-facing behavior.
- [ ] Documentation is not needed because this change is internal, test-only, or otherwise invisible to users.

## Safety And Data Hygiene

- [ ] This pull request does not include PHI, sensitive DICOM files, sensitive screenshots, unredacted logs, private hostnames, usernames, local paths, tokens, or private research data.
- [ ] Any fixtures, screenshots, logs, or examples are synthetic, public, or fully de-identified and approved for public sharing.
- [ ] I understand that `dcmview` is for research and development inspection, not clinical diagnosis.

## Area Checklist

- [ ] Rust changes were formatted with `cargo fmt --all`.
- [ ] Rust source or Cargo changes were checked with `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --locked`.
- [ ] Rust behavior changes have targeted `cargo test --locked ...` coverage.
- [ ] Frontend TypeScript or Svelte changes were checked with `npm --prefix frontend run typecheck`.
- [ ] Frontend runtime or build changes were checked with `npm --prefix frontend run build` when feasible.
- [ ] Python wrapper or packaging changes were checked with `python -m unittest discover -s python/tests`.
- [ ] VS Code extension changes were checked with `npm --prefix vscode run compile`.
- [ ] `debug-api` or server API exposure changes were checked with `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --features debug-api --locked`.

## Follow-Up

List known limitations, deferred work, or release-note considerations.
