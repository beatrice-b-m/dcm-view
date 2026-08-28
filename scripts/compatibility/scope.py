#!/usr/bin/env python3
"""Verify prepared manifests and freeze a canonical compatibility worklist."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from collections import defaultdict
from pathlib import Path
from typing import Any, Optional


WORKLIST_SCHEMA_VERSION = "0.1.0"
DEFAULT_CORPUS_NAME = "viewer-baseline-b4ea0a4"
DEFAULT_SOURCE_COMMIT = "b4ea0a450b63408f9f62709691b50dd50cb64594"
EXPECTED_FILE_COUNT = 165
EXPECTED_GENERATED_CASE_COUNT = 146
EXPECTED_IMPLEMENTED_UNPREPARED_COUNT = 4

CONTRACT_FIELDS = (
    "case_id",
    "dicom",
    "expected_capabilities",
    "expected_semantics",
    "expected_visual_checks",
    "image",
    "known_stressors",
    "pixel_data",
    "recipe",
    "references",
    "uids",
)


class ScopeError(RuntimeError):
    """Raised when the prepared corpus cannot be identity-proven."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def expected_contract(file_entry: dict[str, Any]) -> dict[str, Any]:
    return {field: file_entry.get(field) for field in CONTRACT_FIELDS}


def contract_sha256(file_entry: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_json(expected_contract(file_entry))).hexdigest()


def _load_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ScopeError(f"cannot read JSON object {path}: {error}") from error
    if not isinstance(value, dict):
        raise ScopeError(f"expected a JSON object in {path}")
    return value


def _resolve_manifest_path(root: Path, relative: str) -> Path:
    candidate = (root / relative).resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError as error:
        raise ScopeError(f"manifest path escapes corpus root: {relative}") from error
    return candidate


def _unavailable_inventory(
    suite_root: Path,
    generated_cases: set[str],
    skipped_by_case: dict[str, list[dict[str, Any]]],
) -> list[dict[str, Any]]:
    registry_path = suite_root / "cases" / "registry.json"
    registry = _load_object(registry_path)
    cases = registry.get("cases")
    if not isinstance(cases, list):
        raise ScopeError(f"registry cases must be an array: {registry_path}")

    unavailable: list[dict[str, Any]] = []
    for case in cases:
        if not isinstance(case, dict) or not isinstance(case.get("case_id"), str):
            raise ScopeError(f"registry contains an invalid case entry: {registry_path}")
        case_id = case["case_id"]
        if case_id in generated_cases:
            continue
        status = case.get("status")
        if status not in {"implemented", "planned"}:
            continue
        manifest_rows = skipped_by_case.get(case_id, [])
        reason_codes = sorted(
            {row.get("reason_code", "not_selected") for row in manifest_rows}
        )
        unavailable.append(
            {
                "case_id": case_id,
                "availability": "unavailable",
                "registry_status": status,
                "reason_codes": reason_codes or ["not_selected"],
                "profiles": sorted(case.get("profiles", [])),
                "requirements": case.get("requirements", {}),
            }
        )
    return sorted(unavailable, key=lambda row: row["case_id"])


