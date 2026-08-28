#!/usr/bin/env python3
"""Execute a bounded, manifest-driven dcmview HTTP compatibility campaign."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import signal
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
import zlib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, BinaryIO, Optional

try:
    from scripts.compatibility.scope import ScopeError, sha256_file
except ModuleNotFoundError:
    from scope import ScopeError, sha256_file


DETAIL_SCHEMA_VERSION = "0.1.0"
EXECUTION_OUTCOMES = {"safe", "timeout", "crash", "flaky"}
COMPATIBILITY_OUTCOMES = {
    "full_support",
    "metadata_only",
    "known_gap",
    "failure",
    "unavailable",
}
PROBED_CAPABILITIES = {
    "open_file",
    "read_metadata",
    "render_native_pixels",
    "render_compressed_pixels",
    "navigate_multiframe",
}
LOSSY_TRANSFER_SYNTAXES = {
    "1.2.840.10008.1.2.4.50",
    "1.2.840.10008.1.2.4.51",
}
RAW_HEADERS = (
    "x-frame-rows",
    "x-frame-columns",
    "x-frame-bits-allocated",
    "x-frame-pixel-representation",
    "x-frame-samples-per-pixel",
    "x-frame-photometric-interpretation",
    "x-frame-rescale-slope",
    "x-frame-rescale-intercept",
    "x-frame-default-wc",
    "x-frame-default-ww",
)


class CampaignError(RuntimeError):
    """Raised when a campaign invariant cannot be established."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def json_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def artifact(path: Path, kind: str) -> dict[str, Any]:
    return {
        "kind": kind,
        "path": str(path.resolve()),
        "sha256": sha256_file(path),
        "size_bytes": path.stat().st_size,
    }


class CapturedProcess:
    def __init__(self, command: list[str], environment: dict[str, str], directory: Path):
        self.command = command
        self.directory = directory
        self.process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=directory,
            env=environment,
            start_new_session=True,
            bufsize=0,
        )
        assert self.process.stdout is not None and self.process.stderr is not None
        self._stdout = bytearray()
        self._stderr = bytearray()
        self._condition = threading.Condition()
        self._threads = [
            threading.Thread(target=self._drain, args=(self.process.stdout, self._stdout), daemon=True),
            threading.Thread(target=self._drain, args=(self.process.stderr, self._stderr), daemon=True),
        ]
        for thread in self._threads:
            thread.start()

    def _drain(self, stream: BinaryIO, target: bytearray) -> None:
        while True:
            chunk = stream.read(4096)
            if not chunk:
                break
            with self._condition:
                target.extend(chunk)
                self._condition.notify_all()

    def wait_for_startup_url(self, timeout: float) -> str:
        deadline = time.monotonic() + timeout
        consumed = 0
        while time.monotonic() < deadline:
            with self._condition:
                data = bytes(self._stdout)
                lines = data[consumed:].splitlines(keepends=True)
                complete = lines if data.endswith((b"\n", b"\r")) else lines[:-1]
                for line in complete:
                    consumed += len(line)
                    try:
                        event = json.loads(line.decode("utf-8"))
                    except (UnicodeDecodeError, json.JSONDecodeError):
                        continue
                    if event.get("type") == "server_started" and isinstance(event.get("url"), str):
                        return event["url"].rstrip("/")
                if self.process.poll() is not None:
                    raise CampaignError(
                        f"dcmview exited before startup with code {self.process.returncode}"
                    )
                self._condition.wait(timeout=min(0.1, max(0.0, deadline - time.monotonic())))
        raise TimeoutError(f"dcmview startup exceeded {timeout:.1f}s")

    def shutdown(self, timeout: float) -> tuple[int, bool]:
        forced = False
        if self.process.poll() is None:
            os.killpg(self.process.pid, signal.SIGTERM)
            try:
                self.process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                forced = True
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=max(timeout, 1.0))
        for thread in self._threads:
            thread.join(timeout=1.0)
        return int(self.process.returncode or 0), forced

    def write_logs(self, stdout_path: Path, stderr_path: Path) -> None:
        stdout_path.write_bytes(bytes(self._stdout))
        stderr_path.write_bytes(bytes(self._stderr))


