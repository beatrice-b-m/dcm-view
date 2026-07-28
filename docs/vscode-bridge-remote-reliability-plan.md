# VS Code bridge — remote interception reliability: findings and remediation plan

> **Archival note:** This is a point-in-time, non-normative investigation and
> remediation plan. Bridge discovery, registry trust, protocol, and lifecycle
> behavior has changed since it was written; proposals below may be implemented,
> superseded, or still deferred. See the
> [configuration reference](configuration.md) and
> [VS Code local-testing guide](vscode-extension-local-testing.md).

Investigation of the report that `dcmview_py.view(...)` on a shared Ubuntu
24.04 server (VS Code Remote-SSH) was not intercepted by the VS Code bridge
and fell back to the local viewer + manual port forwarding. The feature is
expected to operate per-user on multi-user hosts and intercept reliably.

Reviewed implementation surfaces:

- **Writer:** `vscode/src/extension.ts` — bridge HTTP server
  (`ensureBridge`), registry publication (`writeBridgeRegistry`,
  `bridgeRegistryDirectory`), hourly refresh, terminal env injection via
  `environmentVariableCollection`, PATH shims.
- **Reader (Python):** `python/dcmview_py/wrapper.py` — `_bridge_endpoints`,
  `_bridge_registry_endpoints`, `_view_via_vscode_bridge`.
- **Reader (Rust):** `src/main.rs` — `discover_vscode_bridge_endpoints`,
  `discover_vscode_bridge_registry_endpoints`, `run_vscode_bridge_client`.
- Shared contract: `docs/contracts/vscode-bridge-registry.json`,
  `docs/contracts/bridge-protocol.json`.

`extensionKind: ["workspace"]` and `onStartupFinished` activation are correct,
so the extension host, bridge server, and registry all live on the remote —
the architecture is sound. The failures below are discovery/lifecycle bugs.

---

## Step 0 — Triage on the affected server (do this first)

Each root cause below leaves a distinct fingerprint. Before changing code,
capture which one actually fired on the shared server, in the exact shell or
kernel where `view()` fell back:

```bash
# 1. Which discovery path was taken?
env | grep DCMVIEW          # URL/TOKEN present => env path; absent => registry path
echo "$XDG_RUNTIME_DIR"

# 2. If URL/TOKEN present: is the endpoint live?
curl -s -o /dev/null -w '%{http_code}\n' "$DCMVIEW_VSCODE_BRIDGE_URL/launch" \
  -H "Authorization: Bearer $DCMVIEW_VSCODE_BRIDGE_TOKEN" -X POST -d '{}'
# connection refused => stale env endpoint (RC1); 200/4xx => bridge alive

# 3. Registry state as the reader would see it:
ls -la "$XDG_RUNTIME_DIR/dcmview/vscode-bridges" 2>/dev/null
ls -la "/tmp/dcmview-vscode-bridges-$USER" 2>/dev/null
stat -c '%U %a' "$XDG_RUNTIME_DIR/dcmview/vscode-bridges" /tmp/dcmview-vscode-bridges-* 2>/dev/null

# 4. Writer state: in VS Code, Output panel -> "dcmview" channel. Look for
#    "terminal interception enabled at ..." vs "terminal interception disabled: ...".
```

Record the result in this doc; it decides whether RC4 (binary resolution)
needs to be in Phase 1 for your deployment.

---

## Root causes (ranked by likelihood of producing the reported symptom)

### RC1 — Injected env endpoint short-circuits registry fallback (High)

**Files:** `python/dcmview_py/wrapper.py` (`_bridge_endpoints`, ~line 437),
`src/main.rs` (`discover_vscode_bridge_endpoints`, ~line 391).

When `DCMVIEW_VSCODE_BRIDGE_URL`/`_TOKEN` are present, both readers return
**only** that endpoint and never consult the registry. The bridge gets a new
random port and token on every extension-host restart (window reload,
extension update, Remote-SSH server restart), while integrated terminals —
and any tmux/screen server started from one — keep the old values
indefinitely. Result: every `view()` call in that shell dials a dead endpoint,
gets `URLError`, and falls back to the local viewer even though a live bridge
is published in the registry. On long-lived remote shells this is the most
common steady state.

**Fix:** Treat the env endpoint as the *first candidate*, then append
registry-discovered endpoints (deduplicated) instead of returning early.
The existing per-endpoint failover loop in `_view_via_vscode_bridge` /
`run_vscode_bridge_launch` then heals stale env automatically. Do not delete
registry entries when the failing endpoint came from env (it has no registry
file; current `_remove_bridge_registry_endpoint` scan is a harmless no-op but
wasted I/O).