def build_worklist(
    suite_root: Path,
    corpus_root: Path,
    *,
    source_commit: str = DEFAULT_SOURCE_COMMIT,
    enforce_prepared_baseline: bool = True,
) -> dict[str, Any]:
    suite_root = suite_root.resolve()
    corpus_root = corpus_root.resolve()
    manifest_paths = sorted(corpus_root.glob("*/manifest.json"))
    if not manifest_paths:
        raise ScopeError(f"no prepared manifests found below {corpus_root}")

    manifest_inventory: list[dict[str, Any]] = []
    identities: dict[tuple[str, str], dict[str, Any]] = {}
    occurrences: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    generated_cases: set[str] = set()
    skipped_by_case: dict[str, list[dict[str, Any]]] = defaultdict(list)

    for manifest_path in manifest_paths:
        root_name = manifest_path.parent.name
        manifest = _load_object(manifest_path)
        files = manifest.get("files")
        skipped = manifest.get("skipped_cases")
        if not isinstance(files, list) or not isinstance(skipped, list):
            raise ScopeError(f"manifest arrays are missing in {manifest_path}")
        manifest_inventory.append(
            {
                "root": root_name,
                "path": str(manifest_path.relative_to(suite_root)),
                "sha256": sha256_file(manifest_path),
                "file_count": len(files),
                "skipped_count": len(skipped),
            }
        )
        for skipped_row in skipped:
            if isinstance(skipped_row, dict) and isinstance(skipped_row.get("case_id"), str):
                skipped_by_case[skipped_row["case_id"]].append(skipped_row)

        for entry in files:
            if not isinstance(entry, dict):
                raise ScopeError(f"manifest file entry is not an object: {manifest_path}")
            required = ("case_id", "path", "sha256", "uids")
            if any(key not in entry for key in required):
                raise ScopeError(f"manifest file entry lacks identity fields: {manifest_path}")
            sop_instance_uid = entry["uids"].get("sop_instance_uid")
            if not isinstance(sop_instance_uid, str) or not sop_instance_uid:
                raise ScopeError(f"manifest file entry lacks SOP Instance UID: {manifest_path}")
            file_path = _resolve_manifest_path(manifest_path.parent, entry["path"])
            if not file_path.is_file():
                raise ScopeError(f"manifest file is missing: {file_path}")
            actual_sha256 = sha256_file(file_path)
            if actual_sha256 != entry["sha256"]:
                raise ScopeError(
                    f"file hash mismatch for {file_path}: expected {entry['sha256']}, "
                    f"observed {actual_sha256}"
                )
            identity = (actual_sha256, contract_sha256(entry))
            existing = identities.get(identity)
            if existing is None:
                identities[identity] = {
                    "case_id": entry["case_id"],
                    "sha256": actual_sha256,
                    "contract_sha256": identity[1],
                    "sop_instance_uid": sop_instance_uid,
                    "expected_contract": expected_contract(entry),
                }
            elif (
                existing["case_id"] != entry["case_id"]
                or existing["sop_instance_uid"] != sop_instance_uid
            ):
                raise ScopeError(
                    "evidence reuse identity collision for "
                    f"{root_name}/{entry['path']}"
                )
            occurrences[identity].append(
                {
                    "root": root_name,
                    "case_id": entry["case_id"],
                    "path": entry["path"],
                    "normalized_path": str(file_path),
                    "sop_instance_uid": sop_instance_uid,
                }
            )
            generated_cases.add(entry["case_id"])

    canonical_files: list[dict[str, Any]] = []
    for identity in sorted(
        identities,
        key=lambda key: (
            identities[key]["case_id"],
            identities[key]["sop_instance_uid"],
            key,
        ),
    ):
        evidence_rows = sorted(
            occurrences[identity], key=lambda row: (row["root"], row["path"])
        )
        canonical_files.append(
            {
                **identities[identity],
                "selected": evidence_rows[0],
                "occurrences": evidence_rows,
            }
        )

    unavailable = _unavailable_inventory(
        suite_root, generated_cases, skipped_by_case
    )
    implemented_unprepared = [
        row for row in unavailable if row["registry_status"] == "implemented"
    ]
    planned = [row for row in unavailable if row["registry_status"] == "planned"]

    if enforce_prepared_baseline:
        observed = (
            len(canonical_files),
            len(generated_cases),
            len(implemented_unprepared),
        )
        expected = (
            EXPECTED_FILE_COUNT,
            EXPECTED_GENERATED_CASE_COUNT,
            EXPECTED_IMPLEMENTED_UNPREPARED_COUNT,
        )
        if observed != expected:
            raise ScopeError(
                "prepared baseline inventory changed: "
                f"expected files/cases/implemented-unprepared {expected}, observed {observed}"
            )

    worklist: dict[str, Any] = {
        "worklist_schema_version": WORKLIST_SCHEMA_VERSION,
        "corpus": {
            "suite_root": str(suite_root),
            "corpus_root": str(corpus_root),
            "source_commit": source_commit,
            "manifest_count": len(manifest_inventory),
            "manifests": manifest_inventory,
        },
        "evidence_policy": {
            "compatibility_scope": "viewer_behavior_not_dicom_conformance",
            "deduplication": "file_sha256_and_expected_contract_sha256",
            "execution_safety_outcomes": ["safe", "timeout", "crash", "flaky"],
            "compatibility_outcomes": [
                "full_support",
                "metadata_only",
                "known_gap",
                "failure",
                "unavailable",
            ],
        },
        "summary": {
            "canonical_files": len(canonical_files),
            "generated_logical_cases": len(generated_cases),
            "manifest_occurrences": sum(len(rows) for rows in occurrences.values()),
            "implemented_unprepared": len(implemented_unprepared),
            "planned_unavailable": len(planned),
        },
        "canonical_files": canonical_files,
        "unavailable": unavailable,
    }
    worklist["worklist_sha256"] = hashlib.sha256(canonical_json(worklist)).hexdigest()
    return worklist


def write_immutable_json(path: Path, value: dict[str, Any]) -> None:
    encoded = json.dumps(value, indent=2, sort_keys=True).encode("utf-8") + b"\n"
    if path.exists():
        if path.read_bytes() == encoded:
            return
        raise ScopeError(f"refusing to replace non-identical frozen worklist: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        path.chmod(0o444)
    finally:
        try:
            Path(temporary).unlink()
        except FileNotFoundError:
            pass


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite-root",
        type=Path,
        default=os.environ.get("DCMVIEW_COMPAT_SUITE_ROOT"),
        required="DCMVIEW_COMPAT_SUITE_ROOT" not in os.environ,
    )
    parser.add_argument("--corpus-root", type=Path)
    parser.add_argument("--source-commit", default=DEFAULT_SOURCE_COMMIT)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-other-inventory", action="store_true")
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    suite_root = args.suite_root.resolve()
    corpus_root = (
        args.corpus_root.resolve()
        if args.corpus_root
        else suite_root / "generated" / DEFAULT_CORPUS_NAME
    )
    try:
        worklist = build_worklist(
            suite_root,
            corpus_root,
            source_commit=args.source_commit,
            enforce_prepared_baseline=not args.allow_other_inventory,
        )
        write_immutable_json(args.output.resolve(), worklist)
    except ScopeError as error:
        print(f"compatibility scope error: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"output": str(args.output.resolve()), **worklist["summary"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
