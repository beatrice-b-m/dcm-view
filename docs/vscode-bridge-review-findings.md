# VS Code bridge registry — open review findings

> **Archival note:** This is a point-in-time, non-normative review, not the
> current bridge issue list. The registry and Rust client have since been
> refactored and hardened; individual findings below should not be assumed open
> or closed without checking current code. See the
> [configuration reference](configuration.md) and
> [current architecture](architecture.md).

Findings from review of commits `0706805` and `6e99a0c`. Each finding is
self-contained: problem, affected files, required resolution, and acceptance
criteria. F1 is the only one with user-visible breakage; the rest are
robustness/hardening and should all be addressed.

---

## F1 — Live bridge entries are deleted and never re-published (High)

**Files:** `vscode/src/extension.ts`, (context: `src/main.rs`, `python/dcmview_py/wrapper.py`)

**Problem:** The extension writes its registry entry only on activation,
configuration change, and workspace-folder change. `createdAtMs` is never
refreshed. Two failure modes follow:

1. A VS Code window open longer than 12 hours (the registry TTL) has its
   entry classified as expired by the next Rust/Python discovery pass, which
   **deletes the live bridge's registry file**. Remote discovery for that
   window is then permanently broken until an unrelated config/folder event
   fires.
2. A transient connection-level failure (e.g. the 5s client timeout against a
   busy-but-alive bridge raises `urllib.error.URLError` in Python, or a
   reqwest send error in Rust) triggers `_remove_bridge_registry_endpoint` /
   `remove_vscode_bridge_registry_endpoint`, permanently de-registering a
   live bridge for the same reason.

**Resolution:** Add a periodic refresh in the extension: a `setInterval`
(suggested 30–60 minutes) that re-invokes `writeBridgeRegistry` for the
current bridge, rewriting the entry with a fresh `createdAtMs`. Clear the
timer in `stopBridge()`/`deactivate` and register it as a disposable on the
extension context. Once refresh exists, deletion becomes self-healing (a
wrongly deleted entry reappears within one refresh interval). Optionally
shorten the TTL in all three implementations to roughly 2× the refresh
interval plus slack (e.g. refresh hourly, TTL 3h) — keep the three constants
in sync.

**Acceptance:**
- Unit test (or extracted-helper test) showing the refresh rewrites the entry
  with an updated `createdAtMs` and the same `instanceId`/path.
- Manually or via test: deleting the registry file while the bridge is alive
  results in the file reappearing after the refresh tick.
- Timer is disposed on deactivation (no leaked interval).

---

## F2 — Rust identifies connection failures by string-matching error text (Medium)

**Files:** `src/main.rs`

**Problem:** `run_vscode_bridge_launch` decides whether to delete a registry
entry with `error.to_string().contains("failed to contact VS Code bridge")` —
matching the `anyhow` context string attached in `launch_vscode_session`. If
that context string is ever reworded, stale-entry cleanup silently stops
firing. It also conflates all `send()` errors (connect, timeout, TLS, etc.)
with "bridge gone".

**Resolution:** Replace the string match with a typed signal. Either:
- introduce a small error enum (e.g. `enum BridgeLaunchError { Connect(reqwest::Error), Http(StatusCode), Decode(...) }`)
  returned by `launch_vscode_session`, deleting the entry only for the
  `Connect` variant; or
- check `reqwest::Error::is_connect() || is_timeout()` on the underlying
  error before wrapping it in `anyhow`.

Python already does this correctly by exception type (`HTTPError` caught
before `URLError`); mirror that semantic.

**Acceptance:** A unit test that an HTTP-status failure does not remove the
registry entry while a connect-level failure does (the Python suite has the
equivalent test to copy from). No `.to_string().contains(...)` on errors.

---

## F3 — Python wrapper discovery has no workspace constraint, asymmetric with Rust CLI (Medium)

**Files:** `python/dcmview_py/wrapper.py`, `docs/vscode-extension-local-testing.md`

**Problem:** After `6e99a0c`, the direct Rust CLI requires a workspace match
(`RegistryMatch::RequireWorkspace`) before using registry-discovered
endpoints, but `dcmview_py.view()` remains fully permissive: any Python
process on the host, in any directory, is intercepted by any open VS Code
window. The permissiveness is intentional (Jupyter kernel cwd may sit outside
the workspace root), but it is undocumented and there is no programmatic
opt-out short of the env var.

