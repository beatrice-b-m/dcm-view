#!/usr/bin/env python3
"""Resolve manifest rows through the versioned DICOM support policy."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_POLICY = HERE / "support-policy.json"
CLASSIFICATIONS = {"pixel_faithful_interactive", "pixel_preview", "metadata_reference_navigation", "controlled_unsupported", "out_of_scope", "temporarily_unverified"}


class PolicyError(RuntimeError):
    pass


def canonical_json(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def load_policy(path: Path = DEFAULT_POLICY) -> dict[str, Any]:
    try:
        policy = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PolicyError(f"cannot read policy {path}: {error}") from error
    rules = policy.get("rules") if isinstance(policy, dict) else None
    if policy.get("policy_version") != "1.0.0" or not isinstance(rules, list) or not rules:
        raise PolicyError("unsupported or malformed support policy")
    ids: set[str] = set()
    for rule in rules:
        required = {"id", "precedence", "match", "classification", "required_assertions", "semantic_context_assertions", "expected_unsupported", "rationale", "exclusions"}
        allowed = required | {"conditional_assertions"}
        if not isinstance(rule, dict) or set(rule) - allowed or not required <= set(rule):
            raise PolicyError("policy rule does not conform to the versioned schema")
        if rule["id"] in ids or rule["classification"] not in CLASSIFICATIONS:
            raise PolicyError(f"duplicate rule or invalid classification: {rule.get('id')}")
        ids.add(rule["id"])
    return policy


def policy_sha256(policy: dict[str, Any]) -> str:
    return hashlib.sha256(canonical_json(policy)).hexdigest()


def _matches(match: dict[str, Any], row: dict[str, Any], profile: str) -> bool:
    dicom = row.get("dicom") or {}
    image = row.get("image")
    fields = {
        "profiles": profile,
        "case_ids": row.get("case_id"),
        "sop_class_uids": dicom.get("sop_class_uid"),
        "transfer_syntax_uids": dicom.get("transfer_syntax_uid"),
        "photometric_interpretations": (image or {}).get("photometric_interpretation"),
        "bits_allocated": (image or {}).get("bits_allocated"),
    }
    for key, actual in fields.items():
        if key in match and actual not in match[key]:
            return False
    if "case_id_prefixes" in match and not any(str(row.get("case_id", "")).startswith(prefix) for prefix in match["case_id_prefixes"]):
        return False
    if "has_image" in match and bool(image) is not match["has_image"]:
        return False
    capabilities = set(row.get("expected_capabilities", []))
    if "capabilities_all" in match and not set(match["capabilities_all"]) <= capabilities:
        return False
    if "capabilities_any" in match and not set(match["capabilities_any"]) & capabilities:
        return False
    return True


def resolve(policy: dict[str, Any], row: dict[str, Any], profile: str) -> dict[str, Any]:
    matches = [rule for rule in policy["rules"] if _matches(rule["match"], row, profile)]
    if not matches:
        raise PolicyError(f"no policy outcome for {profile}:{row.get('case_id')}")
    highest = max(rule["precedence"] for rule in matches)
    winners = [rule for rule in matches if rule["precedence"] == highest]
    if len(winners) != 1:
        raise PolicyError(f"ambiguous policy outcome for {profile}:{row.get('case_id')}: {[rule['id'] for rule in winners]}")
    return winners[0]


def audit_manifests(policy: dict[str, Any], roots: list[Path]) -> dict[str, Any]:
    counts: dict[str, int] = {}
    cases = 0
    for root in roots:
        manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
        profile = manifest["run"]["profile"]
        rows = list(manifest.get("files", [])) + list(manifest.get("qualifications", []))
        for row in rows:
            rule = resolve(policy, row, profile)
            counts[rule["classification"]] = counts.get(rule["classification"], 0) + 1
            cases += 1
    return {"policy_sha256": policy_sha256(policy), "rows": cases, "classifications": counts}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--policy", type=Path, default=DEFAULT_POLICY)
    parser.add_argument("manifest_roots", type=Path, nargs="+")
    args = parser.parse_args(argv)
    try:
        print(json.dumps(audit_manifests(load_policy(args.policy), args.manifest_roots), sort_keys=True))
    except (PolicyError, OSError, json.JSONDecodeError, KeyError) as error:
        print(f"support policy error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