**Acceptance:** Python + Rust tests: with stale env URL/token set and a valid
registry entry present, launch succeeds via the registry endpoint; with both
dead, error mentions the last failure. Existing env-only tests still pass.

### RC2 — Registry directory resolution diverges between writer and readers (High)

**Files:** `vscode/src/extension.ts` (`bridgeRegistryDirectory`),
`python/dcmview_py/wrapper.py` (`_bridge_registry_dir`), `src/main.rs`
(`vscode_bridge_registry_dir_from_values`),
`docs/contracts/vscode-bridge-registry.json`.

All three resolve the directory from the *process's own* environment:
`$DCMVIEW_VSCODE_BRIDGE_REGISTRY_DIR` → `$XDG_RUNTIME_DIR/dcmview/vscode-bridges`
→ `$TMPDIR/dcmview-vscode-bridges-$USER`. On Ubuntu 24.04, `pam_systemd` sets
`XDG_RUNTIME_DIR` for SSH login sessions, but the VS Code Remote server is a
daemonized process whose environment was captured when it was first spawned —
it can lack the variable that interactive shells have (or vice versa for
Jupyter kernels spawned by services, tmux servers from older sessions, cron).
Writer publishes to `/run/user/<uid>/...` while the reader scans
`/tmp/dcmview-vscode-bridges-<user>` (or the reverse) and silently finds
nothing. This breaks notebooks especially, since kernels never receive the
injected env vars (env collection applies to integrated terminals only).

**Fix:** Replace session-env-dependent resolution with one canonical per-user
location that is identical for every process of the same user regardless of
session type: `~/.local/state/dcmview/vscode-bridges` (honoring
`$XDG_STATE_HOME`), mode `0700`, with the existing ownership/permission trust
checks retained. Keep `$DCMVIEW_VSCODE_BRIDGE_REGISTRY_DIR` as an explicit
override for tests. For one release, readers should *also* scan the two legacy
locations (runtime dir, tmp) for compatibility with an older extension on the
host; writer writes only the canonical dir. Update the shared contract fixture
so all three suites enforce the new resolution identically (the F6 fixture
machinery already exists — extend it).

This also removes the predictable shared-`/tmp` namespace entirely (closes
the residual exposure behind review finding F5) and removes the dependency on
`/run/user/<uid>` lifecycle (RC3).

**Acceptance:** Contract fixture updated; TS/Rust/Python suites all assert the
new resolution including the `$XDG_STATE_HOME` and override cases; a test that
a legacy-location entry is still discovered (reader-side compat); manual check
on a shared host that extension + plain SSH shell + Jupyter kernel all resolve
the same path.

### RC3 — `systemd-logind` wipes `/run/user/<uid>` while the VS Code server persists (High)

**Files:** `vscode/src/extension.ts` (refresh timer, `BRIDGE_REGISTRY_REFRESH_MS`).

When the user's last login session closes (e.g. laptop disconnects overnight),
logind removes the runtime directory unless lingering is enabled. The VS Code
remote server — and the bridge — survive, but the registry entry is gone. The
extension only re-publishes on the hourly refresh tick or a config/workspace
event, so interception is silently dead for up to 60 minutes after the user
reconnects, which presents exactly as "the feature intermittently doesn't
work on the shared server."

**Fix:** Primarily resolved by RC2 (state dir is not session-scoped). As
defense in depth, make re-publication event-driven and cheap:

1. On a short interval (60s), `stat` the published registry path; rewrite via
   `writeBridgeRegistry` only if missing (a stat per minute is negligible).
2. Also re-publish on `vscode.window.onDidOpenTerminal` — the moment a user
   opens a terminal is exactly when discovery is about to happen.

Keep the hourly full refresh for `createdAtMs` renewal (TTL semantics
unchanged).

**Acceptance:** Extension test (extracted-helper level): deleting the registry
file results in re-publication on the next check tick with the same
`instanceId`; timers disposed on deactivate.

### RC4 — Interception is all-or-nothing on extension-side binary resolution (Medium — confirm via Step 0)

**Files:** `vscode/src/extension.ts` (`configureTerminalInterception`,
`launchFromBridge`, `resolveBinaryPath`).

