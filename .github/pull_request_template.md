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

- [ ] I ran `python scripts/check.py quick --install`, or listed the equivalent targeted checks above.
- [ ] Frontend changes were checked with `python scripts/check.py frontend --install`.
- [ ] Rust changes were checked with `python scripts/check.py rust --install` or focused tests plus `python scripts/check.py rust-lint --install`.
- [ ] Python wrapper or packaging changes were checked with `python scripts/check.py python-unit` and, when they affect binary launch, `python scripts/check.py python-integration --install`.
- [ ] VS Code extension changes were checked with `python scripts/check.py vscode --install` and, when they affect runtime behavior, `python scripts/check.py vscode-integration --install`.
- [ ] `debug-api` or server API exposure changes were checked with `DCMVIEW_SKIP_FRONTEND_BUILD=1 cargo check --features debug-api --locked`.

## Follow-Up

List known limitations, deferred work, or release-note considerations.