**Resolution:**
1. Document the asymmetry in `docs/vscode-extension-local-testing.md`: the
   Python wrapper discovers any live bridge on the host regardless of cwd,
   and `DCMVIEW_VSCODE_BYPASS=1` disables it.
2. Add an explicit keyword to `view()` (e.g. `vscode_bridge: bool = True` or
   `use_vscode: Optional[bool] = None` meaning auto) so library users can opt
   out per-call without touching the environment.

**Acceptance:** Docs updated; new keyword covered by a test showing
`view(..., vscode_bridge=False)` skips bridge discovery and launches locally
(with bypass set per the existing `_popen_options` behavior).

---

## F4 — Readers delete registry files of unrecognized format without checking `version` (Low)

**Files:** `src/main.rs`, `python/dcmview_py/wrapper.py`

**Problem:** Entries with a missing or non-integer `createdAtMs` are unlinked
on sight. The entry format has a `version` field, but neither reader checks
it — so a future format revision (v2 entry written by a newer extension)
would be deleted by an older CLI/wrapper instead of being skipped.

**Resolution:** Only apply delete-on-invalid/delete-on-expired to entries
whose `version` is `1` (or absent, for compatibility with current files).
Entries with an unknown version should be skipped, never deleted. While
touching the Python validation, tighten the `createdAtMs` check to
`isinstance(created_at, int) and not isinstance(created_at, bool)`.

**Acceptance:** Tests in both Rust and Python: a `version: 2` entry with an
unparseable shape survives a discovery pass (not returned as an endpoint,
file still exists); a `version: 1` malformed entry is still removed.

---

## F5 — /tmp fallback registry directory is squat-able on shared hosts (Low, hardening)

**Files:** `vscode/src/extension.ts`, `src/main.rs`, `python/dcmview_py/wrapper.py`

**Problem:** When `XDG_RUNTIME_DIR` is unset (common in containers and some
SSH setups), the registry lives at the predictable path
`$TMPDIR/dcmview-vscode-bridges-$USER` in a world-writable directory. Another
local user can pre-create that directory and plant entries that redirect
launches to a server they control (receiving cwd/file-path metadata and able
to fake success so launches silently no-op). Readers currently trust any JSON
in the directory; the writer fails safe-ish only because `chmod` on a foreign
directory throws.

**Resolution (unix only; skip on Windows):**
- Readers (Rust and Python): before consuming entries, `stat` the registry
  directory and ignore it entirely unless `st_uid` equals the current
  effective uid (and ideally mode has no group/other write). Optionally also
  check per-file ownership.
- Writer (extension): after `mkdir`, verify the directory is owned by the
  current uid before writing the token file; abort registry publication (log
  to the output channel) if not.

**Acceptance:** Unix-only tests: a registry dir owned by another uid is hard
to simulate without root, so test via an injectable stat/ownership-check
seam (e.g. a function parameter or mockable helper) asserting that a
non-owned directory yields zero endpoints and no writes.

---

## F6 — Discovery logic is triplicated across TS/Rust/Python with no shared contract test (Low, testing)

**Files:** `vscode/src/extension.ts`, `src/main.rs`, `python/dcmview_py/wrapper.py`, test suites, `vscode/src/test/fixtures/` (or equivalent)

**Problem:** Registry-directory resolution, `safe_registry_segment`
sanitization, workspace match scoring, ordering, and TTL semantics are each
implemented three times. The copies are currently in sync and individually
tested, but nothing fails if one drifts.

**Resolution:** Add a shared JSON contract fixture (same pattern as the
existing `bridgeContract` used by the extension tests) checked into the repo
and consumed by all three test suites. It should enumerate cases for:
- registry dir resolution (env override / `XDG_RUNTIME_DIR` / tmp fallback,
  including a username needing sanitization),
- `safe_registry_segment` input→output pairs,
- expiry boundaries (fresh, exactly at TTL, past TTL, zero, far-future),
- endpoint ordering given a set of entries + cwd (match beats recency,
  recency breaks ties, dedup).

**Acceptance:** One fixture file; Rust, Python, and TS tests each iterate it
and assert identical results. Changing a constant (e.g. TTL) in only one
implementation fails at least one suite.

---

## Deferred / explicitly out of scope

- Rust test-suite env mutation still uses a process-global `ENV_LOCK` +
  `EnvGuard`; adequate as long as no other test reads these vars. No action
  unless flakiness appears.