def http_request(base_url: str, path: str, timeout: float) -> dict[str, Any]:
    started = time.monotonic()
    request = urllib.request.Request(f"{base_url}{path}", method="GET")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read()
            status = response.status
            headers = {key.lower(): value for key, value in response.headers.items()}
    except urllib.error.HTTPError as error:
        body = error.read()
        status = error.code
        headers = {key.lower(): value for key, value in error.headers.items()}
    elapsed = round((time.monotonic() - started) * 1000, 3)
    parsed: Any = None
    if headers.get("content-type", "").split(";", 1)[0] == "application/json":
        try:
            parsed = json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            parsed = None
    return {
        "path": path,
        "status": status,
        "content_type": headers.get("content-type"),
        "headers": headers,
        "body": body,
        "json": parsed,
        "body_sha256": hashlib.sha256(body).hexdigest(),
        "elapsed_ms": elapsed,
    }


def evidence(response: dict[str, Any], header_names: tuple[str, ...] = ()) -> dict[str, Any]:
    return {
        "path": response["path"],
        "status": response["status"],
        "content_type": response["content_type"],
        "headers": {
            name: response["headers"][name]
            for name in header_names
            if name in response["headers"]
        },
        "body_sha256": response["body_sha256"],
        "size_bytes": len(response["body"]),
        "elapsed_ms": response["elapsed_ms"],
    }


