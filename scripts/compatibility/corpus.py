#!/usr/bin/env python3
"""Verify pinned dicom-test-suite inputs and generated profile manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_LOCK = HERE / "corpus-lock.json"


class CorpusError(RuntimeError):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CorpusError(f"cannot read JSON object {path}: {error}") from error
    if not isinstance(value, dict):
        raise CorpusError(f"expected JSON object: {path}")
    return value


def verify_suite(suite_root: Path, lock: dict[str, Any]) -> None:
    suite_root = suite_root.resolve()
    try:
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=suite_root, check=True,
            capture_output=True, text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise CorpusError(f"cannot resolve suite commit: {error}") from error
    if head != lock["suite"]["commit"]:
        raise CorpusError(f"suite commit mismatch: expected {lock['suite']['commit']}, observed {head}")
    checks = {
        "manifest_schema_sha256": suite_root / "schemas/manifest.schema.json",
        "viewer_report_schema_sha256": suite_root / "schemas/viewer-report.schema.json",
        "registry_sha256": suite_root / "cases/registry.json",
        "cargo_lock_sha256": suite_root / "Cargo.lock",
    }
    for field, path in checks.items():
        observed = sha256_file(path)
        if observed != lock["suite"][field]:
            raise CorpusError(f"suite contract hash mismatch for {path}: expected {lock['suite'][field]}, observed {observed}")


def verify_manifest(profile: str, root: Path, lock: dict[str, Any]) -> dict[str, Any]:
    expected = lock["profiles"].get(profile)
    if expected is None:
        raise CorpusError(f"profile is not locked: {profile}")
    manifest_path = root.resolve() / "manifest.json"
    manifest = load_json(manifest_path)
    if manifest.get("manifest_schema_version") != "0.2.0":
        raise CorpusError(f"unsupported manifest schema in {manifest_path}")
    if (manifest.get("run") or {}).get("profile") != profile:
        raise CorpusError(f"profile mismatch in {manifest_path}")
    files = manifest.get("files")
    qualifications = manifest.get("qualifications", [])
    if not isinstance(files, list) or not isinstance(qualifications, list):
        raise CorpusError(f"manifest collections are malformed: {manifest_path}")
    case_ids = {row.get("case_id") for row in files if isinstance(row, dict)}
    observed = (len(files), len(case_ids), len(qualifications))
    wanted = (expected["physical_files"], expected["logical_cases"], expected["qualifications"])
    if observed != wanted:
        raise CorpusError(f"profile inventory mismatch for {profile}: expected {wanted}, observed {observed}")
    if profile == "fuzz" and files:
        raise CorpusError("fuzz qualification must not retain payload files")
    for row in files:
        if not isinstance(row, dict) or not isinstance(row.get("path"), str) or not isinstance(row.get("sha256"), str):
            raise CorpusError(f"invalid file identity in {manifest_path}")
        path = (root / row["path"]).resolve()
        try:
            path.relative_to(root.resolve())
        except ValueError as error:
            raise CorpusError(f"manifest path escapes profile root: {row['path']}") from error
        if not path.is_file() or sha256_file(path) != row["sha256"]:
            raise CorpusError(f"declared file hash mismatch: {path}")
    digest = sha256_file(manifest_path)
    if digest != expected["manifest_sha256"]:
        raise CorpusError(f"manifest digest mismatch for {profile}: expected {expected['manifest_sha256']}, observed {digest}")
    return {"profile": profile, "manifest": str(manifest_path), "sha256": digest,
            "physical_files": observed[0], "logical_cases": observed[1], "qualifications": observed[2]}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite-root", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--suite-binary", type=Path, required=True)
    parser.add_argument("--profile", action="append", choices=["all", "legacy", "negative", "stress", "fuzz"])
    args = parser.parse_args(argv)
    try:
        lock = load_json(args.lock)
        verify_suite(args.suite_root, lock)
        profiles = args.profile or list(lock["profiles"])
        results = []
        for profile in profiles:
            root = args.corpus_root / profile
            completed = subprocess.run([str(args.suite_binary.resolve()), "validate", str(root.resolve())], capture_output=True, text=True)
            if completed.returncode or "validation_failures\t0" not in completed.stdout:
                raise CorpusError(f"suite validation failed for {profile}: {completed.stdout}{completed.stderr}")
            results.append(verify_manifest(profile, root, lock))
    except CorpusError as error:
        print(f"compatibility corpus error: {error}", file=sys.stderr)
        return 2
    print(json.dumps({"suite_commit": lock["suite"]["commit"], "profiles": results}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
