#!/usr/bin/env python3
"""Fetch, capture, verify, and publish release marketing media."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCES = REPO_ROOT / "marketing" / "sources.json"
DEFAULT_CAPTURES = REPO_ROOT / "marketing" / "captures.json"
DEFAULT_SOURCE_ROOT = REPO_ROOT / "marketing-source-data"
DEFAULT_REVIEW_ROOT = REPO_ROOT / "marketing-review"
SOURCE_INVENTORY_NAME = "SOURCE_FILES.json"


class MarketingMediaError(RuntimeError):
	"""The requested media workflow could not establish its invariants."""


def load_json(path: Path) -> dict[str, Any]:
	try:
		value = json.loads(path.read_text(encoding="utf-8"))
	except FileNotFoundError as error:
		raise MarketingMediaError(f"required manifest does not exist: {path}") from error
	except json.JSONDecodeError as error:
		raise MarketingMediaError(f"invalid JSON in {path}: {error}") from error
	if not isinstance(value, dict):
		raise MarketingMediaError(f"manifest root must be an object: {path}")
	return value


def sha256_file(path: Path) -> str:
	hasher = hashlib.sha256()
	with path.open("rb") as source:
		for chunk in iter(lambda: source.read(1024 * 1024), b""):
			hasher.update(chunk)
	return hasher.hexdigest()


def manifest_sha256(path: Path) -> str:
	return sha256_file(path)


def require_string(value: Any, field: str) -> str:
	if not isinstance(value, str) or not value.strip():
		raise MarketingMediaError(f"{field} must be a non-empty string")
	return value


def require_positive_int(value: Any, field: str) -> int:
	if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
		raise MarketingMediaError(f"{field} must be a positive integer")
	return value


def source_groups(manifest: dict[str, Any]) -> list[dict[str, Any]]:
	if manifest.get("schema_version") != 1:
		raise MarketingMediaError("marketing source manifest schema_version must be 1")
	groups = manifest.get("groups")
	if not isinstance(groups, list) or not groups:
		raise MarketingMediaError("marketing source manifest must contain groups")

	seen_groups: set[str] = set()
	seen_series: set[str] = set()
	validated: list[dict[str, Any]] = []
	for group_index, raw_group in enumerate(groups):
		if not isinstance(raw_group, dict):
			raise MarketingMediaError(f"groups[{group_index}] must be an object")
		group_id = require_string(raw_group.get("id"), f"groups[{group_index}].id")
		if group_id in seen_groups:
			raise MarketingMediaError(f"duplicate marketing source group: {group_id}")
		seen_groups.add(group_id)
		for field in ("dataset_title", "attribution_party", "year_version", "doi"):
			require_string(raw_group.get(field), f"groups[{group_index}].{field}")
		patient_ids = raw_group.get("patient_ids")
		if not isinstance(patient_ids, list) or not all(
			isinstance(value, str) and value for value in patient_ids
		):
			raise MarketingMediaError(f"groups[{group_index}].patient_ids must be strings")
		series = raw_group.get("series")
		if not isinstance(series, list) or not series:
			raise MarketingMediaError(f"groups[{group_index}].series must not be empty")
		seen_roles: set[str] = set()
		for series_index, raw_series in enumerate(series):
			if not isinstance(raw_series, dict):
				raise MarketingMediaError(
					f"groups[{group_index}].series[{series_index}] must be an object"
				)
			prefix = f"groups[{group_index}].series[{series_index}]"
			role = require_string(raw_series.get("role"), f"{prefix}.role")
			uid = require_string(
				raw_series.get("series_instance_uid"), f"{prefix}.series_instance_uid"
			)
			require_positive_int(raw_series.get("expected_files"), f"{prefix}.expected_files")
			if role in seen_roles:
				raise MarketingMediaError(f"duplicate series role {role!r} in group {group_id}")
			if uid in seen_series:
				raise MarketingMediaError(f"duplicate Series Instance UID: {uid}")
			seen_roles.add(role)
			seen_series.add(uid)
		validated.append(raw_group)
	return validated


def select_groups(groups: Sequence[dict[str, Any]], requested: Sequence[str]) -> list[dict[str, Any]]:
	if not requested:
		return list(groups)
	by_id = {str(group["id"]): group for group in groups}
	unknown = sorted(set(requested) - by_id.keys())
	if unknown:
		raise MarketingMediaError(f"unknown source group(s): {', '.join(unknown)}")
	return [by_id[group_id] for group_id in requested]


def ensure_ignored(path: Path) -> None:
	path.mkdir(parents=True, exist_ok=True)
	probe = path / ".dcmview-ignore-probe"
	result = subprocess.run(
		["git", "check-ignore", "--quiet", str(probe)],
		cwd=REPO_ROOT,
		check=False,
	)
	if result.returncode != 0:
		raise MarketingMediaError(
			f"source root is not ignored by git; refusing to download DICOM data: {path}"
		)


def dicom_files(path: Path) -> list[Path]:
	return sorted(
		candidate
		for candidate in path.rglob("*")
		if candidate.is_file() and candidate.suffix.casefold() in {".dcm", ".dicom"}
	)


def inventory_series(
	*,
	source_root: Path,
	group: dict[str, Any],
	series: dict[str, Any],
) -> list[dict[str, Any]]:
	series_root = source_root / str(group["id"]) / str(series["series_instance_uid"])
	files = dicom_files(series_root)
	expected = int(series["expected_files"])
	if len(files) != expected:
		raise MarketingMediaError(
			f"{group['id']}/{series['role']} expected {expected} DICOM files, found {len(files)}"
		)
	return [
		{
			"path": path.relative_to(source_root).as_posix(),
			"bytes": path.stat().st_size,
			"sha256": sha256_file(path),
			"group": group["id"],
			"series_role": series["role"],
			"series_instance_uid": series["series_instance_uid"],
		}
		for path in files
	]


def write_source_inventory(
	*,
	source_root: Path,
	sources_path: Path,
	groups: Sequence[dict[str, Any]],
) -> Path:
	entries = [
		entry
		for group in groups
		for series in group["series"]
		for entry in inventory_series(source_root=source_root, group=group, series=series)
	]
	inventory = {
		"schema_version": 1,
		"sources_manifest_sha256": manifest_sha256(sources_path),
		"file_count": len(entries),
		"total_bytes": sum(int(entry["bytes"]) for entry in entries),
		"files": entries,
	}
	destination = source_root / SOURCE_INVENTORY_NAME
	destination.write_text(json.dumps(inventory, indent=2, sort_keys=True) + "\n", encoding="utf-8")
	return destination


def require_executable(name: str, installation_hint: str) -> str:
	resolved = shutil.which(name)
	if resolved is None:
		raise MarketingMediaError(f"required executable is unavailable: {name}\n{installation_hint}")
	return resolved


def fetch_sources(args: argparse.Namespace) -> None:
	sources_path = args.sources.resolve()
	manifest = load_json(sources_path)
	groups = select_groups(source_groups(manifest), args.group)
	source_root = args.source_root.resolve()
	ensure_ignored(source_root)
	idc = require_executable(
		"idc",
		"Install the pinned dependency with: python -m pip install -r marketing/requirements.txt",
	)

	for group in groups:
		for series in group["series"]:
			destination = source_root / str(group["id"])
			destination.mkdir(parents=True, exist_ok=True)
			command = [
				idc,
				"download-from-selection",
				"--series-instance-uid",
				str(series["series_instance_uid"]),
				"--download-dir",
				str(destination),
				"--dir-template",
				"%SeriesInstanceUID",
				"--use-s5cmd-sync",
			]
			print(f"\n==> Fetch {group['id']} / {series['role']}", flush=True)
			subprocess.run(command, cwd=REPO_ROOT, check=True)

	inventory_path = write_source_inventory(
		source_root=source_root,
		sources_path=sources_path,
		groups=groups,
	)
	print(f"wrote source inventory: {inventory_path}")


def verify_source_inventory(args: argparse.Namespace) -> None:
	sources_path = args.sources.resolve()
	manifest = load_json(sources_path)
	groups = select_groups(source_groups(manifest), args.group)
	source_root = args.source_root.resolve()
	ensure_ignored(source_root)
	inventory_path = source_root / SOURCE_INVENTORY_NAME
	inventory = load_json(inventory_path)
	if inventory.get("schema_version") != 1:
		raise MarketingMediaError("source inventory schema_version must be 1")
	if inventory.get("sources_manifest_sha256") != manifest_sha256(sources_path):
		raise MarketingMediaError("source inventory was generated from a different sources manifest")
	recorded = inventory.get("files")
	if not isinstance(recorded, list):
		raise MarketingMediaError("source inventory files must be an array")
	selected_group_ids = {str(group["id"]) for group in groups}
	selected_records = [entry for entry in recorded if entry.get("group") in selected_group_ids]
	expected_count = sum(
		int(series["expected_files"])
		for group in groups
		for series in group["series"]
	)
	if len(selected_records) != expected_count:
		raise MarketingMediaError(
			f"source inventory expected {expected_count} selected files, found {len(selected_records)}"
		)

	errors: list[str] = []
	for entry in selected_records:
		relative = entry.get("path")
		if not isinstance(relative, str):
			errors.append("inventory entry has no path")
			continue
		path = source_root / relative
		if not path.is_file():
			errors.append(f"missing: {relative}")
			continue
		actual_size = path.stat().st_size
		if actual_size != entry.get("bytes"):
			errors.append(f"size mismatch: {relative}")
			continue
		if sha256_file(path) != entry.get("sha256"):
			errors.append(f"SHA-256 mismatch: {relative}")
	if errors:
		raise MarketingMediaError("source verification failed:\n  - " + "\n  - ".join(errors))
	print(f"verified {len(selected_records)} DICOM source files against {inventory_path}")


def add_common_manifest_arguments(parser: argparse.ArgumentParser) -> None:
	parser.add_argument("--sources", type=Path, default=DEFAULT_SOURCES)
	parser.add_argument("--source-root", type=Path, default=DEFAULT_SOURCE_ROOT)
	parser.add_argument("--group", action="append", default=[], help="Source group ID; repeatable")


def build_parser() -> argparse.ArgumentParser:
	parser = argparse.ArgumentParser(description=__doc__)
	subparsers = parser.add_subparsers(dest="command", required=True)

	fetch = subparsers.add_parser("fetch", help="Download and inventory pinned IDC series")
	add_common_manifest_arguments(fetch)
	fetch.set_defaults(handler=fetch_sources)

	verify_sources = subparsers.add_parser(
		"verify-sources", help="Verify downloaded source files against their local inventory"
	)
	add_common_manifest_arguments(verify_sources)
	verify_sources.set_defaults(handler=verify_source_inventory)
	return parser


def main(argv: Sequence[str] | None = None) -> int:
	args = build_parser().parse_args(argv)
	try:
		args.handler(args)
	except (MarketingMediaError, subprocess.CalledProcessError) as error:
		print(f"marketing media error: {error}", file=sys.stderr)
		return 1
	return 0


if __name__ == "__main__":
	raise SystemExit(main())