def png_pixels(payload: bytes) -> tuple[int, int, list[tuple[int, int, int, int]]]:
    if payload[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("response is not a PNG")
    offset = 8
    width = height = bit_depth = color_type = None
    compressed = bytearray()
    while offset + 12 <= len(payload):
        length = struct.unpack(">I", payload[offset : offset + 4])[0]
        kind = payload[offset + 4 : offset + 8]
        data = payload[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", data
            )
            if compression or filtering or interlace:
                raise ValueError("unsupported PNG encoding")
        elif kind == b"IDAT":
            compressed.extend(data)
        elif kind == b"IEND":
            break
    if None in (width, height, bit_depth, color_type) or bit_depth != 8:
        raise ValueError("unsupported PNG header")
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color_type)
    if channels is None:
        raise ValueError(f"unsupported PNG color type {color_type}")
    raw = zlib.decompress(bytes(compressed))
    stride = int(width) * channels
    rows: list[bytes] = []
    prior = bytes(stride)
    cursor = 0
    for _ in range(int(height)):
        filter_type = raw[cursor]
        scanline = bytearray(raw[cursor + 1 : cursor + 1 + stride])
        cursor += stride + 1
        for index in range(stride):
            left = scanline[index - channels] if index >= channels else 0
            above = prior[index]
            upper_left = prior[index - channels] if index >= channels else 0
            if filter_type == 1:
                scanline[index] = (scanline[index] + left) & 255
            elif filter_type == 2:
                scanline[index] = (scanline[index] + above) & 255
            elif filter_type == 3:
                scanline[index] = (scanline[index] + ((left + above) // 2)) & 255
            elif filter_type == 4:
                estimate = left + above - upper_left
                distances = (abs(estimate - left), abs(estimate - above), abs(estimate - upper_left))
                predictor = (left, above, upper_left)[distances.index(min(distances))]
                scanline[index] = (scanline[index] + predictor) & 255
            elif filter_type != 0:
                raise ValueError(f"unsupported PNG filter {filter_type}")
        prior = bytes(scanline)
        rows.append(prior)
    pixels: list[tuple[int, int, int, int]] = []
    for row in rows:
        for index in range(0, len(row), channels):
            values = row[index : index + channels]
            if color_type == 0:
                pixels.append((values[0], values[0], values[0], 255))
            elif color_type == 2:
                pixels.append((values[0], values[1], values[2], 255))
            elif color_type == 4:
                pixels.append((values[0], values[0], values[0], values[1]))
            else:
                pixels.append(tuple(values))  # type: ignore[arg-type]
    return int(width), int(height), pixels


def validate_visual(pattern: Optional[str], pixels: list[tuple[int, int, int, int]]) -> dict[str, Any]:
    if not pattern:
        return {"status": "not_declared", "pattern": None}
    luminance = [round(0.2126 * r + 0.7152 * g + 0.0722 * b, 3) for r, g, b, _ in pixels]
    if pattern == "2x2_monochrome_gradient":
        passed = len(luminance) == 4 and luminance == sorted(luminance)
    elif pattern == "2x2_inverse_monochrome_gradient":
        passed = len(luminance) == 4 and luminance == sorted(luminance, reverse=True)
    elif pattern == "2x2_rgb_red_green_blue_white":
        passed = len(pixels) == 4 and (
            pixels[0][0] > pixels[0][1] and pixels[0][0] > pixels[0][2]
            and pixels[1][1] > pixels[1][0] and pixels[1][1] > pixels[1][2]
            and pixels[2][2] > pixels[2][0] and pixels[2][2] > pixels[2][1]
            and min(pixels[3][:3]) > 200
        )
    else:
        return {"status": "unautomated", "pattern": pattern}
    return {"status": "passed" if passed else "failed", "pattern": pattern}


def _normalized_path(value: str) -> str:
    return os.path.normcase(str(Path(value).resolve()))


def select_entries(worklist: dict[str, Any], root: Optional[str]) -> list[dict[str, Any]]:
    selected: list[dict[str, Any]] = []
    for entry in worklist["canonical_files"]:
        occurrence = None
        if root is None:
            occurrence = entry["selected"]
        else:
            occurrence = next(
                (row for row in entry["occurrences"] if row["root"] == root), None
            )
        if occurrence is not None:
            selected.append({**entry, "campaign_occurrence": occurrence})
    return sorted(
        selected,
        key=lambda row: (
            row["campaign_occurrence"]["root"],
            row["case_id"],
            row["campaign_occurrence"]["path"],
        ),
    )


def poll_files(base_url: str, request_timeout: float, startup_deadline: float) -> dict[str, Any]:
    while time.monotonic() < startup_deadline:
        response = http_request(base_url, "/api/files", request_timeout)
        if response["status"] == 200 and isinstance(response["json"], dict):
            if response["json"].get("scan_complete") is True:
                return response["json"]
        time.sleep(0.05)
    raise TimeoutError("discovery did not report scan_complete before startup deadline")


def probe_case(
    base_url: str,
    entry: dict[str, Any],
    file_summary: dict[str, Any],
    request_timeout: float,
    case_timeout: float,
) -> dict[str, Any]:
    started = time.monotonic()
    index = file_summary["index"]
    frame_count = int(file_summary["frame_count"])
    has_pixels = bool(file_summary["has_pixels"])
    expected = entry["expected_contract"]
    expected_capabilities = sorted(expected.get("expected_capabilities") or [])
    image = expected.get("image") or {}
    pixel_data = expected.get("pixel_data") or {}
    checks: dict[str, Any] = {"mapped_after_scan": True}
    http: dict[str, Any] = {}
    errors: list[dict[str, Any]] = []

    def request(name: str, path: str, headers: tuple[str, ...] = ()) -> dict[str, Any]:
        if time.monotonic() - started >= case_timeout:
            raise TimeoutError(f"case exceeded {case_timeout:.1f}s")
        response = http_request(base_url, path, min(request_timeout, case_timeout))
        http[name] = evidence(response, headers)
        return response

    try:
        info = request("info", f"/api/file/{index}/info")
        tags = request("tags", f"/api/file/{index}/tags")
        checks["file_info"] = info["status"] == 200 and isinstance(info["json"], dict)
        checks["tags"] = tags["status"] == 200 and isinstance(tags["json"], list)
        invalid = request("invalid_frame", f"/api/file/{index}/frame/{frame_count}")
        checks["error_envelope"] = (
            invalid["status"] in {400, 404, 422}
            and isinstance(invalid["json"], dict)
            and isinstance(invalid["json"].get("error"), str)
        )
        if has_pixels and frame_count > 0:
            display_first = request(
                "display_first", f"/api/file/{index}/frame/0", ("x-cache",)
            )
            display_second = request(
                "display_second", f"/api/file/{index}/frame/0", ("x-cache",)
            )
            checks["display_cache"] = (
                display_first["headers"].get("x-cache") == "MISS"
                and display_second["headers"].get("x-cache") == "HIT"
            )
            checks["display_body_stable"] = (
                display_first["body_sha256"] == display_second["body_sha256"]
            )
            if display_first["status"] == 200:
                try:
                    width, height, pixels = png_pixels(display_first["body"])
                    checks["png_dimensions"] = {
                        "expected": [image.get("columns"), image.get("rows")],
                        "observed": [width, height],
                        "passed": width == image.get("columns") and height == image.get("rows"),
                    }
                    checks["visual"] = validate_visual(
                        (expected.get("expected_visual_checks") or {}).get("pattern"), pixels
                    )
                except (ValueError, zlib.error) as error:
                    checks["png_dimensions"] = {"passed": False, "error": str(error)}
            raw_first = request(
                "raw_first", f"/api/file/{index}/frame/0/raw", ("x-cache",) + RAW_HEADERS
            )
            raw_second = request(
                "raw_second", f"/api/file/{index}/frame/0/raw", ("x-cache",) + RAW_HEADERS
            )
            checks["raw_cache"] = (
                raw_first["headers"].get("x-cache") == "MISS"
                and raw_second["headers"].get("x-cache") == "HIT"
            )
            expected_hashes = pixel_data.get("frame_hashes") or []
            transfer_syntax = (expected.get("dicom") or {}).get("transfer_syntax_uid")
            if expected_hashes and transfer_syntax not in LOSSY_TRANSFER_SYNTAXES:
                checks["lossless_frame_hash"] = {
                    "expected": expected_hashes[0],
                    "observed": raw_first["body_sha256"] if raw_first["status"] == 200 else None,
                    "passed": raw_first["status"] == 200
                    and raw_first["body_sha256"] == expected_hashes[0],
                }
        else:
            checks["metadata_only_response"] = invalid["status"] == 404
    except TimeoutError as error:
        errors.append({"code": "case_timeout", "message": str(error)})
        return _finish_result(entry, expected_capabilities, checks, http, errors, started, "timeout")
    except (OSError, urllib.error.URLError) as error:
        errors.append({"code": "request_failure", "message": str(error)})
        return _finish_result(entry, expected_capabilities, checks, http, errors, started, "safe")
    return _finish_result(entry, expected_capabilities, checks, http, errors, started, "safe")


def _finish_result(
    entry: dict[str, Any],
    expected_capabilities: list[str],
    checks: dict[str, Any],
    http: dict[str, Any],
    errors: list[dict[str, Any]],
    started: float,
    safety: str,
) -> dict[str, Any]:
    occurrence = entry["campaign_occurrence"]
    unprobed = sorted(set(expected_capabilities) - PROBED_CAPABILITIES)
    statuses = [row["status"] for row in http.values()]
    controlled_gap = any(status == 422 for status in statuses)
    server_failure = any(status >= 500 for status in statuses)
    required_checks = [checks.get("mapped_after_scan"), checks.get("file_info"), checks.get("tags")]
    if safety != "safe" or server_failure or not all(required_checks):
        compatibility = "failure"
    elif unprobed or controlled_gap:
        compatibility = "known_gap"
    elif checks.get("metadata_only_response"):
        compatibility = "metadata_only"
    else:
        validation_failures = []
        for name in ("display_cache", "display_body_stable", "raw_cache"):
            if name in checks and checks[name] is not True:
                validation_failures.append(name)
        for name in ("png_dimensions", "lossless_frame_hash"):
            if name in checks and checks[name].get("passed") is not True:
                validation_failures.append(name)
        visual = checks.get("visual")
        if visual and visual["status"] in {"failed", "unautomated"}:
            validation_failures.append("visual")
        compatibility = "known_gap" if validation_failures else "full_support"
    return {
        "root": occurrence["root"],
        "case_id": occurrence["case_id"],
        "path": occurrence["path"],
        "identity": {
            "normalized_path": occurrence["normalized_path"],
            "sop_instance_uid": entry["sop_instance_uid"],
            "file_sha256": entry["sha256"],
            "contract_sha256": entry["contract_sha256"],
        },
        "execution_safety": safety,
        "compatibility": compatibility,
        "expected_capabilities": expected_capabilities,
        "unprobed_capabilities": unprobed,
        "observations": checks,
        "http": http,
        "timings_ms": {"total": round((time.monotonic() - started) * 1000, 3)},
        "errors": errors,
    }


def validate_report(report: dict[str, Any]) -> None:
    required = {
        "detail_schema_version", "generated_at", "worklist", "viewer", "run",
        "results", "summary", "artifacts", "validation",
    }
    if set(report) != required:
        raise CampaignError(f"report fields do not match companion schema: {sorted(set(report) ^ required)}")
    if report["detail_schema_version"] != DETAIL_SCHEMA_VERSION:
        raise CampaignError("unexpected detail schema version")
    seen: set[tuple[str, str, str]] = set()
    for result in report["results"]:
        key = (result["root"], result["case_id"], result["path"])
        if key in seen:
            raise CampaignError(f"duplicate result identity: {key}")
        seen.add(key)
        if result["execution_safety"] not in EXECUTION_OUTCOMES:
            raise CampaignError(f"invalid execution outcome: {key}")
        if result["compatibility"] not in COMPATIBILITY_OUTCOMES:
            raise CampaignError(f"invalid compatibility outcome: {key}")
        if len(result["identity"]["file_sha256"]) != 64:
            raise CampaignError(f"invalid file identity: {key}")
    if report["summary"]["results"] != len(report["results"]):
        raise CampaignError("summary/result count mismatch")


def validate_json_schema(
    instance: Any,
    schema: dict[str, Any],
    root_schema: Optional[dict[str, Any]] = None,
    path: str = "$",
) -> None:
    """Validate the strict JSON Schema subset used by the detail contract."""
    root = schema if root_schema is None else root_schema
    reference = schema.get("$ref")
    if reference is not None:
        if not reference.startswith("#/"):
            raise CampaignError(f"unsupported external schema reference at {path}: {reference}")
        target: Any = root
        for component in reference[2:].split("/"):
            target = target[component.replace("~1", "/").replace("~0", "~")]
        validate_json_schema(instance, target, root, path)
        return
    if "const" in schema and instance != schema["const"]:
        raise CampaignError(f"schema const violation at {path}")
    expected_type = schema.get("type")
    if expected_type is not None:
        accepted = expected_type if isinstance(expected_type, list) else [expected_type]
        predicates = {
            "object": lambda value: isinstance(value, dict),
            "array": lambda value: isinstance(value, list),
            "string": lambda value: isinstance(value, str),
            "integer": lambda value: isinstance(value, int) and not isinstance(value, bool),
            "number": lambda value: isinstance(value, (int, float)) and not isinstance(value, bool),
            "boolean": lambda value: isinstance(value, bool),
            "null": lambda value: value is None,
        }
        if not any(predicates[kind](instance) for kind in accepted):
            raise CampaignError(f"schema type violation at {path}: expected {accepted}")
    if isinstance(instance, dict):
        required = schema.get("required", [])
        missing = [key for key in required if key not in instance]
        if missing:
            raise CampaignError(f"schema required-field violation at {path}: {missing}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            extra = sorted(set(instance) - set(properties))
            if extra:
                raise CampaignError(f"schema additional-field violation at {path}: {extra}")
        for key, value in instance.items():
            if key in properties:
                validate_json_schema(value, properties[key], root, f"{path}.{key}")
    if isinstance(instance, list) and "items" in schema:
        for index, value in enumerate(instance):
            validate_json_schema(value, schema["items"], root, f"{path}[{index}]")
    if isinstance(instance, str) and "pattern" in schema:
        if re.search(schema["pattern"], instance) is None:
            raise CampaignError(f"schema pattern violation at {path}")
    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            raise CampaignError(f"schema minimum violation at {path}")


def normalized_report(report: dict[str, Any]) -> dict[str, Any]:
    results = []
    for result in report["results"]:
        normalized_http = {
            name: {
                key: value
                for key, value in row.items()
                if key not in {"elapsed_ms"}
            }
            for name, row in result["http"].items()
        }
        for row in normalized_http.values():
            if "path" in row:
                row["path"] = re.sub(r"^/api/file/\d+", "/api/file/{mapped}", row["path"])
        results.append(
            {
                key: value
                for key, value in {**result, "http": normalized_http}.items()
                if key not in {"timings_ms"}
            }
        )
    return {
        "detail_schema_version": report["detail_schema_version"],
        "worklist_content_sha256": report["worklist"]["content_sha256"],
        "viewer_sha256": report["viewer"]["sha256"],
        "selection": report["run"]["selection"],
        "results": results,
        "summary": report["summary"],
    }


def _ensure_external_output(output: Path, suite_root: Path, viewer_root: Path) -> None:
    resolved = output.resolve()
    for repository in (suite_root.resolve(), viewer_root.resolve()):
        try:
            resolved.relative_to(repository)
        except ValueError:
            continue
        raise CampaignError(f"artifact output must be outside both repositories: {resolved}")
    if output.exists() and any(output.iterdir()):
        raise CampaignError(f"artifact output is not empty: {output}")
    output.mkdir(parents=True, exist_ok=True)


def run_campaign(args: argparse.Namespace) -> dict[str, Any]:
    viewer_root = Path(__file__).resolve().parents[2]
    suite_root = args.suite_root.resolve()
    output = args.output.resolve()
    _ensure_external_output(output, suite_root, viewer_root)
    worklist_path = args.worklist.resolve()
    worklist = json.loads(worklist_path.read_text(encoding="utf-8"))
    entries = select_entries(worklist, args.root)
    if not entries:
        raise CampaignError(f"selection contains no cases: {args.root or 'canonical'}")
    binary = args.binary.resolve()
    if not os.access(binary, os.X_OK):
        raise CampaignError(f"dcmview binary is not executable: {binary}")
    version = subprocess.run(
        [str(binary), "--version"], check=True, capture_output=True, text=True, timeout=5
    ).stdout.strip()
    input_paths = [row["campaign_occurrence"]["normalized_path"] for row in entries]
    command = [
        str(binary), "--no-browser", "--host", "127.0.0.1", "--port", "0",
        "--startup-json", *input_paths,
    ]
    environment = dict(os.environ)
    environment["DCMVIEW_VSCODE_BYPASS"] = "1"
    started_at = utc_now()
    shard_started = time.monotonic()
    process = CapturedProcess(command, environment, viewer_root)
    base_url = ""
    results: list[dict[str, Any]] = []
    shutdown_forced = False
    exit_code = -1
    campaign_error: Optional[str] = None
    try:
        base_url = process.wait_for_startup_url(args.startup_timeout)
        scan = poll_files(
            base_url,
            args.request_timeout,
            time.monotonic() + args.startup_timeout,
        )
        by_identity = {
            (_normalized_path(row["path"]), row["sop_instance_uid"]): row
            for row in scan["files"]
        }
        for entry in entries:
            if time.monotonic() - shard_started >= args.shard_timeout:
                raise TimeoutError(f"shard exceeded {args.shard_timeout:.1f}s")
            occurrence = entry["campaign_occurrence"]
            key = (_normalized_path(occurrence["normalized_path"]), entry["sop_instance_uid"])
            summary = by_identity.get(key)
            if summary is None:
                result = _finish_result(
                    entry,
                    sorted(entry["expected_contract"].get("expected_capabilities") or []),
                    {"mapped_after_scan": False},
                    {},
                    [{"code": "discovery_omission", "message": "path and SOP UID not found"}],
                    time.monotonic(),
                    "safe" if process.process.poll() is None else "crash",
                )
            else:
                result = probe_case(
                    base_url, entry, summary, args.request_timeout, args.case_timeout
                )
            if process.process.poll() is not None:
                result["execution_safety"] = "crash"
                result["compatibility"] = "failure"
            results.append(result)
    except (CampaignError, TimeoutError, OSError, urllib.error.URLError) as error:
        campaign_error = str(error)
    finally:
        exit_code, shutdown_forced = process.shutdown(args.shutdown_timeout)
        stdout_path = output / "stdout.log"
        stderr_path = output / "stderr.log"
        process.write_logs(stdout_path, stderr_path)

    counts = {
        outcome: sum(row["compatibility"] == outcome for row in results)
        for outcome in sorted(COMPATIBILITY_OUTCOMES)
    }
    safety_counts = {
        outcome: sum(row["execution_safety"] == outcome for row in results)
        for outcome in sorted(EXECUTION_OUTCOMES)
    }
    schema_path = Path(__file__).with_name("detail-schema.json")
    report = {
        "detail_schema_version": DETAIL_SCHEMA_VERSION,
        "generated_at": utc_now(),
        "worklist": {
            "path": str(worklist_path),
            "sha256": sha256_file(worklist_path),
            "content_sha256": worklist["worklist_sha256"],
        },
        "viewer": {"binary": str(binary), "sha256": sha256_file(binary), "version": version},
        "run": {
            "started_at": started_at,
            "completed_at": utc_now(),
            "selection": args.root or "canonical",
            "base_url": base_url,
            "command": command,
            "timeouts_seconds": {
                "startup": args.startup_timeout,
                "request": args.request_timeout,
                "case": args.case_timeout,
                "shard": args.shard_timeout,
                "shutdown": args.shutdown_timeout,
            },
            "exit_code": exit_code,
            "shutdown_forced": shutdown_forced,
            "campaign_error": campaign_error,
        },
        "results": results,
        "summary": {
            "selected": len(entries),
            "results": len(results),
            "compatibility": counts,
            "execution_safety": safety_counts,
            "campaign_complete": campaign_error is None and len(results) == len(entries),
        },
        "artifacts": [artifact(output / "stdout.log", "stdout"), artifact(output / "stderr.log", "stderr")],
        "validation": {
            "schema": "detail-schema.json",
            "schema_sha256": sha256_file(schema_path),
            "status": "passed",
        },
    }
    validate_report(report)
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    validate_json_schema(report, schema)
    report_path = output / "report.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    normalized = normalized_report(report)
    normalized_path = output / "normalized.json"
    normalized_path.write_text(
        json.dumps(normalized, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    index = {
        "artifacts": [
            artifact(path, kind)
            for path, kind in (
                (report_path, "report"),
                (normalized_path, "normalized_report"),
                (output / "stdout.log", "stdout"),
                (output / "stderr.log", "stderr"),
            )
        ]
    }
    (output / "artifact-index.json").write_text(
        json.dumps(index, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return report


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite-root", type=Path, default=os.environ.get("DCMVIEW_COMPAT_SUITE_ROOT"),
        required="DCMVIEW_COMPAT_SUITE_ROOT" not in os.environ,
    )
    parser.add_argument("--worklist", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--root", help="manifest root such as smoke; omit for canonical selection")
    parser.add_argument("--startup-timeout", type=float, default=20.0)
    parser.add_argument("--request-timeout", type=float, default=10.0)
    parser.add_argument("--case-timeout", type=float, default=60.0)
    parser.add_argument("--shard-timeout", type=float, default=1800.0)
    parser.add_argument("--shutdown-timeout", type=float, default=10.0)
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    try:
        report = run_campaign(parse_args(sys.argv[1:] if argv is None else argv))
    except (CampaignError, ScopeError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"compatibility campaign error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report["summary"], sort_keys=True))
    return 0 if report["summary"]["campaign_complete"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
