#!/usr/bin/env python3
"""Freeze immutable, profile-isolated compatibility campaign worklists."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

try:
    from scripts.compatibility.corpus import DEFAULT_LOCK, CorpusError, load_json, sha256_file, verify_manifest, verify_suite
    from scripts.compatibility.policy import DEFAULT_POLICY, load_policy, policy_sha256, resolve
except ModuleNotFoundError:
    from corpus import DEFAULT_LOCK, CorpusError, load_json, sha256_file, verify_manifest, verify_suite  # type: ignore[no-redef]
    from policy import DEFAULT_POLICY, load_policy, policy_sha256, resolve  # type: ignore[no-redef]

WORKLIST_SCHEMA_VERSION = "0.2.0"
VALID_PROFILES = ("all", "legacy", "negative", "stress", "fuzz")
CONTRACT_EXCLUDED_FIELDS = {"sha256", "size_bytes"}

class ScopeError(RuntimeError):
    pass

def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")

def expected_contract(file_entry: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in sorted(file_entry.items()) if key not in CONTRACT_EXCLUDED_FIELDS}

def contract_sha256(file_entry: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_json(expected_contract(file_entry))).hexdigest()

def _file_row(profile: str, root: Path, manifest_sha256: str, entry: dict[str, Any]) -> dict[str, Any]:
    relative, case_id, declared_hash = entry.get("path"), entry.get("case_id"), entry.get("sha256")
    if not all(isinstance(value, str) and value for value in (relative, case_id, declared_hash)):
        raise ScopeError(f"manifest entry lacks path/case/hash identity in {profile}")
    selected = (root / relative).resolve()
    try:
        selected.relative_to(root.resolve())
    except ValueError as error:
        raise ScopeError(f"manifest path escapes {profile} root: {relative}") from error
    if not selected.is_file() or sha256_file(selected) != declared_hash:
        raise ScopeError(f"manifest file hash mismatch: {selected}")
    uids = entry.get("uids") or {}
    sop_uid = uids.get("sop_instance_uid") if isinstance(uids, dict) else None
    if profile not in {"negative", "fuzz"} and (not isinstance(sop_uid, str) or not sop_uid):
        raise ScopeError(f"valid manifest entry lacks SOP Instance UID: {profile}:{relative}")
    identity = {"profile": profile, "manifest_sha256": manifest_sha256, "case_id": case_id, "path": relative}
    return {"kind": {"all": "valid", "legacy": "legacy", "negative": "negative", "stress": "stress"}[profile],
            "manifest_identity": identity, "manifest_identity_sha256": hashlib.sha256(canonical_json(identity)).hexdigest(),
            "case_id": case_id, "path": relative, "normalized_path": str(selected), "sha256": declared_hash,
            "contract_sha256": contract_sha256(entry), "sop_instance_uid": sop_uid, "expected_contract": expected_contract(entry)}

def build_worklist(suite_root: Path, profile_roots: dict[str, Path], *, lock_path: Path = DEFAULT_LOCK, policy_path: Path = DEFAULT_POLICY) -> dict[str, Any]:
    if not profile_roots:
        raise ScopeError("at least one explicit profile root is required")
    unknown = set(profile_roots) - set(VALID_PROFILES)
    if unknown:
        raise ScopeError(f"unknown profiles: {sorted(unknown)}")
    try:
        lock = load_json(lock_path); verify_suite(suite_root, lock); policy = load_policy(policy_path)
    except Exception as error:
        raise ScopeError(str(error)) from error
    manifests: list[dict[str, Any]] = []; files: list[dict[str, Any]] = []; qualifications: list[dict[str, Any]] = []; unavailable: list[dict[str, Any]] = []
    seen: dict[str, str] = {}
    for profile in VALID_PROFILES:
        if profile not in profile_roots:
            continue
        root = profile_roots[profile].resolve()
        try:
            verified = verify_manifest(profile, root, lock); manifest = load_json(root / "manifest.json")
        except CorpusError as error:
            raise ScopeError(str(error)) from error
        manifests.append({**verified, "root": str(root)})
        for entry in manifest.get("files", []):
            row = _file_row(profile, root, verified["sha256"], entry); key = row["manifest_identity_sha256"]
            content = row["contract_sha256"]
            if key in seen:
                if seen[key] != content:
                    raise ScopeError(f"duplicate manifest identity has conflicting contract: {profile}:{row['path']}")
                continue
            seen[key] = content; rule = resolve(policy, entry, profile)
            row["policy"] = {"rule_id": rule["id"], "classification": rule["classification"], "required_assertions": rule["required_assertions"],
                             "semantic_context_assertions": rule["semantic_context_assertions"], "expected_unsupported": rule["expected_unsupported"]}
            files.append(row)
        for entry in manifest.get("qualifications", []):
            rule = resolve(policy, entry, profile)
            qualifications.append({"profile": profile, "case_id": entry["case_id"], "contract": entry,
                                   "contract_sha256": hashlib.sha256(canonical_json(entry)).hexdigest(),
                                   "policy": {"rule_id": rule["id"], "classification": rule["classification"], "required_assertions": rule["required_assertions"]}})
        unavailable.extend({"profile": profile, **entry} for entry in manifest.get("skipped_cases", []))
    files.sort(key=lambda row: (row["manifest_identity"]["profile"], row["case_id"], row["path"]))
    worklist: dict[str, Any] = {
        "worklist_schema_version": WORKLIST_SCHEMA_VERSION,
        "suite": {"root": str(suite_root.resolve()), "commit": lock["suite"]["commit"]},
        "inputs": {"lock": str(lock_path.resolve()), "lock_sha256": sha256_file(lock_path), "policy": str(policy_path.resolve()),
                   "policy_sha256": policy_sha256(policy), "profiles": [p for p in VALID_PROFILES if p in profile_roots], "manifests": manifests},
        "models": {"valid_files": [r for r in files if r["kind"] == "valid"], "legacy_files": [r for r in files if r["kind"] == "legacy"],
                   "negative_inputs": [r for r in files if r["kind"] == "negative"], "stress_files": [r for r in files if r["kind"] == "stress"],
                   "stress_scenarios": [r for r in qualifications if r["profile"] == "stress"], "fuzz_qualifications": [r for r in qualifications if r["profile"] == "fuzz"]},
        "files": files, "unavailable": unavailable,
        "summary": {"files": len(files), "logical_cases": len({(r['manifest_identity']['profile'], r['case_id']) for r in files}),
                    "qualifications": len(qualifications), "unavailable_selected_profiles": len(unavailable)}}
    worklist["worklist_sha256"] = hashlib.sha256(canonical_json(worklist)).hexdigest(); return worklist

def load_worklist(path: Path) -> dict[str, Any]:
    worklist = load_json(path); version = worklist.get("worklist_schema_version")
    if version != WORKLIST_SCHEMA_VERSION:
        raise ScopeError(f"incompatible worklist schema {version!r}; regenerate with scope.py (expected {WORKLIST_SCHEMA_VERSION})")
    declared = worklist.get("worklist_sha256"); unhashed = {k: v for k, v in worklist.items() if k != "worklist_sha256"}
    observed = hashlib.sha256(canonical_json(unhashed)).hexdigest()
    if declared != observed:
        raise ScopeError(f"worklist content hash mismatch: expected {declared}, observed {observed}")
    return worklist

def write_immutable_json(path: Path, value: dict[str, Any]) -> None:
    encoded = json.dumps(value, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    if path.exists():
        if path.read_bytes() == encoded: return
        raise ScopeError(f"refusing to replace non-identical frozen worklist: {path}")
    path.parent.mkdir(parents=True, exist_ok=True); descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream: stream.write(encoded); stream.flush(); os.fsync(stream.fileno())
        os.replace(temporary, path); path.chmod(0o444)
    finally: Path(temporary).unlink(missing_ok=True)

def _profile_root(value: str) -> tuple[str, Path]:
    profile, separator, root = value.partition("=")
    if not separator or profile not in VALID_PROFILES or not root: raise argparse.ArgumentTypeError("expected PROFILE=ROOT")
    return profile, Path(root)

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--suite-root", type=Path, required=True)
    parser.add_argument("--profile-root", action="append", type=_profile_root, required=True); parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY); parser.add_argument("--output", type=Path, required=True); args = parser.parse_args(argv)
    roots = dict(args.profile_root)
    if len(roots) != len(args.profile_root): print("compatibility scope error: duplicate profile root", file=sys.stderr); return 2
    try: worklist = build_worklist(args.suite_root, roots, lock_path=args.lock, policy_path=args.policy); write_immutable_json(args.output.resolve(), worklist)
    except ScopeError as error: print(f"compatibility scope error: {error}", file=sys.stderr); return 2
    print(json.dumps({"output": str(args.output.resolve()), **worklist["summary"]}, sort_keys=True)); return 0

if __name__ == "__main__": raise SystemExit(main())
