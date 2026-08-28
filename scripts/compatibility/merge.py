#!/usr/bin/env python3
"""Merge a corrected compatibility overlay into a frozen canonical worklist."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any, Optional

try:
    from scripts.compatibility.scope import (
        ScopeError,
        canonical_json,
        contract_sha256,
        sha256_file,
        write_immutable_json,
    )
except ModuleNotFoundError:  # Direct script execution from this directory.
    from scope import (  # type: ignore[no-redef]
        ScopeError,
        canonical_json,
        contract_sha256,
        sha256_file,
        write_immutable_json,
    )


MERGE_IDENTITY_FIELDS = ("case_id", "selected.path", "sop_instance_uid")
CONTRACT_INVARIANT_FIELDS = ("case_id", "dicom", "image", "uids")


def _load_verified_worklist(path: Path) -> dict[str, Any]:
    try:
        worklist = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ScopeError(f"cannot read worklist {path}: {error}") from error
    if not isinstance(worklist, dict) or not isinstance(worklist.get("canonical_files"), list):
        raise ScopeError(f"worklist lacks canonical_files: {path}")
    declared_hash = worklist.get("worklist_sha256")
    unhashed = {key: value for key, value in worklist.items() if key != "worklist_sha256"}
    observed_hash = hashlib.sha256(canonical_json(unhashed)).hexdigest()
    if declared_hash != observed_hash:
        raise ScopeError(
            f"worklist content hash mismatch for {path}: "
            f"expected {declared_hash}, observed {observed_hash}"
        )
    for row in worklist["canonical_files"]:
        if not isinstance(row, dict) or not isinstance(row.get("selected"), dict):
            raise ScopeError(f"worklist contains an invalid canonical row: {path}")
        selected_path = Path(str(row["selected"].get("normalized_path", "")))
        if not selected_path.is_file():
            raise ScopeError(f"selected DICOM file is missing: {selected_path}")
        if sha256_file(selected_path) != row.get("sha256"):
            raise ScopeError(f"selected DICOM hash mismatch: {selected_path}")
        expected = row.get("expected_contract")
        if not isinstance(expected, dict) or contract_sha256(expected) != row.get(
            "contract_sha256"
        ):
            raise ScopeError(f"expected-contract hash mismatch: {selected_path}")
    return worklist


def _identity(row: dict[str, Any]) -> tuple[str, str, str]:
    selected = row.get("selected") or {}
    values = (row.get("case_id"), selected.get("path"), row.get("sop_instance_uid"))
    if not all(isinstance(value, str) and value for value in values):
        raise ScopeError(f"canonical row lacks merge identity {MERGE_IDENTITY_FIELDS}: {values}")
    return values  # type: ignore[return-value]


def _index(rows: list[dict[str, Any]], label: str) -> dict[tuple[str, str, str], dict[str, Any]]:
    indexed: dict[tuple[str, str, str], dict[str, Any]] = {}
    for row in rows:
        key = _identity(row)
        if key in indexed:
            raise ScopeError(f"duplicate {label} merge identity: {key}")
        indexed[key] = row
    return indexed


def merge_worklists(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    base_rows = base["canonical_files"]
    overlay_rows = overlay["canonical_files"]
    base_by_identity = _index(base_rows, "base")
    overlay_by_identity = _index(overlay_rows, "overlay")
    missing = sorted(set(overlay_by_identity) - set(base_by_identity))
    if missing:
        raise ScopeError(f"overlay identities are absent from the canonical base: {missing}")

    replacements: dict[tuple[str, str, str], dict[str, Any]] = {}
    for key, corrected in overlay_by_identity.items():
        original = base_by_identity[key]
        file_changed = corrected["sha256"] != original["sha256"]
        contract_changed = corrected["contract_sha256"] != original["contract_sha256"]
        if file_changed != contract_changed:
            raise ScopeError(
                f"overlay must change file and expected contract together for {key}"
            )
        if not file_changed:
            continue
        original_contract = original["expected_contract"]
        corrected_contract = corrected["expected_contract"]
        changed_invariants = [
            field
            for field in CONTRACT_INVARIANT_FIELDS
            if original_contract.get(field) != corrected_contract.get(field)
        ]
        if changed_invariants:
            raise ScopeError(
                f"overlay changes stable contract identity for {key}: {changed_invariants}"
            )
        replacements[key] = corrected

    merged = {
        key: value
        for key, value in base.items()
        if key not in {"worklist_sha256", "canonical_files", "summary", "merge"}
    }
    merged["merge"] = {
        "policy": "case_id_selected_path_and_sop_instance_uid",
        "contract_invariants": list(CONTRACT_INVARIANT_FIELDS),
        "base_worklist_sha256": base["worklist_sha256"],
        "overlay_worklist_sha256": overlay["worklist_sha256"],
        "overlap_files": len(overlay_rows),
        "replacement_files": len(replacements),
    }
    merged["summary"] = {
        **base["summary"],
        "overlay_files": len(overlay_rows),
        "corrected_replacements": len(replacements),
    }
    merged["canonical_files"] = [
        replacements.get(_identity(row), row) for row in base_rows
    ]
    merged["worklist_sha256"] = hashlib.sha256(canonical_json(merged)).hexdigest()
    return merged


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--overlay", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        base = _load_verified_worklist(args.base.resolve())
        overlay = _load_verified_worklist(args.overlay.resolve())
        merged = merge_worklists(base, overlay)
        write_immutable_json(args.output.resolve(), merged)
    except ScopeError as error:
        print(f"compatibility merge error: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"output": str(args.output.resolve()), **merged["summary"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