`configureTerminalInterception` resolves the dcmview binary *before* starting
the bridge; on failure it disables the bridge, registry, and env injection
with only an output-channel line. On shared research servers users typically
have only `pip install dcmview-py` — whose binary lives inside site-packages
and is invisible to the extension's candidate list. If the installed VSIX
lacks a bundled `linux-x64` binary (generic VSIX, unsupported arch) and
`dcmview` is not on PATH, interception silently never starts even though the
Python side is fully capable of supplying a binary.

**Fix:**

1. Decouple: start the bridge and publish the registry unconditionally;
   resolve the binary lazily inside `launchFromBridge` (and shim generation
   can stay conditional on resolution, since shims wrap the CLI binary).
2. Extend the `/launch` protocol with an optional `binaryPath` supplied by the
   client — `dcmview_py` always knows its bundled binary
   (`_resolve_binary()`). The extension validates before use: absolute path,
   regular file, basename `dcmview`/`dcmview.exe`, and (unix) owned by the
   current uid with no group/other write. Request is already authenticated by
   the per-instance bearer token, so this stays within the same-user trust
   boundary. Update `docs/contracts/bridge-protocol.json` and bump nothing —
   field is optional and ignored by older extensions.
3. When lazy resolution fails *and* the request carried no usable
   `binaryPath`, return a structured 422 with a message the wrapper surfaces
   verbatim, and show a one-time `vscode.window.showWarningMessage`
   prompting to set `dcmview.binaryPath`.

**Acceptance:** Wrapper test that the launch payload includes `binaryPath`;
extension tests for path validation (reject relative, non-dcmview basename,
group-writable); manual: pip-only server with no PATH binary opens the
webview successfully.

### RC5 — Discovery failures are completely silent (Medium, cross-cutting)

**Files:** all three implementations.

Every guard (`bypass env set`, untrusted dir, stat failure, expired entry,
version skip, zero candidates) returns an empty list with no trace. The
wrapper then quietly launches locally — the user cannot distinguish "no
bridge running" from "bridge running but undiscoverable," which is why this
regression reached a shared server before being characterized.

**Fix:** Add `DCMVIEW_VSCODE_BRIDGE_DEBUG=1` honored by Python and Rust
readers: print to stderr the resolved registry dir(s), trust-check result,
each entry considered with its disposition (expired / version-skipped /
accepted / connect-failed), and the chosen endpoint. Add an extension command
`dcmview: Show Bridge Status` that reports bridge URL, registry path, last
publish time, and active session count. When discovery yields zero endpoints
and bypass is not set, the wrapper's existing fallback message should state
the category (`no bridge endpoints found in <dir>` vs `bridge unreachable`).

**Acceptance:** Tests asserting debug output names the registry dir and entry
dispositions; status command listed in `package.json` contributes.

### RC6 — Shims and env reach only new integrated terminals (Low, documentation)

Pre-existing terminals, tmux/screen sessions, and SSH sessions outside VS Code
get neither the PATH shims nor the env vars by design; after RC1+RC2 they are
still covered via registry discovery (Python from anywhere; Rust CLI only
inside a workspace root, per `RegistryMatch::RequireWorkspace`). Document this
matrix in `docs/vscode-extension-local-testing.md` (which terminal types get
which discovery mechanism), including the tmux caveat and
`loginctl enable-linger` note for long-running setups.

---

## Phasing

| Phase | Items | Outcome |
|-------|-------|---------|
| 1 | RC1, RC2, RC3 | Discovery converges on one canonical per-user path with env as a hint, self-healing publication — fixes the shared-server fallback. |
| 2 | RC4, RC5 | pip-only servers work without a PATH binary; failures become diagnosable. |
| 3 | RC6 + contract/doc sync | Documentation and support matrix. |

Phase 1 items ship together: RC1 without RC2 still strands notebooks; RC2
without RC1 still strands long-lived terminals. All three constants/locations
must change in the contract fixture first so any drifting implementation
fails its suite (per finding F6 machinery).

## Compatibility notes

- New canonical registry dir means an old wrapper + new extension (or
  reverse) won't discover each other; reader-side legacy-dir scanning covers
  the common "extension updated first" case for one release. Call out the
  pairing requirement in both changelogs.
- `binaryPath` in `/launch` is optional; old extensions ignore unknown JSON
  fields and new extensions treat its absence as "resolve locally," so the
  protocol change is two-way compatible.
- TTL (3h) and refresh (1h) semantics are unchanged; only publication
  location and re-publication triggers change.
